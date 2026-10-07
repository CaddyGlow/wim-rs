// SPDX-License-Identifier: LGPL-2.1-or-later OR GPL-3.0-or-later
// Lookup format and behavior translated from wimlib src/blob_table.c at
// cd5e231c348c255ae5088873b5a66ee0eb96fa07 (Eric Biggers and contributors).
//! Lookup-table records and resolution of ordinary and solid resource runs.

use crate::allocation::*;
use crate::{Header, ParseError, ResourceHeader};
#[cfg(test)]
use alloc::vec;
use alloc::vec::Vec;
use hashbrown::HashMap;

/// On-disk lookup-table record length.
pub const ENTRY_SIZE: usize = 50;
/// Marker distinguishing solid resources from blobs within a solid run.
pub const SOLID_RESOURCE_MARKER: u64 = 0x1_0000_0000;

/// One unprocessed record, retaining every disk field.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LookupEntry {
    /// Resource location, flags and sizes (overloaded for solid blobs).
    pub resource: ResourceHeader,
    /// Split part number, indexed from one.
    pub part_number: u16,
    /// References across images; zero does not always imply an unused blob.
    pub reference_count: u32,
    /// SHA-1 digest of the blob, or zero for special/empty records.
    pub hash: [u8; 20],
}

impl LookupEntry {
    /// Read a record; short buffers are an EOF error and trailing bytes ignored.
    pub fn parse(bytes: &[u8]) -> Result<Self, ParseError> {
        if bytes.len() < ENTRY_SIZE {
            return Err(ParseError::UnexpectedEndOfFile);
        }
        let mut hash = [0; 20];
        hash.copy_from_slice(&bytes[30..50]);
        Ok(Self {
            resource: ResourceHeader::parse(bytes)?,
            part_number: u16::from_le_bytes([bytes[24], bytes[25]]),
            reference_count: u32::from_le_bytes([bytes[26], bytes[27], bytes[28], bytes[29]]),
            hash,
        })
    }

    /// Encode without changing flags or the recorded reference count.
    pub fn encode(&self) -> [u8; ENTRY_SIZE] {
        let mut bytes = [0; ENTRY_SIZE];
        bytes[..24].copy_from_slice(&self.resource.encode());
        bytes[24..26].copy_from_slice(&self.part_number.to_le_bytes());
        bytes[26..30].copy_from_slice(&self.reference_count.to_le_bytes());
        bytes[30..].copy_from_slice(&self.hash);
        bytes
    }
}

/// Backing resource with resolved compression settings.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LookupResource {
    /// Disk location; solid marker remains in this raw header.
    pub header: ResourceHeader,
    /// Actual size read from the alternate header for solid resources.
    pub uncompressed_size: u64,
    /// Compression format advertised by the file or alternate header.
    pub compression_code: u32,
    /// Maximum chunk size for decompression.
    pub chunk_size: u32,
    /// Whether multiple blobs can share this resource.
    pub solid: bool,
}

/// A retained blob, separate from its backing resource identity.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LookupBlob {
    /// Content digest; metadata blobs retain their digests too.
    pub hash: [u8; 20],
    /// Index into [`LookupTable::resources`].
    pub resource_index: usize,
    /// Byte offset within the resource's uncompressed data.
    pub offset: u64,
    /// Blob length in bytes.
    pub size: u64,
    /// Reference count from the original record.
    pub reference_count: u32,
    /// Effective flags after version-specific normalization.
    pub flags: u8,
}

/// Resolved table. Metadata order is image order; content order is disk order.
#[derive(Debug)]
pub struct LookupTable {
    /// All resource descriptors, including unreferenced solid descriptors.
    pub resources: Vec<LookupResource>,
    /// First retained data blob for each distinct nonzero hash.
    pub blobs: Vec<LookupBlob>,
    /// Metadata blobs in their original lookup-table order.
    pub metadata: Vec<LookupBlob>,
    /// Image count after first-part metadata reconciliation.
    pub effective_image_count: u32,
    index: HashMap<[u8; 20], usize>,
}

