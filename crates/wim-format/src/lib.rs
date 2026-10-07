//! Native WIM header, lookup-table and whole-resource reading primitives.
//! Image metadata parsing and filesystem operations remain separate layers.
//! Structural decoding follows wimlib header.c; open-time compression checks are
//! separate so callers can inspect headers with invalid compression settings.
#![cfg_attr(not(feature = "std"), no_std)]
extern crate alloc;
#[cfg(test)]
extern crate std;
pub use wim_types::{CompressionType as Compression, ErrorCode as ParseError};

mod allocation;
pub mod archive;
#[cfg(feature = "std")]
pub mod file_archive;
#[cfg(feature = "std")]
pub mod file_resource;
pub mod image_build;
pub mod image_ops;
pub mod integrity;
pub mod lookup;
pub mod metadata;
pub mod metadata_write;
pub mod ntfs_upcase;
#[cfg(feature = "std")]
pub mod pipable_image;
#[cfg(feature = "std")]
pub mod pipable_read;
pub mod pipable_write;
pub mod platform_text;
pub mod repack;
pub mod resource;
pub mod resource_write;
pub mod solid_write;
pub mod split_join;
pub mod xml;

/// Fixed disk header length.
pub const HEADER_SIZE: usize = 208;
/// Standard WIM signature.
pub const WIM_MAGIC: [u8; 8] = *b"MSWIM\0\0\0";
/// wimlib pipable WIM signature.
pub const PIPABLE_MAGIC: [u8; 8] = *b"WLPWM\0\0\0";

/// A 24-byte resource location and size descriptor, including uninterpreted flags.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct ResourceHeader {
    pub size_in_wim: u64,
    pub flags: u8,
    pub offset_in_wim: u64,
    pub uncompressed_size: u64,
}
impl ResourceHeader {
    /// Decode one descriptor. Extra bytes are ignored and flags remain opaque.
    pub fn parse(bytes: &[u8]) -> Result<Self, ParseError> {
        if bytes.len() < 24 {
            return Err(ParseError::UnexpectedEndOfFile);
        }
        let mut size = [0; 8];
        size[..7].copy_from_slice(&bytes[..7]);
        Ok(Self {
            size_in_wim: u64::from_le_bytes(size),
            flags: bytes[7],
            offset_in_wim: u64_at(bytes, 8),
            uncompressed_size: u64_at(bytes, 16),
        })
    }
    /// Encode, truncating size to 56 bits as upstream's bitfield assignment does.
    pub fn encode(&self) -> [u8; 24] {
        let mut b = [0; 24];
        b[..7].copy_from_slice(&self.size_in_wim.to_le_bytes()[..7]);
        b[7] = self.flags;
        b[8..16].copy_from_slice(&self.offset_in_wim.to_le_bytes());
        b[16..24].copy_from_slice(&self.uncompressed_size.to_le_bytes());
        b
    }
}
/// Decoded fixed WIM header, retaining reserved bytes for lossless inspection.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Header {
    pub magic: [u8; 8],
    pub version: u32,
    pub flags: u32,
    pub chunk_size: u32,
    pub guid: [u8; 16],
    pub part_number: u16,
    pub total_parts: u16,
    pub image_count: u32,
    pub blob_table: ResourceHeader,
    pub xml_data: ResourceHeader,
    pub boot_metadata: ResourceHeader,
    pub boot_index: u32,
    pub integrity_table: ResourceHeader,
    pub reserved: [u8; 60],
}
impl Header {
    /// Select the correct header from a seekable file, including a pipable WIM.
    /// A pipable file's initial magic is retained, while its final header fields
    /// are used. Upstream does not validate the final copy's magic separately.
    pub fn parse_seekable(file: &[u8]) -> Result<Self, ParseError> {
        let first = file
            .get(..HEADER_SIZE)
            .ok_or(ParseError::UnexpectedEndOfFile)?;
        if first[..8] != PIPABLE_MAGIC {
            return Self::parse(first, Some(file.len() as u64));
        }
        let mut selected = [0; HEADER_SIZE];
        selected.copy_from_slice(&file[file.len() - HEADER_SIZE..]);
        selected[..8].copy_from_slice(&PIPABLE_MAGIC);
        Self::parse(&selected, Some(file.len() as u64))
    }

