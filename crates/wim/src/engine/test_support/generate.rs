// SPDX-License-Identifier: LGPL-2.1-or-later
//! Faithful optional original random tree generation; not a filesystem scan.

//! No public capture flag is accepted until the staged graph is validated.
use super::{primitives, random::Random};
use crate::engine::collections::FallibleMap as _;
use crate::engine::collections::FallibleSet as _;
use sha1::{Digest, Sha1};
use std::collections::BTreeMap;
use wim_format::{
    ParseError,
    metadata_write::{OwnedDentry, OwnedMetadata, OwnedStream},
};

/// Fully generated graph and immediately hashed nonempty payloads.
pub struct GeneratedImage {
    /// Authoritative visible inode graph, including security and Windows fields.
    pub tree: OwnedMetadata,
    /// True in-memory data ownership, deduplicated by its actual SHA-1 digest.
    pub blobs: BTreeMap<[u8; 20], Vec<u8>>,
    /// Original first-hash insertion sequence, retained independently of map order.
    pub blob_order: Vec<[u8; 20]>,
}
struct Context {
    random: Random,
    metadata_only: bool,
    image: GeneratedImage,
    buckets: Vec<Vec<(u64, usize)>>,
    filled: usize,
    aliases: Vec<usize>,
    short_names: Vec<Vec<u8>>,
}
fn bucket(ino: u64, capacity: usize) -> usize {
    ino.wrapping_mul(0x9e37_ffff_fffc_0001) as usize & (capacity - 1)
}
fn tag(node: &mut OwnedDentry, id: u32, bytes: &[u8]) {
    node.tagged_items.extend_from_slice(&id.to_le_bytes());
    node.tagged_items
        .extend_from_slice(&(bytes.len() as u32).to_le_bytes());
    node.tagged_items.extend_from_slice(bytes);
    node.tagged_items
        .resize(node.tagged_items.len().next_multiple_of(8), 0);
}
fn folded(name: &[u8]) -> Vec<u16> {
    name.chunks_exact(2)
        .map(|unit| wim_format::ntfs_upcase::uppercase(u16::from_le_bytes([unit[0], unit[1]])))
        .collect()
}
fn win_name(name: &[u8]) -> bool {
    let units: Vec<_> = name
        .chunks_exact(2)
        .map(|unit| u16::from_le_bytes([unit[0], unit[1]]))
        .collect();
    if units.is_empty()
        || units
            .iter()
            .any(|&unit| !primitives::valid_windows_char(unit))
    {
        return false;
    }
    let base: Vec<_> = folded(name)
        .into_iter()
        .take_while(|&unit| unit != 46)
        .collect();
    ![
        "CON", "PRN", "AUX", "NUL", "COM1", "COM2", "COM3", "COM4", "COM5", "COM6", "COM7", "COM8",
        "COM9", "LPT1", "LPT2", "LPT3", "LPT4", "LPT5", "LPT6", "LPT7", "LPT8", "LPT9",
    ]
    .iter()
    .any(|name| base.iter().copied().eq(name.encode_utf16()))
}
impl Context {
    fn blob(&mut self, bytes: Vec<u8>) -> [u8; 20] {
        if bytes.is_empty() {
            return [0; 20];
        }
        let hash: [u8; 20] = Sha1::digest(&bytes).into();
        if let std::collections::btree_map::Entry::Vacant(entry) = self.image.blobs.entry(hash) {
            entry.insert(bytes);
            self.image.blob_order.push(hash);
        }
        hash
    }
    fn data_hash(&mut self) -> Result<[u8; 20], ParseError> {
        let size = primitives::stream_size(&mut self.random, self.metadata_only);
        if size == 0 {
            return Ok([0; 20]);
        }
        let data = primitives::data(&mut self.random, size)?;
        Ok(self.blob(data))
    }
    fn streams(&mut self, node: &mut OwnedDentry) -> Result<(), ParseError> {
        let mut symlink = false;
        if node.attributes & 0x400 != 0 {
            let (data, reparse_tag, reserved) = if self.random.next_bool() {
                symlink = true;
                let target = primitives::filename(&mut self.random, 255)?;
                let size = target.len() as u16;
                let mut data = Vec::new();
                data.extend_from_slice(&0u16.to_le_bytes());
                data.extend_from_slice(&size.to_le_bytes());
                data.extend_from_slice(&(size + 2).to_le_bytes());
                data.extend_from_slice(&size.to_le_bytes());
                data.extend_from_slice(&1u32.to_le_bytes());
                data.extend_from_slice(&target);
                data.extend_from_slice(&[0, 0]);
                data.extend_from_slice(&target);
                data.extend_from_slice(&[0, 0]);
                (data, 0xa000000cu32, 0u16)
            } else {
                let size = primitives::stream_size(&mut self.random, self.metadata_only) % 16376;
                let mut data = primitives::data(&mut self.random, size)?;
                let reparse_tag = if size >= 16 && self.random.next_bool() {
                    data[6] = (data[6] & 15) | 64;
                    data[8] = (data[8] & 63) | 128;
                    0x100
                } else {
                    0x80000000
                };
                (data, reparse_tag, self.random.next_u16())
            };
            node.inode_union = u64::from(reparse_tag) | (u64::from(reserved) << 48);
            node.main_hash = self.blob(data);
        }
        if node.attributes & 0x10 == 0 && !symlink {
            let hash = self.data_hash()?;
            if node.attributes & 0x400 == 0 {
                node.main_hash = hash;
            } else {
                node.extra_streams.push(OwnedStream {
                    hash,
                    ..OwnedStream::default()
                });
            }
        }
        let count = self.random.next_u32() % 256;
        if count > 230 {
            for index in 0..count - 230 {
                let hash = self.data_hash()?;
                node.extra_streams.push(OwnedStream {
                    hash,
                    name: (97u16 + index as u16).to_le_bytes().to_vec(),
                    ..OwnedStream::default()
                });
            }
        }
        Ok(())
    }
    fn metadata(&mut self, node: &mut OwnedDentry) -> Result<(), ParseError> {
        node.attributes |= self.random.next_u32() & (1 | 2 | 4 | 0x20 | 0x2000 | 0x800 | 0x200);
        node.creation_time = primitives::timestamp(&mut self.random);
        node.last_access_time = primitives::timestamp(&mut self.random);
        node.last_write_time = primitives::timestamp(&mut self.random);
        if self.random.next_bool() {
            let security = primitives::security_descriptor(&mut self.random)?;
            node.security_id = self
                .image
                .tree
                .security_descriptors
                .iter()
                .position(|bytes| bytes == &security)
                .unwrap_or_else(|| {
                    self.image.tree.security_descriptors.push(security);
                    self.image.tree.security_descriptors.len() - 1
                }) as u32;
        }
        if self.random.next_u32().is_multiple_of(32) {
            let bytes: Vec<_> = (0..64).map(|_| self.random.next_u8()).collect();
            tag(node, 1, &bytes);
        }
        let symlink = node.attributes & 0x400 != 0 && node.inode_union as u32 == 0xa000000c;
        let mut unix_mode = None;
        if self.random.next_u32().is_multiple_of(16) {
            #[cfg(unix)]
            let (uid, gid, root) = {
                // SAFETY: These process identity queries have no pointer inputs.
                let (uid, gid) = unsafe { (libc::getuid(), libc::getgid()) };
                (
                    if uid == 0 {
                        self.random.next_u32()
                    } else {
                        uid
                    },
                    if uid == 0 {
                        self.random.next_u32()
                    } else {
                        gid
                    },
                    uid == 0,
                )
            };
            #[cfg(not(unix))]
            let (uid, gid, root) = (0, 0, false);
            let unnamed = if node.attributes & 0x400 != 0 {
                node.extra_streams
                    .first()
                    .map_or([0; 20], |stream| stream.hash)
            } else {
                node.main_hash
            };
            let mode = if symlink {
                0o120000 | 0o777
            } else if node.attributes & 0x10 != 0 {
                0o40000 | 0o700 | (self.random.next_u32() % 0o7777)
            } else if unnamed == [0; 20] && self.random.next_bool() && root {
                let permissions = self.random.next_u32() % 0o7777;
                permissions
                    | [0o10000, 0o20000, 0o60000, 0o140000][self.random.next_u32() as usize % 4]
            } else {
                0o100000 | 0o400 | (self.random.next_u32() % 0o7777)
            };
            unix_mode = Some(mode);
            let mut bytes = Vec::new();
            for value in [uid, gid, mode, 0] {
                bytes.extend_from_slice(&value.to_le_bytes());
            }
            tag(node, 0x337dd873, &bytes);
        }
        if self.random.next_u32().is_multiple_of(32) {
            let count = 1 + self.random.next_u32() % 16;
            #[cfg(unix)]
            // SAFETY: Process identity query has no pointer inputs.
            let root = unsafe { libc::getuid() == 0 };
            #[cfg(not(unix))]
            let root = false;
            let special = symlink
                || unix_mode.is_some_and(|mode| ![0o100000, 0o40000].contains(&(mode & 0o170000)));
            if special && !root {
                return Ok(());
            }
            #[cfg(unix)]
            let prefix = if special {
                b"trusted.".as_slice()
            } else {
                b"user.".as_slice()
            };
            #[cfg(not(unix))]
            let prefix = b"".as_slice();
            let mut bytes = Vec::new();
            let mut capability = false;
            for index in 0..count {
                let value_len = self.random.next_u32() % 64;
                #[cfg(windows)]
                let value_len = value_len.max(1);
                let name = if self.random.next_u32().is_multiple_of(16) && root && !capability {
                    capability = true;
                    b"security.capability".to_vec()
                } else {
                    let len = 1 + self.random.next_u32() % 64;
                    let mut name = prefix.to_vec();
                    name.push(b'A' + index as u8);
                    for _ in 1..len {
                        loop {
                            let byte = self.random.next_u8();
                            #[cfg(windows)]
                            let byte = b'A' + byte % 26;
                            if byte != 0 {
                                name.push(byte);
                                break;
                            }
                        }
                    }
                    name
                };
                bytes.extend_from_slice(&(value_len as u16).to_le_bytes());
                bytes.push(name.len() as u8);
                bytes.push(0);
                bytes.extend_from_slice(&name);
                bytes.push(0);
                for _ in 0..value_len {
                    bytes.push(self.random.next_u8());
                }
            }
            tag(node, 2, &bytes);
        }
        Ok(())
    }
    fn inode_number(&mut self) -> u64 {
        let bucket = self.random.next_u32() as usize % self.buckets.len();
        for &(ino, _) in &self.buckets[bucket] {
            if self.random.next_bool() {
                return ino;
            }
        }
        u64::from(self.random.next_u32())
    }
    fn add_inode(&mut self, ino: u64, index: usize) -> Option<usize> {
        if ino == 0 {
            return None;
        }
        let bucket = bucket(ino, self.buckets.len());
        if let Some(&(_, alias)) = self.buckets[bucket]
            .iter()
            .find(|&&(existing, _)| existing == ino)
        {
            return Some(alias);
        }
        self.buckets[bucket].insert(0, (ino, index));
        self.filled += 1;
        if self.filled > self.buckets.len() {
            let mut buckets = vec![Vec::new(); self.buckets.len() * 2];
            for chain in &self.buckets {
                for &(ino, index) in chain {
                    let slot = bucket_index(ino, buckets.len());
                    buckets[slot].insert(0, (ino, index));
                }
            }
            self.buckets = buckets;
        }
        None
    }
    fn children(&mut self, parent: usize, depth: u32) -> Result<(), ParseError> {
        let b = 1.01230f64;
        let draw = self.random.next_u32() % 500;
        let count = ((b.powf(b.powf(f64::from(draw))) - 1.0) / f64::from(depth).powf(1.5)
            + (2.0 - (0.04 / f64::from(depth)).exp())) as u32;
        self.short_names.clear();
        for _ in 0..count {
            let directory = self.random.next_u32() % 16 <= 6;
            let reparse = self.random.next_u32().is_multiple_of(8);
            let ino = if directory || reparse {
                0
            } else {
                self.inode_number()
            };
            let index = self.image.tree.nodes.len();
            let alias = self.add_inode(ino, index);
            let name = loop {
                let name = primitives::filename(&mut self.random, 63)?;
                let duplicate = self.image.tree.nodes[parent].children.iter().any(|&child| {
                    #[cfg(windows)]
                    {
                        folded(&self.image.tree.nodes[child].name) == folded(&name)
                    }
                    #[cfg(not(windows))]
                    {
                        self.image.tree.nodes[child].name == name
                    }
                });
                if !duplicate {
                    break name;
                }
            };
            let mut node = if let Some(alias) = alias {
                let mut node = self.image.tree.nodes[alias].clone();
                node.name = name;
                node.short_name.clear();
                node.children.clear();
                node
            } else {
                OwnedDentry::new(
                    name,
                    if directory { 0x10 } else { 0 } | if reparse { 0x400 } else { 0 },
                )
            };
            self.aliases.push(alias.unwrap_or(index));
            if alias.is_none() {
                if !reparse {
                    node.inode_union = index as u64 + 1;
                }
                self.streams(&mut node)?;
                self.metadata(&mut node)?;
            }
            self.image.tree.nodes.push(node);
            self.image.tree.nodes[parent].children.push(index);
            if alias.is_none() && directory && !reparse {
                self.children(index, depth + 1)?;
            }
        }
        let mut children = self.image.tree.nodes[parent].children.clone();
        children.sort_by(|&a, &b| {
            let a = &self.image.tree.nodes[a].name;
            let b = &self.image.tree.nodes[b].name;
            folded(a).cmp(&folded(b)).then_with(|| {
                a.chunks_exact(2)
                    .map(|unit| u16::from_le_bytes([unit[0], unit[1]]))
                    .cmp(
                        b.chunks_exact(2)
                            .map(|unit| u16::from_le_bytes([unit[0], unit[1]])),
                    )
            })
        });
        for child in children {
            let select = self.random.next_bool();
            let inode = self.aliases[child];
            if !select
                || self.aliases.iter().enumerate().any(|(node, &alias)| {
                    alias == inode && !self.image.tree.nodes[node].short_name.is_empty()
                })
                || !win_name(&self.image.tree.nodes[child].name)
            {
                continue;
            }
            let short = loop {
                let short = primitives::short_name(&mut self.random)?;
                if self.image.tree.nodes[parent]
                    .children
                    .iter()
                    .any(|&other| folded(&self.image.tree.nodes[other].name) == folded(&short))
                    || self
                        .short_names
                        .iter()
                        .any(|used| folded(used) == folded(&short))
                    || !win_name(&short)
                {
                    continue;
                }
                break short;
            };
            self.short_names.push(short.clone());
            self.image.tree.nodes[child].short_name = short;
        }
        Ok(())
    }
}
fn bucket_index(ino: u64, capacity: usize) -> usize {
    bucket(ino, capacity)
}