impl LookupTable {
    /// Copy descriptor ownership without reading or copying enclosing payloads.
    /// Independent mutable handles must not share their reference counts.
    pub fn try_clone(&self) -> Result<Self, ParseError> {
        let mut resources = Vec::new();
        resources
            .try_extend(self.resources.iter().cloned())
            .map_err(|_| ParseError::Nomem)?;
        let mut blobs = Vec::new();
        blobs
            .try_extend(self.blobs.iter().cloned())
            .map_err(|_| ParseError::Nomem)?;
        let mut metadata = Vec::new();
        metadata
            .try_extend(self.metadata.iter().cloned())
            .map_err(|_| ParseError::Nomem)?;
        let mut index = crate::allocation::map(self.blobs.len().max(1))?;
        for (position, blob) in blobs.iter().enumerate() {
            index
                .try_insert_checked(blob.hash, position)
                .map_err(|_| ParseError::Nomem)?;
        }
        Ok(Self {
            resources,
            blobs,
            metadata,
            index,
            effective_image_count: self.effective_image_count,
        })
    }
    /// Resolve records with upstream filtering and solid-run assignment.
    ///
    /// `read_solid_header` reads exactly 16 bytes at the supplied file offset.
    /// Its errors propagate unchanged. The parser ignores incomplete trailing
    /// records, matching upstream's floor division of the table length.
    pub fn parse(
        bytes: &[u8],
        header: &Header,
        mut read_solid_header: impl FnMut(u64) -> Result<[u8; 16], ParseError>,
    ) -> Result<Self, ParseError> {
        let compression = header.validate_compression()?;
        let count = bytes.len() / ENTRY_SIZE;
        let mut entries = Vec::new();
        entries.try_reserve(count).map_err(|_| ParseError::Nomem)?;
        for raw in bytes.chunks_exact(ENTRY_SIZE) {
            let mut entry = LookupEntry::parse(raw)?;
            if header.version == 0x10d00 {
                entry.resource.flags &= !0x10;
            }
            entries.try_push(entry).map_err(|_| ParseError::Nomem)?;
        }
        let mut table = Self {
            resources: Vec::new(),
            blobs: Vec::new(),
            metadata: Vec::new(),
            effective_image_count: header.image_count,
            index: crate::allocation::map(count.max(1))?,
        };
        table
            .resources
            .try_reserve(count)
            .map_err(|_| ParseError::Nomem)?;
        table
            .blobs
            .try_reserve(count)
            .map_err(|_| ParseError::Nomem)?;
        table
            .metadata
            .try_reserve(count.min(header.image_count as usize))
            .map_err(|_| ParseError::Nomem)?;
        table
            .index
            .try_reserve(count)
            .map_err(|_| ParseError::Nomem)?;
        let mut intervals = Vec::new();
        intervals
            .try_reserve(count)
            .map_err(|_| ParseError::Nomem)?;
        let mut cursor = 0;
        while cursor < entries.len() {
            let run_end = if entries[cursor].resource.flags & 0x10 != 0 {
                entries[cursor..]
                    .iter()
                    .position(|e| e.resource.flags & 0x10 == 0)
                    .map_or(entries.len(), |n| cursor + n)
            } else {
                cursor + 1
            };
            let solid = entries[cursor].resource.flags & 0x10 != 0;
            let resource_start = table.resources.len();
            if solid {
                for entry in &entries[cursor..run_end] {
                    if entry.resource.uncompressed_size != SOLID_RESOURCE_MARKER {
                        continue;
                    }
                    let raw = read_solid_header(entry.resource.offset_in_wim)?;
                    let mut size = [0; 8];
                    size.copy_from_slice(&raw[..8]);
                    table
                        .resources
                        .try_push(LookupResource {
                            header: entry.resource,
                            uncompressed_size: u64::from_le_bytes(size),
                            chunk_size: u32::from_le_bytes([raw[8], raw[9], raw[10], raw[11]]),
                            // Upstream stores this in a 22-bit field, and defers
                            // codec/chunk validation until resource consumption.
                            compression_code: u32::from_le_bytes([
                                raw[12], raw[13], raw[14], raw[15],
                            ]) & 0x003f_ffff,
                            solid: true,
                        })
                        .map_err(|_| ParseError::Nomem)?;
                }
            }
            intervals.clear();
            for entry in &entries[cursor..run_end] {
                let res = entry.resource;
                if solid && res.uncompressed_size == SOLID_RESOURCE_MARKER {
                    continue;
                }
                let (resource_index, offset, size) = if solid {
                    let mut offset = res.offset_in_wim;
                    let mut assigned = None;
                    for (i, resource) in table.resources[resource_start..].iter().enumerate() {
                        // Match unsigned C assignment. Retained ranges are
                        // checked below before any resource bytes are read.
                        if offset.wrapping_add(res.size_in_wim) <= resource.uncompressed_size {
                            assigned = Some((resource_start + i, offset));
                            break;
                        }
                        offset = offset.wrapping_sub(resource.uncompressed_size);
                    }
                    let (index, offset) = assigned.ok_or(ParseError::InvalidLookupTableEntry)?;
                    (index, offset, res.size_in_wim)
                } else {
                    if res.flags & 4 == 0 && res.size_in_wim != res.uncompressed_size {
                        return Err(ParseError::InvalidLookupTableEntry);
                    }
                    (table.resources.len(), 0, res.uncompressed_size)
                };
                if entry.hash == [0; 20] || size == 0 || entry.part_number != header.part_number {
                    continue;
                }
                let metadata = res.flags & 2 != 0;
                if metadata {
                    if entry.reference_count == 0 {
                        continue;
                    }
                    if entry.reference_count != 1 || solid {
                        return Err(ParseError::InvalidLookupTableEntry);
                    }
                    if header.part_number != 1
                        || table.metadata.len() == header.image_count as usize
                    {
                        continue;
                    }
                } else if table.index.contains_key(&entry.hash) {
                    continue;
                }
                if !solid {
                    table
                        .resources
                        .try_push(LookupResource {
                            header: res,
                            uncompressed_size: size,
                            compression_code: if res.flags & 4 != 0 {
                                compression as u32
                            } else {
                                0
                            },
                            chunk_size: if res.flags & 4 != 0 {
                                header.chunk_size
                            } else {
                                0
                            },
                            solid: false,
                        })
                        .map_err(|_| ParseError::Nomem)?;
                }
                let blob = LookupBlob {
                    hash: entry.hash,
                    resource_index,
                    offset,
                    size,
                    reference_count: entry.reference_count,
                    flags: res.flags,
                };
                if metadata {
                    table
                        .metadata
                        .try_push(blob)
                        .map_err(|_| ParseError::Nomem)?;
                } else {
                    table
                        .index
                        .try_insert_checked(entry.hash, table.blobs.len())
                        .map_err(|_| ParseError::Nomem)?;
                    table.blobs.try_push(blob).map_err(|_| ParseError::Nomem)?;
                    if solid {
                        intervals
                            .try_push((resource_index, offset, size))
                            .map_err(|_| ParseError::Nomem)?;
                    }
                }
            }
            if solid {
                for resource in &table.resources[resource_start..] {
                    resource
                        .header
                        .offset_in_wim
                        .checked_add(resource.header.size_in_wim)
                        .ok_or(ParseError::InvalidLookupTableEntry)?;
                }
                intervals.sort_unstable();
                let mut previous = None;
                for &(index, offset, size) in &intervals {
                    let end = offset
                        .checked_add(size)
                        .ok_or(ParseError::InvalidLookupTableEntry)?;
                    if end > table.resources[index].uncompressed_size
                        || previous
                            .is_some_and(|(i, previous_end)| i == index && offset < previous_end)
                    {
                        return Err(ParseError::InvalidLookupTableEntry);
                    }
                    previous = Some((index, end));
                }
            }
            cursor = run_end;
        }
        if header.part_number == 1 {
            table.effective_image_count = table.metadata.len() as u32;
        }
        Ok(table)
    }

