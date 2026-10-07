// SPDX-License-Identifier: LGPL-2.1-or-later OR GPL-3.0-or-later
// Framing derived from wimlib src/write.c, cd5e231c348c255ae5088873b5a66ee0eb96fa07.
//! Buffered serialization of complete wimlib pipable archives.

use crate::{
    Compression, HEADER_SIZE, PIPABLE_MAGIC, ParseError, ResourceHeader,
    archive::Archive,
    integrity::{DEFAULT_CHUNK_SIZE, IntegrityTable},
    lookup::LookupEntry,
    metadata::Metadata,
    repack::WriteOptions,
    resource::ResourceLayout,
    resource_write::encode_resource,
};
use alloc::vec::Vec;
use sha1::Digest;

/// Serialize an unsplit archive using wimlib's sequentially consumable format.
/// Metadata precedes payloads; both preliminary XML and final seekable tables
/// are included. This buffers all output, and provides no file I/O or callbacks.
/// Integrity covers the seekable archive; it is ignored by pipe consumers.
pub fn write_pipable_archive(
    archive: &Archive<'_>,
    options: WriteOptions,
) -> Result<Vec<u8>, ParseError> {
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
    header.magic = PIPABLE_MAGIC;
    header.version = if options.compression == Compression::Lzms {
        0xe00
    } else {
        0x10d00
    };
    header.flags &= 0x84;
    header.chunk_size = options.chunk_size;
    match options.compression {
        Compression::Xpress => header.flags |= 2 | 0x20000,
        Compression::Lzx => header.flags |= 2 | 0x40000,
        Compression::Lzms => header.flags |= 2 | 0x80000,
        _ => {}
    }
    header.reserved = [0; 60];
    header.blob_table = ResourceHeader::default();
    header.xml_data = ResourceHeader::default();
    header.boot_metadata = ResourceHeader::default();
    header.integrity_table = ResourceHeader::default();
    if header.boot_index > header.image_count {
        header.boot_index = 0;
    }
    let mut output = Vec::new();
    output
        .try_reserve_exact(HEADER_SIZE)
        .map_err(|_| ParseError::Nomem)?;
    output.extend_from_slice(&header.encode_canonical());
    xml.set_total_bytes(None)?;
    append(&mut output, &xml.encode_utf16le()?, 2)?;
    let mut table = Vec::new();
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
        let resource = append_blob(&mut output, &bytes, 2, blob.hash, options)?;
        if header.boot_index == index as u32 + 1 {
            header.boot_metadata = resource;
        }
        append_entry(&mut table, resource, 1, blob.hash)?;
    }
    for blob in &archive.lookup.blobs {
        let bytes = archive.read_blob(&blob.hash)?;
        let resource = append_blob(
            &mut output,
            &bytes,
            blob.flags & !(4 | 16 | 2),
            blob.hash,
            options,
        )?;
        append_entry(&mut table, resource, blob.reference_count, blob.hash)?;
    }
    header.blob_table = append(&mut output, &table, 2)?;
    let check_end = output.len() as u64;
    xml.set_total_bytes(Some(check_end))?;
    header.xml_data = append(&mut output, &xml.encode_utf16le()?, 2)?;
    if options.integrity {
        let bytes = IntegrityTable::calculate(&output, check_end, DEFAULT_CHUNK_SIZE)?.encode()?;
        // Upstream integrity tables are raw, without a pipable blob header.
        let size = bytes.len() as u64;
        header.integrity_table = ResourceHeader {
            size_in_wim: size,
            flags: 0,
            offset_in_wim: output.len() as u64,
            uncompressed_size: size,
        };
        output
            .try_reserve(bytes.len())
            .map_err(|_| ParseError::Nomem)?;
        output.extend_from_slice(&bytes);
    }
    output
        .try_reserve(HEADER_SIZE)
        .map_err(|_| ParseError::Nomem)?;
    output.extend_from_slice(&header.encode_canonical());
    Ok(output)
}
fn append_pwm_header(
    output: &mut Vec<u8>,
    size: u64,
    hash: [u8; 20],
    flags: u8,
) -> Result<(), ParseError> {
    output.try_reserve(40).map_err(|_| ParseError::Nomem)?;
    output.extend_from_slice(&0x2b9b9ba2443db9d8u64.to_le_bytes());
    output.extend_from_slice(&size.to_le_bytes());
    output.extend_from_slice(&hash);
    output.extend_from_slice(&u32::from(flags).to_le_bytes());
    Ok(())
}
fn append_blob(
    output: &mut Vec<u8>,
    bytes: &[u8],
    flags: u8,
    hash: [u8; 20],
    options: WriteOptions,
) -> Result<ResourceHeader, ParseError> {
    let encoded = encode_resource(
        bytes,
        options.compression,
        options.chunk_size,
        ResourceLayout::Pipable,
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
    append_pwm_header(output, bytes.len() as u64, hash, resource.flags)?;
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
    append_pwm_header(output, size, sha1::Sha1::digest(bytes).into(), flags)?;
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
