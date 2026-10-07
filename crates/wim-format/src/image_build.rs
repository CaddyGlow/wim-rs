// SPDX-License-Identifier: LGPL-2.1-or-later OR GPL-3.0-or-later
//! Construction of new WIMs from owned image trees and content bytes.
//! This buffers a raw intermediate archive before native resource compression.
//! No existing WIM template, original library, or filesystem capture is used.

use crate::{
    HEADER_SIZE, Header, ParseError, ResourceHeader, WIM_MAGIC,
    archive::Archive,
    lookup::LookupEntry,
    metadata::Metadata,
    metadata_write::OwnedMetadata,
    repack::{WriteOptions, write_archive},
    xml::XmlInfo,
};
use alloc::format;
use alloc::string::String;
use alloc::string::ToString;
use alloc::vec::Vec;
use sha1::{Digest, Sha1};

/// One new image. Metadata hashes must identify content supplied to the builder.
#[derive(Debug, Clone)]
pub struct NewImage {
    /// Visible image tree and opaque Windows fields.
    pub metadata: OwnedMetadata,
    /// Optional image name; nonempty names must be unique.
    pub name: Option<String>,
    /// Optional image description.
    pub description: Option<String>,
    /// Additional XML property paths and values, applied after calculated stats.
    pub properties: Vec<(String, String)>,
}