    /// Resolve content by hash, retaining the first accepted descriptor.
    pub fn find(&self, hash: &[u8; 20]) -> Option<&LookupBlob> {
        self.index
            .get(hash)
            .and_then(|&i| self.blobs.get(i))
            .filter(|blob| &blob.hash == hash)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Compression;

    fn header(version: u32, images: u32) -> Header {
        let mut h =
            Header::parse(include_bytes!("../tests/fixtures/empty_dacl.header"), None).unwrap();
        h.version = version;
        h.image_count = images;
        h
    }

    fn entry(hash: u8, flags: u8, size: u64, offset: u64, usize: u64) -> LookupEntry {
        LookupEntry {
            resource: ResourceHeader {
                size_in_wim: size,
                flags,
                offset_in_wim: offset,
                uncompressed_size: usize,
            },
            part_number: 1,
            reference_count: 1,
            hash: [hash; 20],
        }
    }

    fn table(entries: &[LookupEntry], h: &Header) -> Result<LookupTable, ParseError> {
        let bytes: Vec<_> = entries.iter().flat_map(LookupEntry::encode).collect();
        LookupTable::parse(&bytes, h, |_| Err(ParseError::Read))
    }

    #[test]
    fn disk_record_retains_exact_layout_and_rejects_every_truncation() {
        let e = entry(3, 0xab, 0x123456789abc, u64::MAX, 123);
        let bytes = e.encode();
        assert_eq!(LookupEntry::parse(&bytes), Ok(e));
        for n in 0..ENTRY_SIZE {
            assert_eq!(
                LookupEntry::parse(&bytes[..n]),
                Err(ParseError::UnexpectedEndOfFile)
            );
        }
    }

    #[test]
    fn first_duplicate_wins_and_metadata_follows_table_order() {
        let entries = [
            entry(1, 2, 10, 90, 10),
            entry(2, 0, 7, 40, 7),
            entry(2, 0, 9, 50, 9),
            entry(3, 2, 11, 80, 11),
        ];
        let t = table(&entries, &header(0x10d00, 2)).unwrap();
        assert_eq!(t.blobs.len(), 1);
        assert_eq!(t.find(&[2; 20]).unwrap().size, 7);
        assert_eq!(
            t.metadata.iter().map(|b| b.hash).collect::<Vec<_>>(),
            vec![[1; 20], [3; 20]]
        );
    }

    #[test]
    fn absent_metadata_reconciles_first_part_image_count_and_ignores_trailing_bytes() {
        let mut bytes = entry(4, 0, 1, 208, 1).encode().to_vec();
        bytes.extend_from_slice(&[0xfa; 49]);
        let t = LookupTable::parse(&bytes, &header(0x10d00, 5), |_| unreachable!()).unwrap();
        assert_eq!(t.effective_image_count, 0);
        assert_eq!(t.blobs.len(), 1);
    }

    #[test]
    fn filtering_does_not_bypass_uncompressed_size_validation() {
        let error = table(&[entry(0, 0, 5, 208, 6)], &header(0x10d00, 0)).unwrap_err();
        assert_eq!(error, ParseError::InvalidLookupTableEntry);
    }

    #[test]
    fn ignores_zero_hash_empty_wrong_part_and_unreferenced_metadata() {
        let mut wrong_part = entry(3, 0, 5, 208, 5);
        wrong_part.part_number = 2;
        let mut unreferenced = entry(4, 2, 5, 208, 5);
        unreferenced.reference_count = 0;
        let t = table(
            &[
                entry(0, 0, 5, 208, 5),
                entry(2, 0, 0, 208, 0),
                wrong_part,
                unreferenced,
            ],
            &header(0x10d00, 1),
        )
        .unwrap();
        assert!(t.blobs.is_empty() && t.metadata.is_empty());
        assert_eq!(t.effective_image_count, 0);
    }

    #[test]
    fn ordinary_version_ignores_solid_flag_and_metadata_requires_single_reference() {
        let t = table(&[entry(1, 0x10, 5, 208, 5)], &header(0x10d00, 0)).unwrap();
        assert_eq!(t.blobs[0].flags, 0);
        let mut shared = entry(2, 2, 5, 208, 5);
        shared.reference_count = 2;
        assert_eq!(
            table(&[shared], &header(0x10d00, 1)).unwrap_err(),
            ParseError::InvalidLookupTableEntry
        );
    }

    fn solid_header(size: u64) -> [u8; 16] {
        let mut b = [0; 16];
        b[..8].copy_from_slice(&size.to_le_bytes());
        b[8..12].copy_from_slice(&32768u32.to_le_bytes());
        b[12..].copy_from_slice(&3u32.to_le_bytes());
        b
    }

    #[test]
    fn solid_blobs_use_concatenated_resource_offsets_and_descriptor_size_as_blob_length() {
        let entries = [
            entry(1, 0x10, 3, 12, 0),
            entry(0, 0x10, 100, 208, SOLID_RESOURCE_MARKER),
            entry(0, 0x10, 200, 308, SOLID_RESOURCE_MARKER),
        ];
        let bytes: Vec<_> = entries.iter().flat_map(LookupEntry::encode).collect();
        let t = LookupTable::parse(&bytes, &header(0xe00, 0), |o| {
            Ok(solid_header(if o == 208 { 10 } else { 20 }))
        })
        .unwrap();
        assert_eq!(
            (
                t.blobs[0].resource_index,
                t.blobs[0].offset,
                t.blobs[0].size
            ),
            (1, 2, 3)
        );
        assert_eq!(t.resources[1].compression_code, Compression::Lzms as u32);
    }

    #[test]
    fn solid_overlaps_are_rejected_but_out_of_order_disjoint_blobs_are_accepted() {
        let resource = entry(0, 0x10, 50, 208, SOLID_RESOURCE_MARKER);
        for (second_offset, accepted) in [(0, true), (6, false)] {
            let entries = [
                resource.clone(),
                entry(1, 0x10, 5, 5, 0),
                entry(2, 0x10, 5, second_offset, 0),
            ];
            let bytes: Vec<_> = entries.iter().flat_map(LookupEntry::encode).collect();
            let t = LookupTable::parse(&bytes, &header(0xe00, 0), |_| Ok(solid_header(20)));
            assert_eq!(t.is_ok(), accepted);
        }
    }

    #[test]
    fn solid_metadata_and_missing_backing_resource_are_rejected() {
        for entries in [
            vec![entry(1, 0x10, 5, 0, 0)],
            vec![
                entry(0, 0x10, 50, 208, SOLID_RESOURCE_MARKER),
                entry(1, 0x12, 5, 0, 0),
            ],
        ] {
            let bytes: Vec<_> = entries.iter().flat_map(LookupEntry::encode).collect();
            assert_eq!(
                LookupTable::parse(&bytes, &header(0xe00, 1), |_| Ok(solid_header(20)))
                    .unwrap_err(),
                ParseError::InvalidLookupTableEntry
            );
        }
    }
}
