//! Native read-only archive composition. No filesystem extraction or C ABI.

use crate::{
    Header, ParseError,
    lookup::{LookupBlob, LookupTable},
    resource::{ResourceLayout, read_resource},
};
use alloc::vec::Vec;
use sha1::{Digest, Sha1};

/// A seekable WIM retained as borrowed bytes with resolved lookup records.
pub struct Archive<'a> {
    file: &'a [u8],
    /// Selected fixed header, including the final copy in pipable files.
    pub header: Header,
    /// Lookup table with metadata order and content hash identity.
    pub lookup: LookupTable,
}

impl<'a> Archive<'a> {
    /// Parse header and lookup records. XML/image metadata is validated lazily.
    pub fn open(file: &'a [u8]) -> Result<Self, ParseError> {
        let header = Header::parse_seekable(file)?;
        let compression = header.validate_compression()?;
        let layout = if header.magic == crate::PIPABLE_MAGIC {
            ResourceLayout::Pipable
        } else {
            ResourceLayout::Ordinary
        };
        let bytes = read_resource(
            file,
            &header.blob_table,
            compression,
            header.chunk_size,
            layout,
            |kind, input, output| decode(kind, input, output, header.chunk_size),
        )?;
        let lookup = LookupTable::parse(&bytes, &header, |offset| {
            let start = usize::try_from(offset).map_err(|_| ParseError::UnexpectedEndOfFile)?;
            let end = start
                .checked_add(16)
                .ok_or(ParseError::UnexpectedEndOfFile)?;
            let source = file
                .get(start..end)
                .ok_or(ParseError::UnexpectedEndOfFile)?;
            let mut raw = [0; 16];
            raw.copy_from_slice(source);
            Ok(raw)
        })?;
        Ok(Self {
            file,
            header,
            lookup,
        })
    }

    /// Read and verify the content identified by a SHA-1 digest.
    pub fn read_blob(&self, hash: &[u8; 20]) -> Result<Vec<u8>, ParseError> {
        let blob = self.lookup.find(hash).ok_or(ParseError::ResourceNotFound)?;
        self.read_descriptor(blob)
    }

    /// Read a blob byte range with at most one decoded chunk of scratch memory.
    /// Partial reads do not verify the whole-blob SHA-1; use [`Self::read_blob`]
    /// when whole-content integrity is required. The source file remains borrowed.
    pub fn read_blob_range(
        &self,
        hash: &[u8; 20],
        selection: core::ops::Range<u64>,
    ) -> Result<Vec<u8>, ParseError> {
        self.read_blob_range_with_decoder(hash, selection, decode)
    }

    /// Read a bounded blob range with caller-retained codec scratch.
    /// The callback receives the actual codec and resource chunk size; it must
    /// reset independent-block state and fill the supplied output or return an
    /// error. Partial reads do not verify the whole-blob digest.
    pub fn read_blob_range_with_decoder(
        &self,
        hash: &[u8; 20],
        selection: core::ops::Range<u64>,
        mut decoder: impl FnMut(crate::Compression, &[u8], &mut [u8], u32) -> Result<(), ParseError>,
    ) -> Result<Vec<u8>, ParseError> {
        let blob = self.lookup.find(hash).ok_or(ParseError::ResourceNotFound)?;
        if selection.start > selection.end || selection.end > blob.size {
            return Err(ParseError::InvalidParam);
        }
        let resource = self
            .lookup
            .resources
            .get(blob.resource_index)
            .ok_or(ParseError::InvalidLookupTableEntry)?;
        let start = blob
            .offset
            .checked_add(selection.start)
            .ok_or(ParseError::InvalidLookupTableEntry)?;
        let end = blob
            .offset
            .checked_add(selection.end)
            .ok_or(ParseError::InvalidLookupTableEntry)?;
        let layout = if resource.solid {
            ResourceLayout::Solid
        } else if self.header.magic == crate::PIPABLE_MAGIC {
            ResourceLayout::Pipable
        } else {
            ResourceLayout::Ordinary
        };
        crate::resource::read_resource_range(
            self.file,
            &resource.header,
            crate::Compression::from_i32(resource.compression_code as i32)?,
            resource.chunk_size,
            layout,
            start..end,
            |kind, input, output| decoder(kind, input, output, resource.chunk_size),
        )
    }

    /// Parse the XML resource and check image-count agreement with the header.
    /// This is explicit because [`Self::open`] currently resolves resources only.
    pub fn xml(&self) -> Result<crate::xml::XmlInfo, ParseError> {
        let bytes = self.xml_bytes()?;
        let xml = crate::xml::XmlInfo::parse_utf16le(&bytes)?;
        if xml.image_count() != self.header.image_count as usize {
            return Err(ParseError::ImageCount);
        }
        Ok(xml)
    }

    /// Read the XML resource bytes, retaining its original UTF-16 encoding.
    pub fn xml_bytes(&self) -> Result<Vec<u8>, ParseError> {
        let layout = if self.header.magic == crate::PIPABLE_MAGIC {
            ResourceLayout::Pipable
        } else {
            ResourceLayout::Ordinary
        };
        read_resource(
            self.file,
            &self.header.xml_data,
            self.header.validate_compression()?,
            self.header.chunk_size,
            layout,
            |kind, input, output| decode(kind, input, output, self.header.chunk_size),
        )
    }

