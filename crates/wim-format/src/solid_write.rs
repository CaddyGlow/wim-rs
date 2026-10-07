// SPDX-License-Identifier: LGPL-2.1-or-later OR GPL-3.0-or-later
// Solid lookup framing follows wimlib blob_table.c/write.c at
// cd5e231c348c255ae5088873b5a66ee0eb96fa07 (Eric Biggers and contributors).
//! Buffered solid WIM serialization.

use crate::{
    Compression, HEADER_SIZE, ParseError, ResourceHeader,
    archive::Archive,
    integrity::{DEFAULT_CHUNK_SIZE, IntegrityTable},
    lookup::SOLID_RESOURCE_MARKER,
    repack::{WriteOptions, append, append_blob, append_entry, write_archive},
    resource::ResourceLayout,
    resource_write::encode_resource,
};
use alloc::vec::Vec;
/// Serialize an unsplit archive with all data blobs in one solid resource.
///
/// Retains source GUID, XML, metadata, references and boot selection. Metadata
/// remains ordinary. Source hashes and metadata stream references are checked.
/// This buffers an ordinary staging archive, the concatenated payload, compressed
/// resource and final output; it is not a bounded-memory streaming writer.
pub fn write_solid_archive(
    archive: &Archive<'_>,
    options: WriteOptions,
) -> Result<Vec<u8>, ParseError> {
    write_solid_archive_with_settings(archive, options, options.compression, options.chunk_size)
}

/// Write solid data with independent ordinary metadata and solid data settings.
/// The file header advertises ordinary settings; solid headers advertise their
/// own codec and chunk size. Output remains fully buffered.
pub fn write_solid_archive_with_settings(
    archive: &Archive<'_>,
    options: WriteOptions,
    solid_compression: Compression,
    solid_chunk_size: u32,
) -> Result<Vec<u8>, ParseError> {
    if solid_compression == Compression::None {
        return Err(ParseError::InvalidCompressionType);
    }
    options
        .compression
        .validate_chunk_size(options.chunk_size)?;
    solid_compression.validate_chunk_size(solid_chunk_size)?;
    // Reuse the ordinary writer's validation and metadata compression. Its data
    // resources are temporary; only metadata resources are relocated below.
    let staged = write_archive(
        archive,
        WriteOptions {
            integrity: false,
            ..options
        },
    )?;
    let ordinary = Archive::open(&staged)?;
    let mut header = ordinary.header.clone();
    header.version = 0xe00;
    let mut output = Vec::new();
    output
        .try_reserve_exact(HEADER_SIZE)
        .map_err(|_| ParseError::Nomem)?;
    output.resize(HEADER_SIZE, 0);
    let mut table = Vec::new();
    let mut payload = Vec::new();
    for blob in &archive.lookup.blobs {
        let bytes = archive.read_blob(&blob.hash)?;
        payload
            .try_reserve(bytes.len())
            .map_err(|_| ParseError::Nomem)?;
        payload.extend_from_slice(&bytes);
    }
    if !payload.is_empty() {
        let encoded = encode_resource(
            &payload,
            solid_compression,
            solid_chunk_size,
            ResourceLayout::Solid,
            |kind, chunk| match kind {
                Compression::Xpress => {
                    ms_compress::xpress_encode::compress_xpress(chunk, chunk.len()).map_err(|e| {
                        match e {
                            ms_compress::xpress_encode::EncodeError::OutOfMemory => {
                                ParseError::Nomem
                            }
                            _ => ParseError::InvalidChunkSize,
                        }
                    })
                }
                Compression::Lzx => ms_compress::lzx_encode::compress_lzx(
                    chunk,
                    chunk.len(),
                    solid_chunk_size as usize,
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
                Compression::None => Err(ParseError::InvalidCompressionType),
            },
        )?;
        let resource = append(&mut output, &encoded.bytes, 16)?;
        append_entry(
            &mut table,
            ResourceHeader {
                uncompressed_size: SOLID_RESOURCE_MARKER,
                ..resource
            },
            1,
            [0; 20],
        )?;
        let mut offset = 0u64;
        for blob in &archive.lookup.blobs {
            append_entry(
                &mut table,
                ResourceHeader {
                    size_in_wim: blob.size,
                    flags: 16,
                    offset_in_wim: offset,
                    uncompressed_size: 0,
                },
                blob.reference_count,
                blob.hash,
            )?;
            offset = offset
                .checked_add(blob.size)
                .ok_or(ParseError::InvalidParam)?;
        }
    }
    header.boot_metadata = ResourceHeader::default();
    for (i, blob) in ordinary.lookup.metadata.iter().enumerate() {
        let bytes = ordinary.read_metadata(i as u32 + 1)?;
        let resource = append_blob(&mut output, &bytes, 2, options)?;
        if header.boot_index == i as u32 + 1 {
            header.boot_metadata = resource;
        }
        append_entry(&mut table, resource, 1, blob.hash)?;
    }
    header.blob_table = append(&mut output, &table, 2)?;
    let check_end = output.len() as u64;
    let mut xml = ordinary.xml()?;
    xml.set_total_bytes(Some(check_end))?;
    header.xml_data = append(&mut output, &xml.encode_utf16le()?, 2)?;
    if options.integrity {
        let bytes = IntegrityTable::calculate(&output, check_end, DEFAULT_CHUNK_SIZE)?.encode()?;
        header.integrity_table = append(&mut output, &bytes, 0)?;
    }
    output[..HEADER_SIZE].copy_from_slice(&header.encode_canonical());
    Ok(output)
}
