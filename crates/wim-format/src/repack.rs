// SPDX-License-Identifier: LGPL-2.1-or-later OR GPL-3.0-or-later
//! Archive serialization from an existing, resolved image set.
//! This low-level writer retains the source GUID and image metadata/properties.
//! It does not implement overwrite transactions, file I/O, or write callbacks.

use crate::{
    Compression, HEADER_SIZE, ParseError, ResourceHeader, WIM_MAGIC,
    archive::Archive,
    integrity::{DEFAULT_CHUNK_SIZE, IntegrityTable},
    lookup::LookupEntry,
    metadata::Metadata,
    resource::ResourceLayout,
    resource_write::encode_resource,
};
use alloc::vec::Vec;

/// Settings for ordinary archive serialization.
#[derive(Debug, Clone, Copy)]
pub struct WriteOptions {
    /// Native compression mode for ordinary resources.
    pub compression: Compression,
    /// Uncompressed chunk size; use zero for uncompressed output.
    pub chunk_size: u32,
    /// Include SHA-1 integrity coverage of resources and the lookup table.
    pub integrity: bool,
}

/// Rewrite an unsplit archive with raw resources and optional SHA-1 integrity.
/// Source payload SHA-1 digests and metadata structure are checked before use.
/// Split archives and metadata referencing missing content cannot be rewritten.
pub fn write_uncompressed(archive: &Archive<'_>, integrity: bool) -> Result<Vec<u8>, ParseError> {
    write_archive(
        archive,
        WriteOptions {
            compression: Compression::None,
            chunk_size: 0,
            integrity,
        },
    )
}

/// Serialize a complete ordinary WIM using native raw, XPRESS, LZX or LZMS resources.
/// Retains all images, source GUID, XML properties, metadata and boot image selection.
/// This buffers the result and does not provide the public wimlib write API.
pub fn write_archive(archive: &Archive<'_>, options: WriteOptions) -> Result<Vec<u8>, ParseError> {
    options
        .compression
        .validate_chunk_size(options.chunk_size)?;
    if archive.header.total_parts != 1 {
        return Err(ParseError::IsSplitWim);
    }
    let mut xml = archive.xml()?;
    if archive.lookup.effective_image_count != archive.header.image_count {
        return Err(ParseError::ImageCount);
    }
    let mut header = archive.header.clone();
    header.magic = WIM_MAGIC;
    header.version = if options.compression == Compression::Lzms {
        0xe00
    } else {
        0x10d00
    };
    header.flags &= 0x84; // Upstream write.c retains readonly and reparse-fix flags.
    header.chunk_size = options.chunk_size;
    match options.compression {
        Compression::Xpress => header.flags |= 2 | 0x20000,
        Compression::Lzx => header.flags |= 2 | 0x40000,
        Compression::Lzms => header.flags |= 2 | 0x80000,
        _ => {}
    }
    header.reserved = [0; 60];
    header.boot_metadata = ResourceHeader::default();
    header.integrity_table = ResourceHeader::default();
    if header.boot_index > header.image_count {
        header.boot_index = 0;
    }
    let mut output = Vec::new();
    output
        .try_reserve_exact(HEADER_SIZE)
        .map_err(|_| ParseError::Nomem)?;
    output.resize(HEADER_SIZE, 0);
    let mut table = Vec::new();
    for blob in &archive.lookup.blobs {
        let bytes = archive.read_blob(&blob.hash)?;
        let resource = append_blob(&mut output, &bytes, blob.flags & !(4 | 16 | 2), options)?;
        append_entry(&mut table, resource, blob.reference_count, blob.hash)?;
    }
    for (index, blob) in archive.lookup.metadata.iter().enumerate() {
        let bytes = archive.read_metadata(index as u32 + 1)?;
        let metadata = Metadata::parse(&bytes)?;
        for index in 0..metadata.nodes.len() {
            let entry = metadata
                .inode_entry(index)
                .ok_or(ParseError::InvalidMetadataResource)?;
            for stream in &entry.streams {
                if stream.hash != [0; 20] && archive.lookup.find(&stream.hash).is_none() {
                    return Err(ParseError::ResourceNotFound);
                }
            }
        }
        let resource = append_blob(&mut output, &bytes, 2, options)?;
        if header.boot_index == index as u32 + 1 {
            header.boot_metadata = resource;
        }
        append_entry(&mut table, resource, 1, blob.hash)?;
    }
    header.blob_table = append(&mut output, &table, 2)?;
    let check_end = output.len() as u64;
    xml.set_total_bytes(Some(check_end))?;
    header.xml_data = append(&mut output, &xml.encode_utf16le()?, 2)?;
    if options.integrity {
        let bytes = IntegrityTable::calculate(&output, check_end, DEFAULT_CHUNK_SIZE)?.encode()?;
        header.integrity_table = append(&mut output, &bytes, 0)?;
    }
    output[..HEADER_SIZE].copy_from_slice(&header.encode_canonical());
    Ok(output)
}

