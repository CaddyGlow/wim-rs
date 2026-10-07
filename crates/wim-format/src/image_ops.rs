// SPDX-License-Identifier: LGPL-2.1-or-later OR GPL-3.0-or-later
//! Native image selection and deletion with content pruning.

use crate::{
    HEADER_SIZE, ParseError, ResourceHeader, WIM_MAGIC,
    archive::Archive,
    integrity::{DEFAULT_CHUNK_SIZE, IntegrityTable},
    metadata::Metadata,
    repack::{WriteOptions, append, append_blob, append_entry},
};
use alloc::collections::BTreeMap;
use alloc::vec::Vec;
use sha1::{Digest, Sha1};

/// Serialize selected one-based image indices in the requested order.
/// Each image may occur once. Unknown XML properties and raw metadata survive;
/// unreferenced payloads are omitted and reference counts are recalculated.
/// This buffered operation supports complete unsplit archives only.
pub fn select_images(
    archive: &Archive<'_>,
    images: &[u32],
    options: WriteOptions,
) -> Result<Vec<u8>, ParseError> {
    if archive.header.total_parts != 1 {
        return Err(ParseError::IsSplitWim);
    }
    let mut seen = alloc::collections::BTreeSet::new();
    for &image in images {
        if image == 0 || image > archive.header.image_count {
            return Err(ParseError::InvalidImage);
        }
        if !seen.insert(image) {
            return Err(ParseError::InvalidParam);
        }
    }
    options
        .compression
        .validate_chunk_size(options.chunk_size)?;
    let xml = archive.xml()?.select_images(images)?;
    let sources: Vec<_> = images.iter().map(|&image| (archive, image)).collect();
    let boot_index = images
        .iter()
        .position(|&i| i == archive.header.boot_index)
        .map_or(0, |i| i as u32 + 1);
    write_images(archive, &sources, xml, boot_index, options)
}

/// Export selected images into an existing destination, deduplicating content.
/// Retains destination identity and boot selection. Source names must not collide
/// with destination names. Complete unsplit archives are required; external
/// resource reference contexts and export flag/name override policies are pending.
pub fn export_images(
    destination: &Archive<'_>,
    source: &Archive<'_>,
    images: &[u32],
    options: WriteOptions,
) -> Result<Vec<u8>, ParseError> {
    let mut xml = destination.xml()?;
    let source_xml = source.xml()?;
    for &index in images {
        if index == 0 || index > source.header.image_count {
            return Err(ParseError::InvalidImage);
        }
        if source_xml
            .name(index as i32)
            .is_some_and(|name| xml.name_in_use(name))
        {
            return Err(ParseError::ImageNameCollision);
        }
        xml.append_images(&source_xml, &[index])?;
    }
    let mut sources: Vec<_> = (1..=destination.header.image_count)
        .map(|i| (destination, i))
        .collect();
    sources
        .try_reserve(images.len())
        .map_err(|_| ParseError::Nomem)?;
    sources.extend(images.iter().map(|&i| (source, i)));
    write_images(
        destination,
        &sources,
        xml,
        destination.header.boot_index,
        options,
    )
}

fn write_images<'a>(
    archive: &Archive<'a>,
    sources: &[(&Archive<'a>, u32)],
    mut xml: crate::xml::XmlInfo,
    boot_index: u32,
    options: WriteOptions,
) -> Result<Vec<u8>, ParseError> {
    options
        .compression
        .validate_chunk_size(options.chunk_size)?;
    let mut metadata_bytes = Vec::new();
    let mut references: BTreeMap<[u8; 20], (u32, &Archive<'_>)> = BTreeMap::new();
    for &(source, image) in sources {
        if source.header.total_parts != 1 {
            return Err(ParseError::IsSplitWim);
        }
        let bytes = source.read_metadata(image)?;
        let metadata = Metadata::parse(&bytes)?;
        // Counting canonical streams per dentry equals upstream i_nlink.
        for node in 0..metadata.nodes.len() {
            let entry = metadata
                .inode_entry(node)
                .ok_or(ParseError::InvalidMetadataResource)?;
            for stream in &entry.streams {
                if stream.hash != [0; 20] {
                    let (count, _) = references.entry(stream.hash).or_insert((0, source));
                    *count = count.checked_add(1).ok_or(ParseError::InvalidParam)?;
                }
            }
        }
        metadata_bytes
            .try_reserve(1)
            .map_err(|_| ParseError::Nomem)?;
        drop(metadata);
        metadata_bytes.push(bytes);
    }
    let mut header = archive.header.clone();
    header.image_count = sources.len() as u32;
    header.boot_index = if boot_index <= header.image_count {
        boot_index
    } else {
        0
    };
    header.magic = WIM_MAGIC;
    header.version = if options.compression == crate::Compression::Lzms {
        0xe00
    } else {
        0x10d00
    };
    header.flags &= 0x84;
    header.flags |= match options.compression {
        crate::Compression::None => 0,
        crate::Compression::Xpress => 0x20002,
        crate::Compression::Lzx => 0x40002,
        crate::Compression::Lzms => 0x80002,
    };
    header.chunk_size = options.chunk_size;
    header.reserved = [0; 60];
    header.boot_metadata = ResourceHeader::default();
    header.integrity_table = ResourceHeader::default();
    let mut output = Vec::new();
    output
        .try_reserve_exact(HEADER_SIZE)
        .map_err(|_| ParseError::Nomem)?;
    output.resize(HEADER_SIZE, 0);
    let mut table = Vec::new();
    for (hash, (count, source)) in references {
        let bytes = source.read_blob(&hash)?;
        let resource = append_blob(&mut output, &bytes, 0, options)?;
        append_entry(&mut table, resource, count, hash)?;
    }
    for (index, bytes) in metadata_bytes.iter().enumerate() {
        let resource = append_blob(&mut output, bytes, 2, options)?;
        if header.boot_index == index as u32 + 1 {
            header.boot_metadata = resource;
        }
        append_entry(&mut table, resource, 1, Sha1::digest(bytes).into())?;
    }
    header.blob_table = append(&mut output, &table, 2)?;
    let check_end = output.len() as u64;
    xml.set_total_bytes(Some(check_end))?;
    header.xml_data = append(&mut output, &xml.encode_utf16le()?, 2)?;
    if options.integrity {
        let integrity =
            IntegrityTable::calculate(&output, check_end, DEFAULT_CHUNK_SIZE)?.encode()?;
        header.integrity_table = append(&mut output, &integrity, 0)?;
    }
    output[..HEADER_SIZE].copy_from_slice(&header.encode_canonical());
    Ok(output)
}

/// Delete one image, retain the others in source order and prune its content.
/// Deleting the last image produces a valid zero-image archive.
pub fn delete_image(
    archive: &Archive<'_>,
    image: u32,
    options: WriteOptions,
) -> Result<Vec<u8>, ParseError> {
    if image == 0 || image > archive.header.image_count {
        return Err(ParseError::InvalidImage);
    }
    let images: Vec<_> = (1..=archive.header.image_count)
        .filter(|&i| i != image)
        .collect();
    select_images(archive, &images, options)
}