/// Generate the entire tree using the chosen original LCG state.
/// Payloads are fully owned and hashed immediately; there are no source paths.
/// Temporary graph allocations currently use the Rust allocator.
pub fn image(mut random: Random) -> Result<GeneratedImage, ParseError> {
    let metadata_only = !random.next_u32().is_multiple_of(8);
    let mut context = Context {
        random,
        metadata_only,
        image: GeneratedImage {
            tree: OwnedMetadata::default(),
            blobs: BTreeMap::new(),
            blob_order: Vec::new(),
        },
        buckets: vec![Vec::new(); 64],
        filled: 0,
        aliases: vec![0],
        short_names: Vec::new(),
    };
    let mut root = OwnedDentry::new(Vec::new(), 0x10);
    root.inode_union = 1;
    context.streams(&mut root)?;
    context.metadata(&mut root)?;
    context.image.tree.nodes.push(root);
    context.children(0, 1)?;
    // Hardlinked aliases share final inode fields, while their names remain
    // independent. Short names are dentry fields and must not be propagated.
    for index in 1..context.aliases.len() {
        let alias = context.aliases[index];
        if alias != index {
            let mut node = context.image.tree.nodes[alias].clone();
            node.name = std::mem::take(&mut context.image.tree.nodes[index].name);
            node.short_name = std::mem::take(&mut context.image.tree.nodes[index].short_name);
            context.image.tree.nodes[index] = node;
        }
    }
    // Source write_dentry_streams emits all unnamed streams in extras when
    // multiple streams or named streams exist, including implicit empty data.
    for node in &mut context.image.tree.nodes {
        let mut unnamed = Vec::new();
        if node.attributes & 0x400 != 0 {
            unnamed.push(node.main_hash);
        }
        if node.attributes & 0x10 == 0 {
            unnamed.push(if node.attributes & 0x400 == 0 {
                node.main_hash
            } else {
                node.extra_streams
                    .iter()
                    .find(|stream| stream.name.is_empty())
                    .map_or([0; 20], |stream| stream.hash)
            });
        }
        let mut named: Vec<_> = node
            .extra_streams
            .drain(..)
            .filter(|stream| !stream.name.is_empty())
            .collect();
        if unnamed.len() <= 1 && named.is_empty() {
            node.main_hash = unnamed.first().copied().unwrap_or([0; 20]);
        } else {
            node.main_hash = [0; 20];
            node.extra_streams = unnamed
                .into_iter()
                .map(|hash| OwnedStream {
                    hash,
                    ..OwnedStream::default()
                })
                .collect();
            node.extra_streams.append(&mut named);
        }
    }
    Ok(context.image)
}