/// New unsplit image set with explicitly supplied archive identity.
#[derive(Debug)]
pub struct ImageBuilder {
    guid: [u8; 16],
    images: Vec<NewImage>,
    blobs: Vec<([u8; 20], Vec<u8>)>,
    boot_index: u32,
}
impl ImageBuilder {
    /// Create an empty set. The caller owns GUID generation and uniqueness.
    pub fn new(guid: [u8; 16]) -> Self {
        Self {
            guid,
            images: Vec::new(),
            blobs: Vec::new(),
            boot_index: 0,
        }
    }
    /// Intern bytes by their SHA-1. Empty streams use the format's zero hash.
    pub fn add_blob(&mut self, bytes: &[u8]) -> Result<[u8; 20], ParseError> {
        if bytes.is_empty() {
            return Ok([0; 20]);
        }
        let hash: [u8; 20] = Sha1::digest(bytes).into();
        if let Some((_, existing)) = self.blobs.iter().find(|(h, _)| *h == hash) {
            if existing != bytes {
                return Err(ParseError::InvalidResourceHash);
            }
            return Ok(hash);
        }
        let mut copy = Vec::new();
        append(&mut copy, bytes)?;
        self.blobs.try_reserve(1).map_err(|_| ParseError::Nomem)?;
        self.blobs.push((hash, copy));
        Ok(hash)
    }
    /// Validate and append an image; return its one-based image number.
    pub fn add_image(&mut self, image: NewImage) -> Result<u32, ParseError> {
        if self.images.len() == 65535 {
            return Err(ParseError::ImageCount);
        }
        image.metadata.encode()?;
        if image.name.as_deref().is_some_and(|name| {
            !name.is_empty() && self.images.iter().any(|i| i.name.as_deref() == Some(name))
        }) {
            return Err(ParseError::ImageNameCollision);
        }
        self.images.try_reserve(1).map_err(|_| ParseError::Nomem)?;
        self.images.push(image);
        Ok(self.images.len() as u32)
    }
    /// Select a one-based boot image; zero clears the selection.
    pub fn set_boot_index(&mut self, index: u32) -> Result<(), ParseError> {
        if index as usize > self.images.len() {
            return Err(ParseError::InvalidImage);
        }
        self.boot_index = index;
        Ok(())
    }
    /// Write ordinary WIM bytes using native encoders and optional integrity.
    /// Unreferenced supplied blobs are omitted. Alias references use the parsed
    /// canonical inode streams, matching upstream inode_fixup and i_nlink counts.
    pub fn write(&self, options: WriteOptions) -> Result<Vec<u8>, ParseError> {
        options
            .compression
            .validate_chunk_size(options.chunk_size)?;
        let mut xml_text = String::from("<WIM>");
        for i in 0..self.images.len() {
            xml_text.push_str(&format!("<IMAGE INDEX=\"{}\"/>", i + 1));
        }
        xml_text.push_str("</WIM>");
        let mut xml = XmlInfo::parse(&xml_text)?;
        let mut refs = Vec::new();
        refs.try_reserve_exact(self.blobs.len())
            .map_err(|_| ParseError::Nomem)?;
        refs.resize(self.blobs.len(), 0u32);
        let mut metadata_bytes = Vec::new();
        metadata_bytes
            .try_reserve_exact(self.images.len())
            .map_err(|_| ParseError::Nomem)?;
        for (image_index, image) in self.images.iter().enumerate() {
            let bytes = image.metadata.encode()?;
            let metadata = Metadata::parse(&bytes)?;
            let mut directories = 0u64;
            let mut files = 0u64;
            let mut total = 0u64;
            let mut hardlinks = 0u64;
            let mut visited = Vec::new();
            visited
                .try_reserve_exact(metadata.nodes.len())
                .map_err(|_| ParseError::Nomem)?;
            visited.resize(metadata.nodes.len(), false);
            for node in 0..metadata.nodes.len() {
                let entry = metadata
                    .inode_entry(node)
                    .ok_or(ParseError::InvalidMetadataResource)?;
                if entry.attributes & 0x10 != 0 {
                    directories += 1;
                } else {
                    files += 1;
                }
                let mut size = 0u64;
                for stream in &entry.streams {
                    if stream.hash == [0; 20] {
                        continue;
                    }
                    let blob_index = self
                        .blobs
                        .iter()
                        .position(|(hash, _)| *hash == stream.hash)
                        .ok_or(ParseError::ResourceNotFound)?;
                    refs[blob_index] = refs[blob_index]
                        .checked_add(1)
                        .ok_or(ParseError::InvalidParam)?;
                    size = size
                        .checked_add(self.blobs[blob_index].1.len() as u64)
                        .ok_or(ParseError::InvalidParam)?;
                }
                total = total.checked_add(size).ok_or(ParseError::InvalidParam)?;
                let canonical = metadata.nodes[node].inode;
                if visited[canonical] {
                    hardlinks = hardlinks
                        .checked_add(size)
                        .ok_or(ParseError::InvalidParam)?;
                }
                visited[canonical] = true;
            }
            let index = image_index as i32 + 1;
            xml.set_name(index, image.name.as_deref())?;
            xml.set_description(index, image.description.as_deref())?;
            for (name, value) in [
                ("DIRCOUNT", directories),
                ("FILECOUNT", files),
                ("TOTALBYTES", total),
                ("HARDLINKBYTES", hardlinks),
            ] {
                xml.set_property(index, name, Some(&value.to_string()))?;
            }
            for (path, value) in &image.properties {
                xml.set_property(index, path, Some(value))?;
            }
            drop(metadata);
            metadata_bytes.push(bytes);
        }
        let mut output = Vec::new();
        output
            .try_reserve_exact(HEADER_SIZE)
            .map_err(|_| ParseError::Nomem)?;
        output.resize(HEADER_SIZE, 0);
        let mut table = Vec::new();
        let mut header = Header {
            magic: WIM_MAGIC,
            version: 0x10d00,
            flags: 0,
            chunk_size: 0,
            guid: self.guid,
            part_number: 1,
            total_parts: 1,
            image_count: self.images.len() as u32,
            blob_table: ResourceHeader::default(),
            xml_data: ResourceHeader::default(),
            boot_metadata: ResourceHeader::default(),
            boot_index: self.boot_index,
            integrity_table: ResourceHeader::default(),
            reserved: [0; 60],
        };
        for ((hash, bytes), count) in self.blobs.iter().zip(refs) {
            if count == 0 {
                continue;
            }
            let resource = raw_resource(&mut output, bytes, 0)?;
            append(
                &mut table,
                &LookupEntry {
                    resource,
                    part_number: 1,
                    reference_count: count,
                    hash: *hash,
                }
                .encode(),
            )?;
        }
        for (index, bytes) in metadata_bytes.iter().enumerate() {
            let resource = raw_resource(&mut output, bytes, 2)?;
            if self.boot_index == index as u32 + 1 {
                header.boot_metadata = resource;
            }
            append(
                &mut table,
                &LookupEntry {
                    resource,
                    part_number: 1,
                    reference_count: 1,
                    hash: Sha1::digest(bytes).into(),
                }
                .encode(),
            )?;
        }
        header.blob_table = raw_resource(&mut output, &table, 2)?;
        header.xml_data = raw_resource(&mut output, &xml.encode_utf16le()?, 2)?;
        output[..HEADER_SIZE].copy_from_slice(&header.encode_canonical());
        write_archive(&Archive::open(&output)?, options)
    }
}
fn append(output: &mut Vec<u8>, bytes: &[u8]) -> Result<(), ParseError> {
    output
        .try_reserve(bytes.len())
        .map_err(|_| ParseError::Nomem)?;
    output.extend_from_slice(bytes);
    Ok(())
}
fn raw_resource(
    output: &mut Vec<u8>,
    bytes: &[u8],
    flags: u8,
) -> Result<ResourceHeader, ParseError> {
    if bytes.len() as u64 >= 1 << 56 {
        return Err(ParseError::InvalidParam);
    }
    let resource = ResourceHeader {
        size_in_wim: bytes.len() as u64,
        uncompressed_size: bytes.len() as u64,
        offset_in_wim: output.len() as u64,
        flags,
    };
    append(output, bytes)?;
    Ok(resource)
}
