// SPDX-License-Identifier: LGPL-2.1-or-later OR GPL-3.0-or-later
//! Native image metadata construction and serialization.
//! Names and Windows security/tagged data remain opaque bytes; filesystem
//! capture chooses their semantics. Offsets are rebuilt, never caller supplied.
use crate::{ParseError, metadata::Metadata};
use alloc::vec::Vec;

fn invalid() -> ParseError {
    ParseError::InvalidMetadataResource
}
fn align(n: usize) -> Result<usize, ParseError> {
    n.checked_add(7).map(|n| n & !7).ok_or_else(invalid)
}
fn append(out: &mut Vec<u8>, bytes: &[u8]) -> Result<(), ParseError> {
    out.try_reserve(bytes.len())
        .map_err(|_| ParseError::Nomem)?;
    out.extend_from_slice(bytes);
    Ok(())
}
fn zeros(out: &mut Vec<u8>, n: usize) -> Result<(), ParseError> {
    let end = out.len().checked_add(n).ok_or_else(invalid)?;
    out.try_reserve(n).map_err(|_| ParseError::Nomem)?;
    out.resize(end, 0);
    Ok(())
}
fn copy(bytes: &[u8]) -> Result<Vec<u8>, ParseError> {
    let mut v = Vec::new();
    append(&mut v, bytes)?;
    Ok(v)
}

/// Validate and reproduce every original byte, including hidden entries,
/// padding, security bytes and reserved fields. This does not normalize data.
pub fn write_lossless(metadata: &Metadata<'_>) -> Result<Vec<u8>, ParseError> {
    Metadata::parse(metadata.raw)?;
    copy(metadata.raw)
}