/// Complete generated data staged with actual shared retained ownership.
/// The caller must commit in `blobs` order to preserve the original hash index.
pub struct RetainedImage {
    /// Serialized authoritative tree, ready for a pending image.
    pub metadata: Vec<u8>,
    /// Memory descriptors in original first-hash insertion order.
    pub blobs: Vec<([u8; 20], crate::engine::handles::OwnedBlob)>,
}
/// Prepare immutable Memory resources before changing the destination handle.
pub fn retain(image: GeneratedImage) -> Result<RetainedImage, ParseError> {
    use crate::engine::handles::{OwnedBlob, OwnedBlobData, OwnedResource};
    use wim_format::lookup::{LookupBlob, LookupResource};
    let metadata = image.tree.encode()?;
    let parsed = wim_format::metadata::Metadata::parse(&metadata)?;
    let mut references = BTreeMap::<[u8; 20], u32>::new();
    for node in 0..parsed.nodes.len() {
        let entry = parsed
            .inode_entry(node)
            .ok_or(ParseError::InvalidMetadataResource)?;
        for stream in &entry.streams {
            if stream.hash != [0; 20] {
                *references.entry(stream.hash).or_default() += 1;
            }
        }
    }
    drop(parsed);
    let mut blobs = Vec::new();
    blobs
        .try_reserve_exact(image.blob_order.len())
        .map_err(|_| ParseError::Nomem)?;
    for hash in image.blob_order {
        let bytes = image.blobs.get(&hash).ok_or(ParseError::ResourceNotFound)?;
        let size = bytes.len() as u64;
        blobs.push((
            hash,
            OwnedBlob {
                bytes: OwnedBlobData::memory(bytes)?,
                backing: None,
                captured: None,
                descriptor: OwnedResource {
                    blob: LookupBlob {
                        hash,
                        resource_index: 0,
                        offset: 0,
                        size,
                        reference_count: references.get(&hash).copied().unwrap_or(0),
                        flags: 0,
                    },
                    resource: LookupResource {
                        header: wim_format::ResourceHeader::default(),
                        uncompressed_size: size,
                        compression_code: 0,
                        chunk_size: 0,
                        solid: false,
                    },
                    part: 0,
                },
            },
        ));
    }
    Ok(RetainedImage { metadata, blobs })
}

