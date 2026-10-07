//! Borrowed, lossless decoding of uncompressed WIM image metadata.
//!
//! Raw records retain reserved fields and tagged metadata bytes. The visible
//! tree applies wimlib's name filtering without destroying the source buffer.
use crate::allocation::*;
use crate::{ParseError, u16_at, u32_at, u64_at};
use alloc::vec::Vec;
use hashbrown::HashMap;
use hashbrown::HashSet;

fn invalid() -> ParseError {
    ParseError::InvalidMetadataResource
}
fn aligned(n: u64) -> Result<usize, ParseError> {
    usize::try_from(n.checked_add(7).ok_or_else(invalid)? & !7).map_err(|_| invalid())
}
fn region(b: &[u8], start: usize, len: usize) -> Result<&[u8], ParseError> {
    b.get(start..start.checked_add(len).ok_or_else(invalid)?)
        .ok_or_else(invalid)
}
/// Security descriptors, which remain opaque self-relative Windows SD bytes.
#[derive(Debug)]
pub struct SecurityTable<'a> {
    pub total_length: usize,
    pub descriptors: Vec<&'a [u8]>,
    pub raw: &'a [u8],
}
impl<'a> SecurityTable<'a> {
    pub fn parse(bytes: &'a [u8]) -> Result<Self, ParseError> {
        region(bytes, 0, 8)?;
        let total_length = aligned(u32_at(bytes, 0) as u64)?.max(8);
        let count = u32_at(bytes, 4) as usize;
        if count > 0x80000000 {
            return Err(invalid());
        }
        let raw = region(bytes, 0, total_length)?;
        let mut p = 8usize
            .checked_add(count.checked_mul(8).ok_or_else(invalid)?)
            .ok_or_else(invalid)?;
        region(raw, 0, p)?;
        let mut descriptors = Vec::new();
        descriptors
            .try_reserve(count)
            .map_err(|_| ParseError::Nomem)?;
        for i in 0..count {
            let size = u64_at(raw, 8 + i * 8);
            if size > u32::MAX as u64 {
                return Err(invalid());
            }
            let size = size as usize;
            descriptors
                .try_push(region(raw, p, size)?)
                .map_err(|_| ParseError::Nomem)?;
            p += size;
        }
        Ok(Self {
            total_length,
            descriptors,
            raw,
        })
    }
}
/// The semantic stream type inferred by upstream's stream ordering rules.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StreamType {
    Unknown,
    Data,
    ReparsePoint,
    EncryptedRaw,
}
/// One stream; the implicit main hash has an empty raw record.
#[derive(Debug)]
pub struct Stream<'a> {
    pub hash: [u8; 20],
    pub name: &'a [u8],
    pub kind: StreamType,
    pub raw: &'a [u8],
}
/// An on-disk directory entry, including extra stream records.
#[derive(Debug)]
pub struct Dentry<'a> {
    pub offset: usize,
    pub attributes: u32,
    pub security_id: u32,
    pub subdir_offset: u64,
    pub creation_time: u64,
    pub last_access_time: u64,
    pub last_write_time: u64,
    pub unknown_0x54: u32,
    /// Raw union: reparse information or hard link group ID.
    pub inode_union: u64,
    pub name: &'a [u8],
    pub short_name: &'a [u8],
    pub tagged_items: &'a [u8],
    pub streams: Vec<Stream<'a>>,
    pub raw: &'a [u8],
}
impl<'a> Dentry<'a> {
    /// Decode one record; a length rounded to at most eight is a terminator.
    pub fn parse(bytes: &'a [u8], offset: usize) -> Result<Option<(Self, usize)>, ParseError> {
        let head = region(bytes, offset, 8)?;
        let len = aligned(u64_at(head, 0))?;
        if len <= 8 {
            return Ok(None);
        }
        if len < 102 {
            return Err(invalid());
        }
        let record = region(bytes, offset, len)?;
        let name_len = u16_at(record, 100) as usize;
        let short_len = u16_at(record, 98) as usize;
        if (name_len | short_len) & 1 != 0 {
            return Err(invalid());
        }
        let mut cursor = 102;
        let name = read_name(record, &mut cursor, name_len, true)?;
        let short_name = read_name(record, &mut cursor, short_len, true)?;
        let extra_start = aligned((offset + cursor) as u64)?
            .saturating_sub(offset)
            .min(len);
        let tagged_items = &record[extra_start..];
        let attributes = u32_at(record, 8);
        let mut hash = [0; 20];
        hash.copy_from_slice(&record[64..84]);
        let count = u16_at(record, 96) as usize;
        let mut streams = Vec::new();
        streams
            .try_reserve(count + 1)
            .map_err(|_| ParseError::Nomem)?;
        streams
            .try_push(Stream {
                hash,
                name: &[],
                kind: StreamType::Unknown,
                raw: &[],
            })
            .map_err(|_| ParseError::Nomem)?;
        let mut next = offset + len;
        for _ in 0..count {
            let h = region(bytes, next, 38)?;
            let length = aligned(u64_at(h, 0))?;
            if length < 38 {
                return Err(invalid());
            }
            let raw = region(bytes, next, length)?;
            let mut cursor = 38;
            let name = read_name(raw, &mut cursor, u16_at(raw, 36) as usize, false)?;
            let mut hash = [0; 20];
            hash.copy_from_slice(&raw[16..36]);
            streams
                .try_push(Stream {
                    hash,
                    name,
                    kind: StreamType::Unknown,
                    raw,
                })
                .map_err(|_| ParseError::Nomem)?;
            next += length;
        }
        assign_stream_types(attributes, &mut streams);
        Ok(Some((
            Self {
                offset,
                attributes,
                security_id: u32_at(record, 12),
                subdir_offset: u64_at(record, 16),
                creation_time: u64_at(record, 40),
                last_access_time: u64_at(record, 48),
                last_write_time: u64_at(record, 56),
                unknown_0x54: u32_at(record, 84),
                inode_union: u64_at(record, 88),
                name,
                short_name,
                tagged_items,
                streams,
                raw: region(bytes, offset, next - offset)?,
            },
            next,
        )))
    }
    /// Retrieve the first matching tagged item. Malformed tails remain in raw
    /// storage but terminate lookup, as in inode_get_tagged_item().
    pub fn tagged_item(&self, tag: u32, min_len: usize) -> Option<&'a [u8]> {
        let mut b = self.tagged_items;
        while b.len() >= 8usize.checked_add(min_len)? {
            let n = u32_at(b, 4) as usize;
            let full = 8usize.checked_add(aligned(n as u64).ok()?)?;
            if full > b.len() {
                return None;
            }
            if u32_at(b, 0) == tag && n >= min_len {
                return b.get(8..8 + n);
            }
            b = &b[full..];
        }
        None
    }
    pub fn is_directory(&self) -> bool {
        self.attributes & 0x10 != 0
    }
    pub fn hard_link_group_id(&self) -> Option<u64> {
        (self.attributes & 0x400 == 0).then_some(self.inode_union)
    }
    pub fn reparse_fields(&self) -> Option<(u32, u16, u16)> {
        (self.attributes & 0x400 != 0).then_some((
            self.inode_union as u32,
            (self.inode_union >> 32) as u16,
            (self.inode_union >> 48) as u16,
        ))
    }
}
fn read_name<'a>(
    b: &'a [u8],
    p: &mut usize,
    n: usize,
    terminator: bool,
) -> Result<&'a [u8], ParseError> {
    if n & 1 != 0 {
        return Err(invalid());
    }
    if n == 0 {
        return Ok(&[]);
    }
    let name = region(b, *p, n)?;
    *p += n;
    if terminator {
        region(b, *p, 2)?;
        *p += 2;
    }
    Ok(name)
}
fn assign_stream_types(attributes: u32, streams: &mut [Stream<'_>]) {
    if attributes & 0x4000 != 0 {
        if let Some(s) = streams
            .iter_mut()
            .find(|s| s.name.is_empty() && s.hash != [0; 20])
        {
            s.kind = StreamType::EncryptedRaw;
        }
        return;
    }
    let mut reparse = false;
    let mut data = false;
    for (i, s) in streams.iter_mut().enumerate() {
        if !s.name.is_empty() {
            s.kind = StreamType::Data;
        } else if i != 0 || s.hash != [0; 20] {
            if attributes & 0x400 != 0 && !reparse {
                reparse = true;
                s.kind = StreamType::ReparsePoint;
            } else if !data {
                data = true;
                s.kind = StreamType::Data;
            }
        }
    }
    if !reparse && !data {
        streams[0].kind = if attributes & 0x400 != 0 {
            StreamType::ReparsePoint
        } else {
            StreamType::Data
        };
    }
}
/// A visible tree node. Index zero is the root, whose names are logically empty.
#[derive(Debug)]
pub struct Node<'a> {
    pub entry: Dentry<'a>,
    pub parent: Option<usize>,
    pub children: Vec<usize>,
    /// Index of the canonical node supplying this alias's inode fields.
    pub inode: usize,
}
/// Parsed security table and directory tree with the original bytes retained.
#[derive(Debug)]
pub struct Metadata<'a> {
    pub security: SecurityTable<'a>,
    pub nodes: Vec<Node<'a>>,
    pub raw: &'a [u8],
}
impl<'a> Metadata<'a> {
    /// Resolve a visible node's shared inode fields after hard-link fixup.
    pub fn inode_entry(&self, node: usize) -> Option<&Dentry<'a>> {
        let canonical = self.nodes.get(node)?.inode;
        self.nodes.get(canonical).map(|n| &n.entry)
    }
    /// Invalid SD indices are normalized to absent, matching fix_security_ids.
    pub fn security_descriptor(&self, node: usize) -> Option<&'a [u8]> {
        let id = self.inode_entry(node)?.security_id as usize;
        self.security.descriptors.get(id).copied()
    }

    pub fn parse(bytes: &'a [u8]) -> Result<Self, ParseError> {
        let security = SecurityTable::parse(bytes)?;
        let mut nodes = Vec::new();
        if let Some((entry, _)) = Dentry::parse(bytes, security.total_length)? {
            if entry.attributes & 0x410 != 0x10 {
                return Err(invalid());
            }
            nodes
                .try_push(Node {
                    entry,
                    parent: None,
                    children: Vec::new(),
                    inode: 0,
                })
                .map_err(|_| ParseError::Nomem)?;
            // Explicit DFS frames avoid stack overflow. Ancestor list offsets
            // detect cycles without rejecting valid repeated sibling lists.
            let mut stack = Vec::new();
            let mut active = HashSet::new();
            if nodes[0].entry.subdir_offset != 0 {
                let p = usize::try_from(nodes[0].entry.subdir_offset).map_err(|_| invalid())?;
                active
                    .try_insert_checked(p)
                    .map_err(|_| ParseError::Nomem)?;
                stack
                    .try_push((0usize, p, p, HashSet::<&[u8]>::new()))
                    .map_err(|_| ParseError::Nomem)?;
            }
            while let Some((parent, cursor, start, names)) = stack.last_mut() {
                let parent = *parent;
                let parsed = Dentry::parse(bytes, *cursor)?;
                let Some((entry, next)) = parsed else {
                    active.remove(start);
                    stack.pop();
                    continue;
                };
                *cursor = next;
                if ignored_name(entry.name)
                    || !names
                        .try_insert_checked(entry.name)
                        .map_err(|_| ParseError::Nomem)?
                {
                    continue;
                }
                let subdir = entry.subdir_offset;
                let directory = entry.attributes & 0x410 == 0x10;
                if subdir != 0 && !directory {
                    return Err(invalid());
                }
                let index = nodes.len();
                nodes.try_reserve(1).map_err(|_| ParseError::Nomem)?;
                nodes
                    .try_push(Node {
                        entry,
                        parent: Some(parent),
                        children: Vec::new(),
                        inode: index,
                    })
                    .map_err(|_| ParseError::Nomem)?;
                nodes[parent]
                    .children
                    .try_push(index)
                    .map_err(|_| ParseError::Nomem)?;
                if directory && subdir != 0 {
                    let p = usize::try_from(subdir).map_err(|_| invalid())?;
                    if stack.len() >= 16384
                        || !active
                            .try_insert_checked(p)
                            .map_err(|_| ParseError::Nomem)?
                    {
                        return Err(invalid());
                    }
                    stack
                        .try_push((index, p, p, HashSet::new()))
                        .map_err(|_| ParseError::Nomem)?;
                }
            }
            // Upstream collates NTFS uppercase names, with a case-sensitive tie break.
            let mut names = Vec::new();
            names
                .try_extend(nodes.iter().map(|n| n.entry.name))
                .map_err(|_| ParseError::Nomem)?;
            for n in &mut nodes {
                n.children
                    .sort_by(|&a, &b| crate::ntfs_upcase::compare_names(names[a], names[b]));
            }
        }
        // Upstream inode fixup walks the sorted tree, merging only regular
        // files whose nonzero group ID AND unnamed data digest agree.
        let mut groups = HashMap::new();
        let mut pending = Vec::new();
        if !nodes.is_empty() {
            pending.try_push(0).map_err(|_| ParseError::Nomem)?;
        }
        while let Some(index) = pending.pop() {
            pending
                .try_extend(nodes[index].children.iter().rev().copied())
                .map_err(|_| ParseError::Nomem)?;
            let entry = &nodes[index].entry;
            if !entry.is_directory()
                && let Some(id) = entry.hard_link_group_id().filter(|&id| id != 0)
            {
                let hash = entry
                    .streams
                    .iter()
                    .find(|s| s.kind == StreamType::Data && s.name.is_empty())
                    .map_or([0; 20], |s| s.hash);
                let canonical = if let Some(&canonical) = groups.get(&(id, hash)) {
                    canonical
                } else {
                    groups
                        .try_insert_checked((id, hash), index)
                        .map_err(|_| ParseError::Nomem)?;
                    index
                };
                nodes[index].inode = canonical;
            }
        }
        Ok(Self {
            security,
            nodes,
            raw: bytes,
        })
    }
}
fn ignored_name(name: &[u8]) -> bool {
    name.is_empty()
        || name == [b'.', 0]
        || name == [b'.', 0, b'.', 0]
        || name.chunks_exact(2).any(|u| u == [0, 0])
}
