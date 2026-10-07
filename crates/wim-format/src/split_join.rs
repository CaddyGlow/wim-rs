// SPDX-License-Identifier: LGPL-2.1-or-later OR GPL-3.0-or-later
//! Native in-memory split and join operations.
//! Resource partitioning and spanned-set validation follow wimlib split.c/join.c.
//! File naming, callbacks, transactions and the public C exports are separate.
use crate::allocation::*;
use crate::{
    HEADER_SIZE, Header, ParseError, ResourceHeader, WIM_MAGIC,
    archive::Archive,
    integrity::{DEFAULT_CHUNK_SIZE, IntegrityTable},
    lookup::{LookupBlob, LookupEntry},
    repack::{WriteOptions, append, append_blob},
};
use alloc::vec::Vec;
use hashbrown::HashSet;

/// Split an unmodified archive, copying ordinary encoded resource payloads.
/// Pipable inputs are decoded and re-encoded as ordinary resources.
/// `target_size` budgets stored metadata and content bytes, excluding headers,
/// lookup tables and XML. A single resource is never split. All metadata belongs
/// to part one. `guid` must be a fresh set identifier supplied by the caller.
/// Solid resources are rejected, as in upstream. Optional integrity is per part.
pub fn split_archive(
    input: &[u8],
    target_size: u64,
    guid: [u8; 16],
    integrity: bool,
) -> Result<Vec<Vec<u8>>, ParseError> {
    if target_size == 0 {
        return Err(ParseError::InvalidParam);
    }
    let archive = Archive::open(input)?;
    if archive.header.part_number != 1 {
        return Err(ParseError::MetadataNotFound);
    }
    if archive.lookup.resources.iter().any(|r| r.solid) {
        return Err(ParseError::Unsupported);
    }
    if archive.lookup.effective_image_count != archive.header.image_count {
        return Err(ParseError::ImageCount);
    }
    let xml = archive.xml_bytes()?;
    archive.xml()?;
    let mut groups: Vec<Vec<&LookupBlob>> = Vec::new();
    groups.try_reserve(1).map_err(|_| ParseError::Nomem)?;
    groups.push(Vec::new());
    let mut size = archive.lookup.metadata.iter().try_fold(0u64, |n, b| {
        n.checked_add(
            archive.lookup.resources[b.resource_index]
                .header
                .size_in_wim,
        )
        .ok_or(ParseError::InvalidParam)
    })?;
    let mut blobs: Vec<&LookupBlob> = Vec::new();
    blobs
        .try_reserve(archive.lookup.blobs.len())
        .map_err(|_| ParseError::Nomem)?;
    blobs.extend(archive.lookup.blobs.iter());
    blobs.sort_by_key(|b| {
        archive.lookup.resources[b.resource_index]
            .header
            .offset_in_wim
    });
    for blob in blobs {
        let stored = archive.lookup.resources[blob.resource_index]
            .header
            .size_in_wim;
        if size != 0 && size.checked_add(stored).ok_or(ParseError::InvalidParam)? >= target_size {
            if groups.len() == u16::MAX as usize {
                return Err(ParseError::InvalidParam);
            }
            groups.try_reserve(1).map_err(|_| ParseError::Nomem)?;
            groups.push(Vec::new());
            size = 0;
        }
        size = size.checked_add(stored).ok_or(ParseError::InvalidParam)?;
        let group = groups.last_mut().ok_or(ParseError::InvalidParam)?;
        group.try_reserve(1).map_err(|_| ParseError::Nomem)?;
        group.push(blob);
    }
    let total = groups.len() as u16;
    let mut parts = Vec::new();
    parts
        .try_reserve(total as usize)
        .map_err(|_| ParseError::Nomem)?;
    for (i, group) in groups.iter().enumerate() {
        let number = i as u16 + 1;
        let mut header = archive.header.clone();
        header.guid = guid;
        header.part_number = number;
        header.total_parts = total;
        header.flags &= 0x84 | 2 | 0xe0000;
        if total != 1 {
            header.flags |= 8;
            header.boot_index = 0;
        }
        header.magic = WIM_MAGIC;
        if header.boot_index > header.image_count {
            header.boot_index = 0;
        }
        header.reserved = [0; 60];
        header.boot_metadata = ResourceHeader::default();
        header.integrity_table = ResourceHeader::default();
        let mut output = initial_buffer()?;
        let mut table = Vec::new();
        for blob in group {
            copy_resource(input, &archive, blob, &mut output, &mut table, number)?;
        }
        if number == 1 {
            for (index, blob) in archive.lookup.metadata.iter().enumerate() {
                let resource =
                    copy_resource(input, &archive, blob, &mut output, &mut table, number)?;
                if header.boot_index == index as u32 + 1 {
                    header.boot_metadata = resource;
                }
            }
        }
        finish(&mut output, &mut header, &table, &xml, integrity)?;
        parts.push(output);
    }
    Ok(parts)
}