/// Capture the original optional generated tree without interpreting its ignored
/// source pointer as a pathname. Supported callbacks retain the scan snapshot.
/// # Safety
/// Handle/name/config meet the public add-image contract. The source pointer is
/// passed opaquely to scan callbacks, as in the original test-only backend.
pub(crate) unsafe fn add_image(
    handle: *mut crate::engine::WimHandle,
    source: *const crate::engine::TChar,
    name: *const crate::engine::TChar,
    config: *const crate::engine::TChar,
    flags: i32,
) -> i32 {
    use crate::engine::blob_index::BlobOwner;
    use crate::engine::{ProgressInfo, capture::ScanProgress, handles};
    if handle.is_null() {
        return 24;
    }
    let mut image_index = 0;
    // SAFETY: Caller-supplied handle/name contract, writable local index.
    let status = unsafe { crate::engine::wimlib_add_empty_image(handle, name, &mut image_index) };
    if status != 0 {
        return status;
    }
    let result = (|| -> Result<(), ParseError> {
        // SAFETY: Handle is live, no callback or external access during validation.
        let normalized = unsafe {
            crate::engine::capture::normalize_flags(
                &(*handle).header,
                true,
                flags & !(0x08000000 | 8),
            )
        }?;
        if normalized & 0x800 != 0 && !config.is_null() {
            return Err(ParseError::InvalidParam);
        }
        if !config.is_null() {
            // Source get_capture_config validates actual configuration before
            // SCAN_BEGIN; the generated backend deliberately ignores filters.
            let translated = unsafe { crate::engine::text_file::load_capture_text(config) }
                .map_err(|code| match ParseError::from_i32(code) {
                    Some(ParseError::InvalidUtf8String | ParseError::InvalidUtf16String) => {
                        ParseError::InvalidCaptureConfig
                    }
                    Some(
                        ParseError::Open | ParseError::Stat | ParseError::Nomem | ParseError::Read,
                    ) => ParseError::UnableToReadCaptureConfig,
                    Some(error) => error,
                    None => ParseError::InvalidParam,
                })?;
            #[cfg(unix)]
            let bytes: Vec<_> = translated.into_iter().map(|unit| unit as u8).collect();
            #[cfg(windows)]
            let bytes = wim_format::platform_text::utf16_to_wtf8(&translated)?;
            crate::engine::capture::CaptureConfig::parse_text(&bytes)?;
            if normalized & 0x1000 != 0 {
                return Err(ParseError::Unsupported);
            }
        }
        // SAFETY: Copy borrowed registration; no handle reference crosses callbacks.
        let registration = unsafe { (*handle).progress.get() };
        let target = [47 as crate::engine::TChar, 0];
        let mut info = ProgressInfo {
            scan: ScanProgress {
                source,
                current_path: std::ptr::null(),
                status: 0,
                target: target.as_ptr(),
                directories: 0,
                nondirectories: 0,
                bytes: 0,
            },
        };
        // SAFETY: Real scan begins before generation; payload/source is caller-borrowed.
        unsafe { registration.call(9, &mut info) }?;
        let staged = retain(image(Random::Global)?)?;
        // Prepare rollback/storage before installing descriptors visible at SCAN_END.
        // SAFETY: No callback is active; a fresh exclusive borrow is valid.
        let h = unsafe { &mut *handle };
        h.owned_blobs
            .try_reserve(staged.blobs.len())
            .map_err(|_| ParseError::Nomem)?;
        let previous_index = h.blob_index.try_clone()?;
        let staged_index = h.blob_index.try_clone()?;
        h.dirty_images
            .try_reserve(1)
            .map_err(|_| ParseError::Nomem)?;
        for (hash, _) in &staged.blobs {
            if !h.owned_blobs.contains_key(hash)
                && h.lookup
                    .as_ref()
                    .is_none_or(|table| table.find(hash).is_none())
            {
                staged_index.insert(*hash, BlobOwner::Owned)?;
            }
        }
        let mut previous_owned = Vec::new();
        let mut previous_stored = Vec::new();
        previous_owned
            .try_reserve_exact(staged.blobs.len())
            .map_err(|_| ParseError::Nomem)?;
        previous_stored
            .try_reserve_exact(staged.blobs.len())
            .map_err(|_| ParseError::Nomem)?;
        for (hash, blob) in staged.blobs {
            if let Some(existing) = h.owned_blobs.get_mut(&hash) {
                previous_owned.push((hash, Some(existing.descriptor.blob.reference_count)));
                existing.descriptor.blob.reference_count = existing
                    .descriptor
                    .blob
                    .reference_count
                    .saturating_add(blob.descriptor.blob.reference_count);
            } else if let Some(index) = h
                .lookup
                .as_ref()
                .and_then(|table| table.blobs.iter().position(|blob| blob.hash == hash))
            {
                let existing =
                    &mut h.lookup.as_mut().ok_or(ParseError::ResourceNotFound)?.blobs[index];
                previous_stored.push((index, existing.reference_count));
                existing.reference_count = existing
                    .reference_count
                    .saturating_add(blob.descriptor.blob.reference_count);
            } else {
                h.owned_blobs
                    .try_insert_reserved(hash, blob)
                    .map_err(|_| ParseError::Nomem)?;
                h.removed_blobs.remove(&hash);
                previous_owned.push((hash, None));
            }
        }
        h.blob_index = staged_index;
        // End immutable staging/handle borrow before the reentrant callback.
        let end = unsafe { registration.call(11, &mut info) };
        if let Err(error) = end {
            // SAFETY: Callback ended; fresh exclusive access for rollback.
            let h = unsafe { &mut *handle };
            for (hash, old) in previous_owned {
                if let Some(count) = old {
                    if let Some(blob) = h.owned_blobs.get_mut(&hash) {
                        blob.descriptor.blob.reference_count = count;
                    }
                } else {
                    h.owned_blobs.remove(&hash);
                }
            }
            if let Some(table) = &mut h.lookup {
                for (index, count) in previous_stored {
                    table.blobs[index].reference_count = count;
                }
            }
            h.blob_index = previous_index;
            return Err(error);
        }
        // SAFETY: No callback is active; publish the tree after SCAN_END.
        let h = unsafe { &mut *handle };
        let image = h
            .images
            .get(image_index as usize - 1)
            .ok_or(ParseError::InvalidImage)?;
        let identity = handles::image_identity(h, image);
        let shared = handles::pending_metadata(image).ok_or(ParseError::InvalidMetadataResource)?;
        let mut pending = shared.lock().map_err(|_| ParseError::InvalidParam)?;
        pending.metadata = staged.metadata;
        pending.capture = None;
        pending.hash = [0; 20];
        drop(pending);
        h.dirty_images
            .try_insert(identity)
            .map_err(|_| ParseError::Nomem)?;
        if flags & 8 != 0 {
            h.header.boot_index = image_index as u32;
        }
        if normalized & 0x1000 != 0 {
            h.xml
                .set_property_bytes(image_index, b"WIMBOOT", Some(b"1"))?;
        }
        if normalized & 0x100 != 0 {
            h.header.flags |= 0x80;
        }
        Ok(())
    })();
    match result {
        Ok(()) => 0,
        Err(error) => {
            // SAFETY: Failed image is solely owned; resource decrements follow its graph.
            unsafe { crate::engine::wimlib_delete_image(handle, image_index) };
            error as i32
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use wim_format::{archive::Archive, metadata::Metadata};
    #[test]
    #[ignore = "requires independently original-generated seed0..7 WIMs"]
    fn generated_graphs_and_payloads_match_original_seed_images() {
        let folder =
            std::env::var_os("WIM_GENERATOR_ORIGINAL_DIR").expect("original seed fixture folder");
        let count = std::env::var("WIM_GENERATOR_SEEDS").map_or(8, |value| value.parse().unwrap());
        for seed in 0..count {
            let original_bytes =
                std::fs::read(std::path::Path::new(&folder).join(format!("seed{seed}.wim")))
                    .unwrap();
            let original = Archive::open(&original_bytes).unwrap();
            let expected_bytes = original.read_metadata(1).unwrap();
            let expected = Metadata::parse(&expected_bytes).unwrap();
            let generated = image(Random::Local(seed)).unwrap();
            let actual_bytes = generated.tree.encode().unwrap();
            let actual = Metadata::parse(&actual_bytes).unwrap();

            assert_eq!(
                expected.nodes.len(),
                actual.nodes.len(),
                "seed{seed} node count"
            );
            for i in 0..expected.nodes.len() {
                let a = &expected.nodes[i].entry;
                let b = &actual.nodes[i].entry;
                if a.name != b.name
                    || a.short_name != b.short_name
                    || !super::super::inode_equal(a, b, 0)
                    || expected.security_descriptor(i) != actual.security_descriptor(i)
                    || super::super::xattrs(a).unwrap() != super::super::xattrs(b).unwrap()
                {
                    eprintln!("seed{seed} entry{i} expected {a:?} actual {b:?}");
                    break;
                }
            }
            assert!(
                super::super::compare(&expected, &actual, 0).unwrap(),
                "seed{seed} graph mismatch"
            );
            for (hash, bytes) in &generated.blobs {
                assert_eq!(
                    &original.read_blob(hash).unwrap(),
                    bytes,
                    "seed{seed} payload {hash:?}"
                );
            }
            assert_eq!(
                original.lookup.blobs.len(),
                generated.blobs.len(),
                "seed{seed} data resource count"
            );
        }
    }
}