/// One extra stream slot, in on-disk order.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct OwnedStream {
    pub hash: [u8; 20],
    /// UTF-16LE bytes, excluding the terminator.
    pub name: Vec<u8>,
    pub reserved: [u8; 8],
    /// Opaque bytes following the name, including an existing terminator and padding.
    pub trailing: Vec<u8>,
}
/// Mutable directory entry and inode fields, independent of on-disk offsets.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OwnedDentry {
    pub attributes: u32,
    pub security_id: u32,
    pub creation_time: u64,
    pub last_access_time: u64,
    pub last_write_time: u64,
    pub unknown_0x54: u32,
    /// Hard-link group identity, or packed reparse fields for attribute 0x400.
    pub inode_union: u64,
    pub reserved: [u8; 16],
    pub main_hash: [u8; 20],
    pub name: Vec<u8>,
    pub short_name: Vec<u8>,
    pub tagged_items: Vec<u8>,
    pub extra_streams: Vec<OwnedStream>,
    pub children: Vec<usize>,
}
impl OwnedDentry {
    /// Construct a regular entry; root and directories require attribute 0x10.
    pub fn new(name: Vec<u8>, attributes: u32) -> Self {
        Self {
            attributes,
            security_id: u32::MAX,
            creation_time: 0,
            last_access_time: 0,
            last_write_time: 0,
            unknown_0x54: 0,
            inode_union: 0,
            reserved: [0; 16],
            main_hash: [0; 20],
            name,
            short_name: Vec::new(),
            tagged_items: Vec::new(),
            extra_streams: Vec::new(),
            children: Vec::new(),
        }
    }
}
/// Owned visible image tree. Node zero is an unnamed directory root.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct OwnedMetadata {
    pub security_descriptors: Vec<Vec<u8>>,
    pub nodes: Vec<OwnedDentry>,
}
impl OwnedMetadata {
    /// Copy visible entries, retaining opaque Windows fields and raw stream slots.
    /// Invalid names omitted by the reader stay omitted; use `write_lossless` to
    /// retain invisible records too. Hardlink aliases use their canonical inode.
    pub fn from_metadata(metadata: &Metadata<'_>) -> Result<Self, ParseError> {
        let mut security_descriptors = Vec::new();
        security_descriptors
            .try_reserve(metadata.security.descriptors.len())
            .map_err(|_| ParseError::Nomem)?;
        for sd in &metadata.security.descriptors {
            security_descriptors.push(copy(sd)?);
        }
        let mut nodes = Vec::new();
        nodes
            .try_reserve(metadata.nodes.len())
            .map_err(|_| ParseError::Nomem)?;
        for (i, node) in metadata.nodes.iter().enumerate() {
            let entry = metadata.inode_entry(i).ok_or_else(invalid)?;
            let mut d = OwnedDentry::new(
                if i == 0 {
                    Vec::new()
                } else {
                    copy(node.entry.name)?
                },
                entry.attributes,
            );
            d.security_id = if (entry.security_id as usize) < security_descriptors.len() {
                entry.security_id
            } else {
                u32::MAX
            };
            d.creation_time = entry.creation_time;
            d.last_access_time = entry.last_access_time;
            d.last_write_time = entry.last_write_time;
            d.unknown_0x54 = entry.unknown_0x54;
            d.inode_union = entry.inode_union;
            d.reserved.copy_from_slice(&entry.raw[24..40]);
            d.main_hash = entry.streams[0].hash;
            d.short_name = copy(node.entry.short_name)?;
            d.tagged_items = copy(entry.tagged_items)?;
            d.children
                .try_reserve(node.children.len())
                .map_err(|_| ParseError::Nomem)?;
            d.children.extend_from_slice(&node.children);
            d.extra_streams
                .try_reserve(entry.streams.len().saturating_sub(1))
                .map_err(|_| ParseError::Nomem)?;
            for s in entry.streams.iter().skip(1) {
                let mut reserved = [0; 8];
                reserved.copy_from_slice(&s.raw[8..16]);
                d.extra_streams.push(OwnedStream {
                    hash: s.hash,
                    name: copy(s.name)?,
                    reserved,
                    trailing: copy(&s.raw[38 + s.name.len()..])?,
                });
            }
            nodes.push(d);
        }
        Ok(Self {
            security_descriptors,
            nodes,
        })
    }
    /// Serialize a validated tree in original NTFS filename collation order.
    /// Duplicate children, cycles, unreachable nodes and invalid visible names
    /// are rejected. Unknown tagged fields are retained without interpretation.
    pub fn encode(&self) -> Result<Vec<u8>, ParseError> {
        if self.nodes.is_empty()
            || self.nodes[0].attributes & 0x10 == 0
            || !self.nodes[0].name.is_empty()
        {
            return Err(invalid());
        }
        let mut order = Vec::new();
        order
            .try_reserve(self.nodes.len())
            .map_err(|_| ParseError::Nomem)?;
        let mut seen = Vec::new();
        zeros(&mut seen, self.nodes.len())?;
        let mut pending = Vec::new();
        pending
            .try_reserve(self.nodes.len())
            .map_err(|_| ParseError::Nomem)?;
        pending.push(0);
        while let Some(i) = pending.pop() {
            let n = self.nodes.get(i).ok_or_else(invalid)?;
            if seen[i] != 0 {
                return Err(invalid());
            }
            seen[i] = 1;
            order.push(i);
            validate_name(&n.name, i == 0)?;
            validate_short(&n.short_name)?;
            if !n.children.is_empty() && n.attributes & 0x410 != 0x10 {
                return Err(invalid());
            }
            let children = sorted_children(self, n)?;
            pending.extend(children.into_iter().rev());
        }
        if order.len() != self.nodes.len() {
            return Err(invalid());
        }
        let count = u32::try_from(self.security_descriptors.len()).map_err(|_| invalid())?;
        let mut out = Vec::new();
        zeros(&mut out, 8)?;
        for sd in &self.security_descriptors {
            append(&mut out, &(sd.len() as u64).to_le_bytes())?;
        }
        for sd in &self.security_descriptors {
            append(&mut out, sd)?;
        }
        let padding = align(out.len())? - out.len();
        zeros(&mut out, padding)?;
        let total = u32::try_from(out.len()).map_err(|_| invalid())?;
        out[..4].copy_from_slice(&total.to_le_bytes());
        out[4..8].copy_from_slice(&count.to_le_bytes());
        let mut positions = Vec::new();
        positions
            .try_reserve(self.nodes.len())
            .map_err(|_| ParseError::Nomem)?;
        positions.resize(self.nodes.len(), 0);
        positions[0] = out.len();
        encode_entry(&self.nodes[0], &mut out)?;
        zeros(&mut out, 8)?;
        for i in order {
            let node = &self.nodes[i];
            // Reparse directories are opaque leaves and never have child lists.
            // WIMMount uses the child-list offset to recognize ordinary directories,
            // including empty ones. Emit their terminating empty list too.
            // The image root is identified separately and keeps the native
            // empty-image encoding without a child-list offset.
            if node.attributes & 0x410 != 0x10 || (i == 0 && node.children.is_empty()) {
                continue;
            }
            let offset = out.len() as u64;
            out[positions[i] + 16..positions[i] + 24].copy_from_slice(&offset.to_le_bytes());
            for child in sorted_children(self, node)? {
                positions[child] = out.len();
                encode_entry(&self.nodes[child], &mut out)?;
            }
            zeros(&mut out, 8)?;
        }
        Metadata::parse(&out)?;
        Ok(out)
    }
}
fn units(b: &[u8]) -> impl Iterator<Item = u16> + '_ {
    b.chunks_exact(2).map(|u| u16::from_le_bytes([u[0], u[1]]))
}
fn validate_short(name: &[u8]) -> Result<(), ParseError> {
    if name.len() > u16::MAX as usize || name.len() & 1 != 0 || units(name).any(|u| u == 0) {
        return Err(invalid());
    }
    Ok(())
}
fn validate_name(name: &[u8], root: bool) -> Result<(), ParseError> {
    validate_short(name)?;
    if !root && (name.is_empty() || name == [46, 0] || name == [46, 0, 46, 0]) {
        return Err(invalid());
    }
    Ok(())
}
fn sorted_children(m: &OwnedMetadata, n: &OwnedDentry) -> Result<Vec<usize>, ParseError> {
    let mut children = Vec::new();
    children
        .try_reserve(n.children.len())
        .map_err(|_| ParseError::Nomem)?;
    children.extend_from_slice(&n.children);
    if children.iter().any(|&i| i >= m.nodes.len()) {
        return Err(invalid());
    }
    children
        .sort_by(|&a, &b| crate::ntfs_upcase::compare_names(&m.nodes[a].name, &m.nodes[b].name));
    if children
        .windows(2)
        .any(|w| m.nodes[w[0]].name == m.nodes[w[1]].name)
    {
        return Err(invalid());
    }
    Ok(children)
}
fn encode_entry(n: &OwnedDentry, out: &mut Vec<u8>) -> Result<(), ParseError> {
    let count = u16::try_from(n.extra_streams.len()).map_err(|_| invalid())?;
    let start = out.len();
    zeros(out, 102)?;
    out[start + 8..start + 12].copy_from_slice(&n.attributes.to_le_bytes());
    out[start + 12..start + 16].copy_from_slice(&n.security_id.to_le_bytes());
    out[start + 24..start + 40].copy_from_slice(&n.reserved);
    for (offset, value) in [
        (40, n.creation_time),
        (48, n.last_access_time),
        (56, n.last_write_time),
        (88, n.inode_union),
    ] {
        out[start + offset..start + offset + 8].copy_from_slice(&value.to_le_bytes());
    }
    out[start + 64..start + 84].copy_from_slice(&n.main_hash);
    out[start + 84..start + 88].copy_from_slice(&n.unknown_0x54.to_le_bytes());
    out[start + 96..start + 98].copy_from_slice(&count.to_le_bytes());
    out[start + 98..start + 100].copy_from_slice(&(n.short_name.len() as u16).to_le_bytes());
    out[start + 100..start + 102].copy_from_slice(&(n.name.len() as u16).to_le_bytes());
    for name in [&n.name, &n.short_name] {
        if !name.is_empty() {
            append(out, name)?;
            zeros(out, 2)?;
        }
    }
    let pad = align(out.len())? - out.len();
    zeros(out, pad)?;
    append(out, &n.tagged_items)?;
    let pad = align(out.len())? - out.len();
    zeros(out, pad)?;
    let len = (out.len() - start) as u64;
    out[start..start + 8].copy_from_slice(&len.to_le_bytes());
    for s in &n.extra_streams {
        validate_short(&s.name)?;
        let start = out.len();
        zeros(out, 38)?;
        out[start + 8..start + 16].copy_from_slice(&s.reserved);
        out[start + 16..start + 36].copy_from_slice(&s.hash);
        out[start + 36..start + 38].copy_from_slice(&(s.name.len() as u16).to_le_bytes());
        append(out, &s.name)?;
        append(out, &s.trailing)?;
        // Upstream writes a UTF-16 terminator even when the name ends on an
        // alignment boundary. Imported tails already include that terminator.
        if !s.name.is_empty() && s.trailing.len() < 2 {
            zeros(out, 2 - s.trailing.len())?;
        }
        let pad = align(out.len())? - out.len();
        zeros(out, pad)?;
        let len = (out.len() - start) as u64;
        out[start..start + 8].copy_from_slice(&len.to_le_bytes());
    }
    Ok(())
}