/// Validate and join a complete set, accepting arbitrary part order.
/// GUID, total count and consecutive unique part numbers must agree. Metadata,
/// XML and boot selection come from part one; all content is digest-verified and
/// serialized with the requested native compression. The joined GUID is retained.
pub fn join_archives(parts: &[&[u8]], options: WriteOptions) -> Result<Vec<u8>, ParseError> {
    if parts.is_empty() || parts.len() > u16::MAX as usize {
        return Err(ParseError::InvalidParam);
    }
    options
        .compression
        .validate_chunk_size(options.chunk_size)?;
    let mut archives = Vec::new();
    archives
        .try_reserve(parts.len())
        .map_err(|_| ParseError::Nomem)?;
    for part in parts {
        archives.push(Archive::open(part)?);
    }
    archives.sort_by_key(|a| a.header.part_number);
    let first = &archives[0];
    for (index, archive) in archives.iter().enumerate() {
        if archive.header.guid != first.header.guid
            || archive.header.total_parts as usize != parts.len()
            || archive.header.part_number as usize != index + 1
        {
            return Err(ParseError::SplitInvalid);
        }
    }
    first.xml()?;
    if first.lookup.effective_image_count != first.header.image_count {
        return Err(ParseError::ImageCount);
    }
    let mut header = first.header.clone();
    header.part_number = 1;
    header.total_parts = 1;
    header.magic = WIM_MAGIC;
    if header.boot_index > header.image_count {
        header.boot_index = 0;
    }
    header.flags &= 0x84;
    header.chunk_size = options.chunk_size;
    header.reserved = [0; 60];
    header.version = if options.compression == crate::Compression::Lzms {
        0xe00
    } else {
        0x10d00
    };
    header.flags |= match options.compression {
        crate::Compression::None => 0,
        crate::Compression::Xpress => 2 | 0x20000,
        crate::Compression::Lzx => 2 | 0x40000,
        crate::Compression::Lzms => 2 | 0x80000,
    };
    header.boot_metadata = ResourceHeader::default();
    header.integrity_table = ResourceHeader::default();
    let mut output = initial_buffer()?;
    let mut table = Vec::new();
    let mut seen = HashSet::new();
    for archive in &archives {
        for blob in &archive.lookup.blobs {
            if seen.contains(&blob.hash) {
                continue;
            }
            seen.try_reserve(1).map_err(|_| ParseError::Nomem)?;
            seen.try_insert_checked(blob.hash)
                .map_err(|_| ParseError::Nomem)?;
            let bytes = archive.read_blob(&blob.hash)?;
            let resource = append_blob(&mut output, &bytes, blob.flags & !(4 | 16 | 2), options)?;
            push_entry(&mut table, resource, blob, 1)?;
        }
    }
    for (index, blob) in first.lookup.metadata.iter().enumerate() {
        let bytes = first.read_metadata(index as u32 + 1)?;
        let metadata = crate::metadata::Metadata::parse(&bytes)?;
        for node in 0..metadata.nodes.len() {
            let inode = metadata
                .inode_entry(node)
                .ok_or(ParseError::InvalidMetadataResource)?;
            for stream in &inode.streams {
                if stream.hash != [0; 20] && !seen.contains(&stream.hash) {
                    return Err(ParseError::ResourceNotFound);
                }
            }
        }
        let resource = append_blob(&mut output, &bytes, 2, options)?;
        if header.boot_index == index as u32 + 1 {
            header.boot_metadata = resource;
        }
        push_entry(&mut table, resource, blob, 1)?;
    }
    finish(
        &mut output,
        &mut header,
        &table,
        &first.xml_bytes()?,
        options.integrity,
    )?;
    Ok(output)
}
fn initial_buffer() -> Result<Vec<u8>, ParseError> {
    let mut b = Vec::new();
    b.try_reserve_exact(HEADER_SIZE)
        .map_err(|_| ParseError::Nomem)?;
    b.resize(HEADER_SIZE, 0);
    Ok(b)
}
fn push_entry(
    table: &mut Vec<u8>,
    resource: ResourceHeader,
    blob: &LookupBlob,
    part_number: u16,
) -> Result<(), ParseError> {
    table.try_reserve(50).map_err(|_| ParseError::Nomem)?;
    table.extend_from_slice(
        &LookupEntry {
            resource,
            part_number,
            reference_count: blob.reference_count,
            hash: blob.hash,
        }
        .encode(),
    );
    Ok(())
}
fn copy_resource(
    input: &[u8],
    archive: &Archive<'_>,
    blob: &LookupBlob,
    output: &mut Vec<u8>,
    table: &mut Vec<u8>,
    part: u16,
) -> Result<ResourceHeader, ParseError> {
    if archive.header.magic != WIM_MAGIC {
        let bytes = if blob.flags & 2 != 0 {
            let index = archive
                .lookup
                .metadata
                .iter()
                .position(|b| b.hash == blob.hash)
                .ok_or(ParseError::MetadataNotFound)?;
            archive.read_metadata(index as u32 + 1)?
        } else {
            archive.read_blob(&blob.hash)?
        };
        let resource = append_blob(
            output,
            &bytes,
            blob.flags & !(4 | 16),
            WriteOptions {
                compression: archive.header.validate_compression()?,
                chunk_size: archive.header.chunk_size,
                integrity: false,
            },
        )?;
        push_entry(table, resource, blob, part)?;
        return Ok(resource);
    }
    let mut resource = archive.lookup.resources[blob.resource_index].header;
    let start =
        usize::try_from(resource.offset_in_wim).map_err(|_| ParseError::InvalidResourceHash)?;
    let end = start
        .checked_add(
            usize::try_from(resource.size_in_wim).map_err(|_| ParseError::InvalidResourceHash)?,
        )
        .ok_or(ParseError::InvalidResourceHash)?;
    let bytes = input
        .get(start..end)
        .ok_or(ParseError::UnexpectedEndOfFile)?;
    resource.offset_in_wim = output.len() as u64;
    output
        .try_reserve(bytes.len())
        .map_err(|_| ParseError::Nomem)?;
    output.extend_from_slice(bytes);
    push_entry(table, resource, blob, part)?;
    Ok(resource)
}
fn finish(
    output: &mut Vec<u8>,
    header: &mut Header,
    table: &[u8],
    xml: &[u8],
    integrity: bool,
) -> Result<(), ParseError> {
    header.blob_table = append(output, table, 2)?;
    let check_end = output.len() as u64;
    header.xml_data = append(output, xml, 2)?;
    if integrity {
        let bytes = IntegrityTable::calculate(output, check_end, DEFAULT_CHUNK_SIZE)?.encode()?;
        header.integrity_table = append(output, &bytes, 0)?;
    }
    output[..HEADER_SIZE].copy_from_slice(&header.encode_canonical());
    Ok(())
}