    /// Decode a header already selected by the caller. For seekable pipable
    /// files, the caller must choose the final header, as upstream does.
    /// `file_size` applies the upstream uncompressed table allocation guard;
    /// zero or None means the size is unknown. Boot index is not normalized here.
    pub fn parse(b: &[u8], file_size: Option<u64>) -> Result<Self, ParseError> {
        if b.len() < HEADER_SIZE {
            return Err(ParseError::UnexpectedEndOfFile);
        }
        let mut magic = [0; 8];
        magic.copy_from_slice(&b[..8]);
        if magic != WIM_MAGIC && magic != PIPABLE_MAGIC {
            return Err(ParseError::NotAWimFile);
        }
        if u32_at(b, 8) != 208 {
            return Err(ParseError::InvalidHeader);
        }
        let version = u32_at(b, 12);
        if version != 0x10d00 && version != 0xe00 {
            return Err(ParseError::UnknownVersion);
        }
        let part_number = u16_at(b, 40);
        let total_parts = u16_at(b, 42);
        if total_parts == 0 || part_number == 0 || part_number > total_parts {
            return Err(ParseError::InvalidPartNumber);
        }
        let image_count = u32_at(b, 44);
        if image_count > 65535 {
            return Err(ParseError::ImageCount);
        }
        let mut guid = [0; 16];
        guid.copy_from_slice(&b[24..40]);
        let mut reserved = [0; 60];
        reserved.copy_from_slice(&b[148..208]);
        let h = Self {
            magic,
            version,
            flags: u32_at(b, 16),
            chunk_size: u32_at(b, 20),
            guid,
            part_number,
            total_parts,
            image_count,
            blob_table: ResourceHeader::parse(&b[48..72])?,
            xml_data: ResourceHeader::parse(&b[72..96])?,
            boot_metadata: ResourceHeader::parse(&b[96..120])?,
            boot_index: u32_at(b, 120),
            integrity_table: ResourceHeader::parse(&b[124..148])?,
            reserved,
        };
        if let Some(size) = file_size.filter(|&n| n > 0)
            && [h.blob_table, h.xml_data, h.integrity_table]
                .iter()
                .any(|r| r.uncompressed_size > size)
        {
            return Err(ParseError::InvalidHeader);
        }
        Ok(h)
    }
    /// Apply compression selection and chunk validation from open_wim().
    /// Multiple algorithm flags are accepted with upstream's precedence.
    pub fn validate_compression(&self) -> Result<Compression, ParseError> {
        let c = if self.flags & 2 == 0 {
            Compression::None
        } else if self.flags & 0x40000 != 0 {
            Compression::Lzx
        } else if self.flags & (0x20000 | 0x200000) != 0 {
            Compression::Xpress
        } else if self.flags & 0x80000 != 0 {
            Compression::Lzms
        } else {
            return Err(ParseError::InvalidCompressionType);
        };
        c.validate_chunk_size(self.chunk_size)?;
        Ok(c)
    }
    /// Encode without validation, preserving reserved bytes.
    pub fn encode(&self) -> [u8; 208] {
        let mut b = [0; 208];
        b[..8].copy_from_slice(&self.magic);
        b[8..12].copy_from_slice(&208u32.to_le_bytes());
        for (o, v) in [
            (12, self.version),
            (16, self.flags),
            (20, self.chunk_size),
            (44, self.image_count),
            (120, self.boot_index),
        ] {
            b[o..o + 4].copy_from_slice(&v.to_le_bytes());
        }
        b[24..40].copy_from_slice(&self.guid);
        b[40..42].copy_from_slice(&self.part_number.to_le_bytes());
        b[42..44].copy_from_slice(&self.total_parts.to_le_bytes());
        for (o, r) in [
            (48, self.blob_table),
            (72, self.xml_data),
            (96, self.boot_metadata),
            (124, self.integrity_table),
        ] {
            b[o..o + 24].copy_from_slice(&r.encode());
        }
        b[148..].copy_from_slice(&self.reserved);
        b
    }
    /// Encode like write_wim_header(), zeroing the unused 60 bytes.
    pub fn encode_canonical(&self) -> [u8; 208] {
        let mut b = self.encode();
        b[148..].fill(0);
        b
    }
}
fn u16_at(b: &[u8], o: usize) -> u16 {
    u16::from_le_bytes([b[o], b[o + 1]])
}
fn u32_at(b: &[u8], o: usize) -> u32 {
    u32::from_le_bytes([b[o], b[o + 1], b[o + 2], b[o + 3]])
}
fn u64_at(b: &[u8], o: usize) -> u64 {
    let mut a = [0; 8];
    a.copy_from_slice(&b[o..o + 8]);
    u64::from_le_bytes(a)
}
