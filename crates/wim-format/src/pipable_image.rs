// SPDX-License-Identifier: LGPL-2.1-or-later
//! Sequential XML/image selection and metadata loading from upstream extract.c.
use crate::allocation::*;
use crate::{
    Header, ParseError,
    file_resource::FileReadError,
    metadata::Metadata,
    pipable_read::{BlobHeader, Frame, PipableReader},
    xml::XmlInfo,
};
use alloc::vec::Vec;
use sha1::{Digest, Sha1};
use std::io::Read;

/// Selected image state, retaining only XML and the selected metadata resource.
pub struct PipeImage {
    /// Initial archive header.
    pub header: Header,
    /// Image names and extraction byte estimates.
    pub xml: XmlInfo,
    /// One-based selected image.
    pub index: u32,
    /// Validated metadata bytes; file payloads remain unread in the pipe.
    pub metadata: Vec<u8>,
}
fn metadata_frame<R: Read>(reader: &mut PipableReader<R>) -> Result<BlobHeader, FileReadError> {
    match reader.next_frame(false)? {
        Frame::Blob(blob) if blob.flags & 2 != 0 => Ok(blob),
        _ => Err(ParseError::InvalidPipableWim.into()),
    }
}
fn collect<R: Read>(
    reader: &mut PipableReader<R>,
    blob: BlobHeader,
) -> Result<Vec<u8>, FileReadError> {
    let size = usize::try_from(blob.uncompressed_size).map_err(|_| ParseError::Nomem)?;
    let mut bytes = Vec::new();
    bytes.try_reserve(size).map_err(|_| ParseError::Nomem)?;
    reader.read_resource(blob, false, false, |chunk| {
        bytes
            .try_extend_from_slice(chunk)
            .map_err(|_| ParseError::Nomem)
    })?;
    Ok(bytes)
}
fn select_image<R: Read>(
    reader: &mut PipableReader<R>,
    selector: Option<&[u8]>,
) -> Result<(Header, XmlInfo, u32), FileReadError> {
    let header = reader.header().clone();
    if header.part_number != 1 {
        return Err(ParseError::InvalidPipableWim.into());
    }
    let xml_frame = metadata_frame(reader)?;
    let xml_bytes = collect(reader, xml_frame)?;
    let xml = XmlInfo::parse_utf16le(&xml_bytes)?;
    if xml.image_count() != header.image_count as usize {
        return Err(ParseError::ImageCount.into());
    }
    let index = match selector {
        Some(selector) => xml.resolve_image_bytes(Some(selector)),
        None if header.image_count == 1 => 1,
        None => 0,
    };
    if index <= 0 {
        return Err(ParseError::InvalidImage.into());
    }
    Ok((header, xml, index as u32))
}
/// Resolve a single image and consume its metadata without reading file payloads.
/// XML digests are not checked by the original; selected metadata digests are
/// checked and report metadata error 21. Unselected metadata is decoded/skipped.
pub fn read_image<R: Read>(
    reader: &mut PipableReader<R>,
    selector: Option<&[u8]>,
) -> Result<PipeImage, FileReadError> {
    let (header, xml, index) = select_image(reader, selector)?;
    let mut metadata = None;
    for image in 1..=header.image_count {
        let blob = metadata_frame(reader)?;
        if image == index {
            let bytes = collect(reader, blob)?;
            if <[u8; 20]>::from(Sha1::digest(bytes.as_slice())) != blob.hash {
                return Err(ParseError::InvalidMetadataResource.into());
            }
            Metadata::parse(&bytes)?;
            metadata = Some(bytes);
        } else {
            reader.skip_resource(blob)?;
        }
    }
    Ok(PipeImage {
        header,
        xml,
        index,
        metadata: metadata.ok_or(ParseError::MetadataNotFound)?,
    })
}

/// Consume a selected image with one retained allocated metadata graph.
/// The graph borrows local resource bytes for the entire callback, avoiding
/// self-referential ownership and a second validation/allocation pass. Remaining
/// image metadata is skipped after selected-tree validation, as upstream does.
pub fn with_image<R: Read, T>(
    reader: &mut PipableReader<R>,
    selector: Option<&[u8]>,
    consume: impl FnOnce(
        Header,
        XmlInfo,
        u32,
        &Metadata<'_>,
        &mut PipableReader<R>,
    ) -> Result<T, FileReadError>,
) -> Result<T, FileReadError> {
    let (header, xml, index) = select_image(reader, selector)?;
    for image in 1..=header.image_count {
        let blob = metadata_frame(reader)?;
        if image != index {
            reader.skip_resource(blob)?;
            continue;
        }
        let bytes = collect(reader, blob)?;
        if <[u8; 20]>::from(Sha1::digest(bytes.as_slice())) != blob.hash {
            return Err(ParseError::InvalidMetadataResource.into());
        }
        let metadata = Metadata::parse(&bytes)?;
        for _ in image + 1..=header.image_count {
            let blob = metadata_frame(reader)?;
            reader.skip_resource(blob)?;
        }
        return consume(header, xml, index, &metadata, reader);
    }
    Err(ParseError::MetadataNotFound.into())
}