    /// Verify the optional integrity table using its format-defined byte range.
    pub fn check_integrity(&self) -> Result<crate::integrity::IntegrityStatus, ParseError> {
        crate::integrity::check_wim_integrity(self.file, &self.header, |kind, input, output| {
            decode(kind, input, output, self.header.chunk_size)
        })
    }

    /// Read an image metadata resource by one-based image index, checking SHA-1.
    pub fn read_metadata(&self, image: u32) -> Result<Vec<u8>, ParseError> {
        let index = image.checked_sub(1).ok_or(ParseError::InvalidImage)? as usize;
        let blob = self
            .lookup
            .metadata
            .get(index)
            .ok_or(ParseError::InvalidImage)?;
        self.read_descriptor(blob)
    }

    fn read_descriptor(&self, blob: &LookupBlob) -> Result<Vec<u8>, ParseError> {
        let resource = self
            .lookup
            .resources
            .get(blob.resource_index)
            .ok_or(ParseError::InvalidLookupTableEntry)?;
        let compression = crate::Compression::from_i32(resource.compression_code as i32)?;
        let layout = if resource.solid {
            ResourceLayout::Solid
        } else if self.header.magic == crate::PIPABLE_MAGIC {
            ResourceLayout::Pipable
        } else {
            ResourceLayout::Ordinary
        };
        let output = read_resource(
            self.file,
            &resource.header,
            compression,
            resource.chunk_size,
            layout,
            |kind, input, output| decode(kind, input, output, resource.chunk_size),
        )?;
        let start =
            usize::try_from(blob.offset).map_err(|_| ParseError::InvalidLookupTableEntry)?;
        let size = usize::try_from(blob.size).map_err(|_| ParseError::InvalidLookupTableEntry)?;
        let end = start
            .checked_add(size)
            .ok_or(ParseError::InvalidLookupTableEntry)?;
        let bytes = output
            .get(start..end)
            .ok_or(ParseError::InvalidLookupTableEntry)?;
        let actual: [u8; 20] = Sha1::digest(bytes).into();
        if actual != blob.hash {
            return Err(ParseError::InvalidResourceHash);
        }
        if start == 0 && end == output.len() {
            return Ok(output);
        }
        let mut result = Vec::new();
        result
            .try_reserve_exact(size)
            .map_err(|_| ParseError::Nomem)?;
        result.extend_from_slice(bytes);
        Ok(result)
    }
}

pub(crate) fn decode(
    kind: crate::Compression,
    input: &[u8],
    output: &mut [u8],
    chunk_size: u32,
) -> Result<(), ParseError> {
    let accepted = match kind {
        crate::Compression::Xpress => ms_compress::decompress_xpress(input, output).is_ok(),
        crate::Compression::Lzx => {
            ms_compress::lzx::decompress_lzx(input, output, chunk_size as usize).is_ok()
        }
        crate::Compression::Lzms => match ms_compress::lzms::decompress_lzms(input, output) {
            Err(ms_compress::lzms::LzmsError::OutOfMemory) => return Err(ParseError::Nomem),
            result => result.is_ok(),
        },
        crate::Compression::None => false,
    };
    if accepted {
        Ok(())
    } else {
        Err(ParseError::Decompression)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn generated_original_resources_resolve_and_decode_in_all_layouts() {
        for bytes in [
            include_bytes!("../tests/fixtures/xpress-resource.wim").as_slice(),
            include_bytes!("../tests/fixtures/pipable-resource.wim").as_slice(),
            include_bytes!("../tests/fixtures/solid-resource.wim").as_slice(),
        ] {
            let archive = Archive::open(bytes).unwrap();
            assert_eq!(archive.xml().unwrap().image_count(), 1);
            let metadata = archive.read_metadata(1).unwrap();
            assert!(
                !crate::metadata::Metadata::parse(&metadata)
                    .unwrap()
                    .nodes
                    .is_empty()
            );
            let mut expected = (0..300).flat_map(|_| 0u8..=255).collect::<Vec<_>>();
            expected.extend_from_slice(b"last chunk");
            let hash: [u8; 20] = Sha1::digest(&expected).into();
            assert_eq!(archive.read_blob(&hash).unwrap(), expected);
            assert_eq!(
                archive.read_blob(&[0xff; 20]),
                Err(ParseError::ResourceNotFound)
            );
        }
    }
    #[test]
    fn content_digest_detects_raw_resource_corruption() {
        let mut bytes = include_bytes!("../tests/fixtures/xpress-resource.wim").to_vec();
        let a = Archive::open(&bytes).unwrap();
        let blob = &a.lookup.blobs[0];
        let hash = blob.hash;
        let offset = a.lookup.resources[blob.resource_index].header.offset_in_wim as usize;
        bytes[offset + 256] ^= 0x80;
        let a = Archive::open(&bytes).unwrap();
        assert!(a.read_blob(&hash).is_err());
    }
}