pub(crate) fn append_blob(
    output: &mut Vec<u8>,
    bytes: &[u8],
    flags: u8,
    options: WriteOptions,
) -> Result<ResourceHeader, ParseError> {
    let encoded = encode_resource(
        bytes,
        options.compression,
        options.chunk_size,
        ResourceLayout::Ordinary,
        |kind, chunk| match kind {
            Compression::Xpress => ms_compress::xpress_encode::compress_xpress(chunk, chunk.len())
                .map_err(|e| match e {
                    ms_compress::xpress_encode::EncodeError::OutOfMemory => ParseError::Nomem,
                    _ => ParseError::InvalidChunkSize,
                }),
            Compression::Lzx => ms_compress::lzx_encode::compress_lzx(
                chunk,
                chunk.len(),
                options.chunk_size as usize,
            )
            .map_err(|e| match e {
                ms_compress::lzx_encode::EncodeError::OutOfMemory => ParseError::Nomem,
                _ => ParseError::InvalidChunkSize,
            }),
            Compression::Lzms => ms_compress::lzms::encode::compress_lzms(chunk, chunk.len())
                .map_err(|e| match e {
                    ms_compress::lzms::encode::EncodeError::OutOfMemory => ParseError::Nomem,
                    _ => ParseError::InvalidParam,
                }),
            _ => Err(ParseError::Unsupported),
        },
    )?;
    let mut resource = encoded.header;
    resource.flags |= flags;
    resource.offset_in_wim = output.len() as u64;
    output
        .try_reserve(encoded.bytes.len())
        .map_err(|_| ParseError::Nomem)?;
    output.extend_from_slice(&encoded.bytes);
    Ok(resource)
}

pub(crate) fn append(
    output: &mut Vec<u8>,
    bytes: &[u8],
    flags: u8,
) -> Result<ResourceHeader, ParseError> {
    let size = bytes.len() as u64;
    if size >= 1 << 56 {
        return Err(ParseError::InvalidParam);
    }
    let header = ResourceHeader {
        size_in_wim: size,
        flags,
        offset_in_wim: output.len() as u64,
        uncompressed_size: size,
    };
    output
        .try_reserve(bytes.len())
        .map_err(|_| ParseError::Nomem)?;
    output.extend_from_slice(bytes);
    Ok(header)
}
pub(crate) fn append_entry(
    table: &mut Vec<u8>,
    resource: ResourceHeader,
    reference_count: u32,
    hash: [u8; 20],
) -> Result<(), ParseError> {
    let entry = LookupEntry {
        resource,
        reference_count,
        hash,
        part_number: 1,
    };
    table
        .try_reserve(crate::lookup::ENTRY_SIZE)
        .map_err(|_| ParseError::Nomem)?;
    table.extend_from_slice(&entry.encode());
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn refuses_split_or_unresolved_image_content() {
        let input = include_bytes!("../tests/fixtures/xpress-resource.wim");
        let mut source = Archive::open(input).unwrap();
        source.header.total_parts = 2;
        assert_eq!(
            write_uncompressed(&source, false),
            Err(ParseError::IsSplitWim)
        );
        source.header.total_parts = 1;
        source.lookup.blobs.clear();
        assert_eq!(
            write_uncompressed(&source, false),
            Err(ParseError::ResourceNotFound)
        );
    }

    #[test]
    fn preserves_boot_resource_and_normalizes_invalid_boot_index() {
        let source_bytes = include_bytes!("../tests/fixtures/xpress-resource.wim");
        for index in [0, 1, 2] {
            let mut source = Archive::open(source_bytes).unwrap();
            source.header.boot_index = index;
            let bytes = write_uncompressed(&source, false).unwrap();
            let output = Archive::open(&bytes).unwrap();
            assert_eq!(output.header.boot_index, if index == 1 { 1 } else { 0 });
            if index == 1 {
                assert_eq!(
                    output.header.boot_metadata,
                    output.lookup.resources[output.lookup.metadata[0].resource_index].header
                );
            } else {
                assert_eq!(output.header.boot_metadata, ResourceHeader::default());
            }
        }
    }
    #[test]
    fn original_images_roundtrip_with_metadata_xml_hashes_and_integrity() {
        for input in [
            include_bytes!("../tests/fixtures/xpress-resource.wim").as_slice(),
            include_bytes!("../tests/fixtures/pipable-resource.wim").as_slice(),
            include_bytes!("../tests/fixtures/solid-resource.wim").as_slice(),
        ] {
            let source = Archive::open(input).unwrap();
            for (compression, chunk_size) in [
                (Compression::None, 0),
                (Compression::Xpress, 32768),
                (Compression::Lzx, 32768),
                (Compression::Lzms, 32768),
            ] {
                for integrity in [false, true] {
                    let bytes = write_archive(
                        &source,
                        WriteOptions {
                            compression,
                            chunk_size,
                            integrity,
                        },
                    )
                    .unwrap();
                    let output = Archive::open(&bytes).unwrap();
                    assert_eq!(output.header.validate_compression().unwrap(), compression);
                    let mut original_xml = source.xml().unwrap();
                    let mut written_xml = output.xml().unwrap();
                    original_xml.set_total_bytes(None).unwrap();
                    written_xml.set_total_bytes(None).unwrap();
                    assert_eq!(
                        written_xml.to_xml().unwrap(),
                        original_xml.to_xml().unwrap()
                    );
                    assert_eq!(
                        output.read_metadata(1).unwrap(),
                        source.read_metadata(1).unwrap()
                    );
                    for blob in &source.lookup.blobs {
                        assert_eq!(
                            output.read_blob(&blob.hash).unwrap(),
                            source.read_blob(&blob.hash).unwrap()
                        );
                    }
                    assert_eq!(
                        output.check_integrity().unwrap(),
                        if integrity {
                            crate::integrity::IntegrityStatus::Ok
                        } else {
                            crate::integrity::IntegrityStatus::Nonexistent
                        }
                    );
                }
            }
        }
    }
}
