// SPDX-License-Identifier: LGPL-2.1-or-later
//! Linux apply backend. All image-derived paths are traversed relative to open

//! directory descriptors; symlinks are never used as extraction directories.
use super::blob::{BlobSource, blob_source, chunk_end, read_chunk};
use crate::engine::collections::FallibleCollections as _;
use crate::engine::collections::FallibleMap as _;
use crate::engine::collections::FallibleSet as _;
use crate::engine::{
    handles::WimHandle,
    progress::{ExtractProgress, ProgressInfo, ProgressRegistration, next_progress},
};
use sha1::{Digest, Sha1};
use std::{
    ffi::{CStr, CString},
    os::{
        fd::{AsRawFd, FromRawFd, OwnedFd, RawFd},
        unix::{ffi::OsStrExt, fs::FileExt},
    },
    path::Path,
};
use wim_format::{
    ParseError,
    metadata::{Dentry, Metadata, StreamType},
    platform_text::{utf16_to_wtf8, utf16le_to_wtf8z},
};

use hashbrown::HashMap;
use hashbrown::HashSet;
use std::vec::Vec;

struct RetainedName(Vec<u8>);
impl RetainedName {
    fn new(bytes: &[u8]) -> Result<Self, ParseError> {
        if bytes.contains(&0) {
            return Err(ParseError::InvalidMetadataResource);
        }
        let mut text = Vec::new();
        text.try_extend_from_slice(bytes)
            .map_err(|_| ParseError::Nomem)?;
        text.try_push(0).map_err(|_| ParseError::Nomem)?;
        Ok(Self(text))
    }
}
impl std::ops::Deref for RetainedName {
    type Target = CStr;
    fn deref(&self) -> &CStr {
        // SAFETY: Constructor stores exactly one trailing NUL and no interior NUL.
        unsafe { CStr::from_bytes_with_nul_unchecked(&self.0) }
    }
}

fn u16_at(b: &[u8], p: usize) -> u16 {
    u16::from_le_bytes([b[p], b[p + 1]])
}
fn u32_at(b: &[u8], p: usize) -> u32 {
    u32::from_le_bytes([b[p], b[p + 1], b[p + 2], b[p + 3]])
}

fn cstring(bytes: &[u8]) -> Result<CString, ParseError> {
    CString::new(bytes).map_err(|_| ParseError::InvalidMetadataResource)
}
fn name(entry: &Dentry<'_>) -> Result<RetainedName, ParseError> {
    let bytes = utf16le_to_wtf8z(entry.name)?;
    let text =
        CStr::from_bytes_with_nul(&bytes).map_err(|_| ParseError::InvalidMetadataResource)?;
    if text.to_bytes().is_empty()
        || text.to_bytes() == b"."
        || text.to_bytes() == b".."
        || text.to_bytes().contains(&b'/')
    {
        return Err(ParseError::InvalidMetadataResource);
    }
    Ok(RetainedName(bytes))
}
fn opened(fd: RawFd, error: ParseError) -> Result<OwnedFd, ParseError> {
    if fd < 0 {
        Err(error)
    } else {
        // SAFETY: A successful libc open returns a newly owned descriptor.
        Ok(unsafe { OwnedFd::from_raw_fd(fd) })
    }
}
fn open_directory(parent: RawFd, name: &CStr) -> Result<OwnedFd, ParseError> {
    // SAFETY: The name is terminated and parent is an open directory.
    let fd = unsafe {
        libc::openat(
            parent,
            name.as_ptr(),
            libc::O_RDONLY | libc::O_DIRECTORY | libc::O_NOFOLLOW | libc::O_CLOEXEC,
        )
    };
    opened(fd, ParseError::Mkdir)
}
fn create_directory(parent: RawFd, name: &CStr) -> Result<OwnedFd, ParseError> {
    // SAFETY: Name and parent remain live during the syscall.
    if unsafe { libc::mkdirat(parent, name.as_ptr(), 0o755) } != 0 {
        if std::io::Error::last_os_error().raw_os_error() != Some(libc::EEXIST) {
            return Err(ParseError::Mkdir);
        }
        // SAFETY: fstatat initializes this stat record and does not follow the named leaf.
        let mut stat = unsafe { std::mem::zeroed::<libc::stat>() };
        // SAFETY: Parent and terminated name are live; stat is writable.
        if unsafe { libc::fstatat(parent, name.as_ptr(), &mut stat, libc::AT_SYMLINK_NOFOLLOW) }
            != 0
            || stat.st_mode & libc::S_IFMT != libc::S_IFDIR
        {
            return Err(ParseError::Mkdir);
        }
    }
    open_directory(parent, name)
}
fn unlink(parent: RawFd, name: &CStr) -> bool {
    // SAFETY: This removes only a named leaf in the already opened directory.
    unsafe { libc::unlinkat(parent, name.as_ptr(), 0) == 0 }
}
fn create_file(parent: RawFd, name: &CStr) -> Result<OwnedFd, ParseError> {
    let flags = libc::O_WRONLY | libc::O_CREAT | libc::O_EXCL | libc::O_NOFOLLOW | libc::O_CLOEXEC;
    // SAFETY: Parent/name are valid; O_EXCL never follows an existing leaf.
    let mut fd = unsafe { libc::openat(parent, name.as_ptr(), flags, 0o644) };
    if fd < 0
        && std::io::Error::last_os_error().raw_os_error() == Some(libc::EEXIST)
        && unlink(parent, name)
    {
        // SAFETY: Same descriptor-relative creation after removing the leaf.
        fd = unsafe { libc::openat(parent, name.as_ptr(), flags, 0o644) };
    }
    opened(fd, ParseError::Open)
}

struct Progress {
    registration: ProgressRegistration,
    info: ProgressInfo,
    next: u64,
}
impl Progress {
    fn call(&mut self, event: i32) -> Result<(), ParseError> {
        // SAFETY: This extraction context owns live strings and the complete union.
        unsafe { self.registration.call(event, &mut self.info) }
    }
    fn data(&mut self, bytes: u64, streams: u64) -> Result<(), ParseError> {
        // SAFETY: This union always contains extract.
        let p = unsafe { &mut self.info.extract };
        p.completed_bytes += bytes;
        p.completed_streams += streams;
        if p.completed_bytes >= self.next {
            self.call(4)?;
            // SAFETY: Registered callbacks may inspect but not change payload fields.
            let p = unsafe { self.info.extract };
            self.next = next_progress(p.completed_bytes, p.total_bytes, self.next);
        }
        Ok(())
    }
    fn phase(&mut self, event: i32, current: u64, end: u64) -> Result<(), ParseError> {
        // SAFETY: This union always contains extract.
        let p = unsafe { &mut self.info.extract };
        p.current_file_count = current;
        p.end_file_count = end;
        self.call(event)
    }
}

fn unix_data(entry: &Dentry<'_>) -> Option<(u32, u32, u32, u32)> {
    let b = entry.tagged_item(0x337d_d873, 16)?;
    Some((u32_at(b, 0), u32_at(b, 4), u32_at(b, 8), u32_at(b, 12)))
}
fn symlink(entry: &Dentry<'_>) -> bool {
    matches!(
        entry.reparse_fields(),
        Some((0xa000_000c | 0xa000_0003, _, _))
    )
}
fn timespec(ticks: u64) -> libc::timespec {
    let ticks = i128::from(ticks) - 116_444_736_000_000_000i128;
    libc::timespec {
        tv_sec: ticks.div_euclid(10_000_000) as libc::time_t,
        tv_nsec: (ticks.rem_euclid(10_000_000) * 100) as libc::c_long,
    }
}
fn metadata(
    entry: &Dentry<'_>,
    fd: Option<RawFd>,
    parent: RawFd,
    name: &CStr,
    flags: u32,
) -> Result<(), ParseError> {
    let path = cstring(
        format!("/proc/self/fd/{parent}/")
            .as_bytes()
            .iter()
            .copied()
            .chain(name.to_bytes().iter().copied())
            .collect::<Vec<_>>()
            .as_slice(),
    )?;
    if flags & 0x20 != 0 {
        if let Some((uid, gid, _, _)) = unix_data(entry) {
            // SAFETY: Descriptor or anchored path identifies this leaf without following symlinks.
            let result = unsafe {
                if let Some(fd) = fd {
                    libc::fchown(fd, uid, gid)
                } else {
                    libc::lchown(path.as_ptr(), uid, gid)
                }
            };
            if result != 0 && flags & 0x80 != 0 {
                return Err(ParseError::SetSecurity);
            }
        }
        apply_xattrs(entry, fd, &path, flags)?;
        if !symlink(entry)
            && let Some((_, _, mode, _)) = unix_data(entry)
        {
            // SAFETY: Regular/special-file metadata uses its descriptor or the anchored leaf path.
            let result = unsafe {
                if let Some(fd) = fd {
                    libc::fchmod(fd, mode)
                } else {
                    libc::chmod(path.as_ptr(), mode)
                }
            };
            if result != 0 && flags & 0x80 != 0 {
                return Err(ParseError::SetSecurity);
            }
        }
    }
    let times = [
        timespec(entry.last_access_time),
        timespec(entry.last_write_time),
    ];
    #[cfg(target_os = "linux")]
    // SAFETY: Match upstream's fallback sentinel in this thread's errno slot.
    unsafe {
        *libc::__errno_location() = libc::ENOSYS;
    }
    // SAFETY: Times has two valid elements; AT_SYMLINK_NOFOLLOW preserves symlink timestamps.
    let result = unsafe {
        if let Some(fd) = fd {
            libc::futimens(fd, times.as_ptr())
        } else {
            libc::utimensat(
                parent,
                name.as_ptr(),
                times.as_ptr(),
                libc::AT_SYMLINK_NOFOLLOW,
            )
        }
    };
    if result != 0 && flags & 0x2000 != 0 {
        return Err(ParseError::SetTimestamps);
    }
    Ok(())
}
fn apply_xattrs(
    entry: &Dentry<'_>,
    fd: Option<RawFd>,
    path: &CStr,
    flags: u32,
) -> Result<(), ParseError> {
    let (mut bytes, old) = if let Some(b) = entry.tagged_item(2, 6) {
        (b, false)
    } else if let Some(b) = entry.tagged_item(0x337d_d874, 12) {
        (b, true)
    } else {
        return Ok(());
    };
    while !bytes.is_empty() {
        let header = if old { 8 } else { 4 };
        if bytes.len() < header {
            return Err(ParseError::InvalidXattr);
        }
        let (name_len, value_len) = if old {
            (usize::from(u16_at(bytes, 0)), u32_at(bytes, 4) as usize)
        } else {
            (usize::from(bytes[2]), usize::from(u16_at(bytes, 0)))
        };
        let value_offset = header + name_len + usize::from(!old);
        let size = value_offset
            .checked_add(value_len)
            .ok_or(ParseError::InvalidXattr)?;
        let padded = if old {
            size.checked_add(3).ok_or(ParseError::InvalidXattr)? & !3
        } else {
            size
        };
        if name_len == 0
            || name_len > 255
            || value_len > 65535
            || padded > bytes.len()
            || bytes[header..header + name_len].contains(&0)
            || (!old && bytes[header + name_len] != 0)
        {
            return Err(ParseError::InvalidXattr);
        }
        let key =
            cstring(&bytes[header..header + name_len]).map_err(|_| ParseError::InvalidXattr)?;
        let value = &bytes[value_offset..size];
        // SAFETY: Keys and value buffers remain live for this syscall.
        let result = unsafe {
            if let Some(fd) = fd {
                libc::fsetxattr(fd, key.as_ptr(), value.as_ptr().cast(), value.len(), 0)
            } else {
                libc::lsetxattr(
                    path.as_ptr(),
                    key.as_ptr(),
                    value.as_ptr().cast(),
                    value.len(),
                    0,
                )
            }
        };
        let security = key.as_bytes().starts_with(b"security.")
            || matches!(
                key.as_bytes(),
                b"system.posix_acl_access" | b"system.posix_acl_default"
            );
        if result != 0 && security && flags & 0x80 != 0 {
            return Err(ParseError::SetXattr);
        }
        bytes = &bytes[padded..];
    }
    Ok(())
}

fn link_target(
    entry: &Dentry<'_>,
    blob: &[u8],
    root: &[u8],
    flags: u32,
) -> Result<CString, ParseError> {
    if blob.len() > 16376 || blob.len() < 8 {
        return Err(ParseError::Readlink);
    }
    let (tag, _, rp_flags) = entry.reparse_fields().ok_or(ParseError::Readlink)?;
    let start = if tag == 0xa000_000c { 12 } else { 8 };
    if blob.len() < start {
        return Err(ParseError::Readlink);
    }
    let offset = usize::from(u16_at(blob, 0));
    let len = usize::from(u16_at(blob, 2));
    let print_offset = usize::from(u16_at(blob, 4));
    let print_len = usize::from(u16_at(blob, 6));
    if (offset | len | print_offset | print_len) & 1 != 0
        || start + print_offset + print_len > blob.len()
    {
        return Err(ParseError::Readlink);
    }
    let b = blob
        .get(start + offset..start + offset + len)
        .ok_or(ParseError::Readlink)?;
    let units: Vec<_> = b
        .chunks_exact(2)
        .map(|p| u16::from_le_bytes([p[0], p[1]]))
        .collect();
    let mut target = utf16_to_wtf8(&units).map_err(|_| ParseError::Readlink)?;
    let relative = tag == 0xa000_000c && u32_at(blob, 8) & 1 != 0;
    if !relative {
        for prefix in [b"\\??\\".as_slice(), b"\\DosDevices\\", b"\\Device\\"] {
            if target.starts_with(prefix) {
                let mut n = prefix.len();
                while target.get(n) == Some(&b'\\') {
                    n += 1;
                }
                while target.get(n).is_some_and(|&b| b != b'\\') {
                    n += 1;
                }
                target.drain(..n);
                break;
            }
        }
    }
    for b in &mut target {
        if *b == b'\\' {
            *b = b'/';
        } else if *b == b'/' {
            *b = b'\\';
        }
    }
    if !relative && rp_flags & 1 == 0 && flags & 0x100 != 0 {
        let mut fixed = root.to_vec();
        fixed.extend_from_slice(&target);
        target = fixed;
    } else if target.is_empty() {
        target.push(b'/');
    }
    cstring(&target).map_err(|_| ParseError::Readlink)
}

struct Layout {
    selected: Vec<usize>,
    names: Vec<RetainedName>,
    groups: Vec<([u8; 20], Vec<usize>)>,
    empty: Vec<usize>,
    directories: Vec<usize>,
}
fn layout(
    tree: &Metadata<'_>,
    flags: u32,
    selection: Option<&super::paths::Selection>,
) -> Result<Layout, ParseError> {
    if selection.is_none() {
        super::paths::whole_image_root(tree)?;
    }
    let mut all_nodes = Vec::new();
    all_nodes
        .try_extend(0..tree.nodes.len())
        .map_err(|_| ParseError::Nomem)?;
    let selected = selection.map_or(all_nodes.as_slice(), |s| s.nodes.as_slice());
    if flags & 0x4000 != 0
        && selected
            .iter()
            .any(|&i| !tree.nodes[i].entry.short_name.is_empty())
    {
        return Err(ParseError::Unsupported);
    }
    if flags & (0x80 | 0x20) == 0x80
        && selected
            .iter()
            .any(|&index| tree.security_descriptor(index).is_some())
    {
        return Err(ParseError::Unsupported);
    }
    let mut names = Vec::new();
    for (index, node) in tree.nodes.iter().enumerate() {
        let text = if index == 0 {
            RetainedName::new(b".")?
        } else {
            name(&node.entry)?
        };
        names.try_push(text).map_err(|_| ParseError::Nomem)?;
    }
    let mut seen = HashSet::new();
    let mut group_indices = HashMap::new();
    let mut groups: Vec<([u8; 20], Vec<usize>)> = Vec::new();
    let mut empty = Vec::new();
    let mut directories = Vec::new();
    for &index in selected {
        let node = &tree.nodes[index];
        let inode = tree
            .inode_entry(index)
            .ok_or(ParseError::InvalidMetadataResource)?;
        if inode.is_directory() && !symlink(inode) {
            directories.try_push(index).map_err(|_| ParseError::Nomem)?;
            continue;
        }
        if !seen.try_insert(node.inode).map_err(|_| ParseError::Nomem)? {
            continue;
        }
        let stream = inode.streams.iter().find(|s| {
            if symlink(inode) {
                s.kind == StreamType::ReparsePoint
            } else {
                s.kind == StreamType::Data && s.name.is_empty()
            }
        });
        if let Some(stream) = stream.filter(|s| s.hash != [0; 20]) {
            let group = if let Some(&group) = group_indices.get(&stream.hash) {
                group
            } else {
                let next = groups.len();
                groups
                    .try_push((stream.hash, Vec::new()))
                    .map_err(|_| ParseError::Nomem)?;
                group_indices
                    .try_insert_reserved(stream.hash, next)
                    .map_err(|_| ParseError::Nomem)?;
                next
            };
            groups[group]
                .1
                .try_push(index)
                .map_err(|_| ParseError::Nomem)?;
        } else {
            empty.try_push(index).map_err(|_| ParseError::Nomem)?;
        }
    }
    let mut retained_selected = Vec::new();
    retained_selected
        .try_extend_from_slice(selected)
        .map_err(|_| ParseError::Nomem)?;
    Ok(Layout {
        selected: retained_selected,
        names,
        groups,
        empty,
        directories,
    })
}

/// Count actual selected inode consumers, including separate identical files.
pub(crate) fn required_stream_count(tree: &Metadata<'_>, flags: u32) -> Result<u64, ParseError> {
    if flags & 0x4000 != 0
        && tree
            .nodes
            .iter()
            .any(|node| !node.entry.short_name.is_empty())
    {
        return Err(ParseError::Unsupported);
    }
    if flags & (0x80 | 0x20) == 0x80
        && (0..tree.nodes.len()).any(|index| tree.security_descriptor(index).is_some())
    {
        return Err(ParseError::Unsupported);
    }
    // Canonical inode indices already deduplicate hard-link aliases. Counting
    // whole-image consumers requires no second layout/name/group allocation.
    let mut count = 0;
    for (index, node) in tree.nodes.iter().enumerate() {
        if node.inode != index {
            continue;
        }
        let inode = &node.entry;
        if inode.is_directory() && !symlink(inode) {
            continue;
        }
        if inode.streams.iter().any(|stream| {
            stream.hash != [0; 20]
                && if symlink(inode) {
                    stream.kind == StreamType::ReparsePoint
                } else {
                    stream.kind == StreamType::Data && stream.name.is_empty()
                }
        }) {
            count += 1;
        }
    }
    Ok(count)
}
/// Descriptor-anchored extraction state, independent of resource input ownership.
pub(crate) struct PreparedExtraction<'tree, 'data> {
    tree: &'tree Metadata<'data>,
    flags: u32,
    selected: Vec<usize>,
    parents: HashMap<usize, usize>,
    names: Vec<RetainedName>,
    directories: Vec<usize>,
    dirs: HashMap<usize, OwnedFd>,
    fallback: bool,
    groups: HashMap<[u8; 20], Vec<usize>>,
    root_path: Option<crate::engine::handles::RetainedPath>,
    progress: Progress,
    path_mode: bool,
    _filename: Option<Vec<crate::engine::TChar>>,
    _target: Vec<crate::engine::TChar>,
    _image_name: Vec<crate::engine::TChar>,
}
impl<'tree, 'data> PreparedExtraction<'tree, 'data> {
    /// Prepare a whole image for sequential resource input with declared totals.
    pub(crate) fn prepare_image(
        handle: &WimHandle,
        image: i32,
        target: &Path,
        flags: u32,
        tree: &'tree Metadata<'data>,
        totals: (u64, u64),
    ) -> Result<Self, ParseError> {
        Self::prepare(handle, image, target, flags, tree, None, totals)
    }

    /// Prepare target directories and empty nodes using declared stream totals.
    fn prepare(
        handle: &WimHandle,
        image: i32,
        target: &Path,
        flags: u32,
        tree: &'tree Metadata<'data>,
        selection: Option<&super::paths::Selection>,
        totals: (u64, u64),
    ) -> Result<Self, ParseError> {
        let (total_bytes, total_streams) = totals;
        let layout = layout(tree, flags, selection)?;
        let selected = layout.selected.as_slice();
        let mut names = layout.names;
        let directories = layout.directories;
        let empty = layout.empty;
        let path_text = |path: &Path| -> Result<Vec<crate::engine::TChar>, ParseError> {
            let bytes = path.as_os_str().as_bytes();
            let mut text = Vec::new();
            text.try_reserve(bytes.len().checked_add(1).ok_or(ParseError::Nomem)?)
                .map_err(|_| ParseError::Nomem)?;
            text.try_extend(bytes.iter().map(|&byte| byte as crate::engine::TChar))
                .map_err(|_| ParseError::Nomem)?;
            text.try_push(0).map_err(|_| ParseError::Nomem)?;
            Ok(text)
        };
        let filename = handle.filename.as_deref().map(path_text).transpose()?;
        let target_buffer = path_text(target)?;
        let mut image_name = Vec::new();
        image_name
            .try_extend(
                handle
                    .xml
                    .name_bytes(image)
                    .unwrap_or_default()
                    .iter()
                    .map(|&b| b as crate::engine::TChar),
            )
            .map_err(|_| ParseError::Nomem)?;
        image_name.try_push(0).map_err(|_| ParseError::Nomem)?;
        let mut info = ProgressInfo::zeroed();
        info.extract = ExtractProgress {
            image: image as u32,
            extract_flags: flags,
            wimfile_name: filename.as_ref().map_or(std::ptr::null(), |b| b.as_ptr()),
            image_name: image_name.as_ptr(),
            target: target_buffer.as_ptr(),
            reserved: std::ptr::null(),
            total_bytes,
            completed_bytes: 0,
            total_streams,
            completed_streams: 0,
            part_number: 0,
            total_parts: 0,
            guid: [0; 16],
            current_file_count: 0,
            end_file_count: 0,
        };
        let mut progress = Progress {
            registration: handle.progress.get(),
            info,
            next: 0,
        };
        progress.call(if selection.is_some() { 1 } else { 0 })?;
        let structure_count = (directories.len() + empty.len()) as u64;
        progress.phase(3, 0, structure_count)?;
        // SAFETY: Actual owned platform text is terminated and remains live.
        let created = unsafe { libc::mkdir(target_buffer.as_ptr(), 0o777) };
        if created != 0 && std::io::Error::last_os_error().raw_os_error() != Some(libc::EEXIST) {
            return Err(ParseError::Mkdir);
        }
        // SAFETY: The caller's root path is terminated; image paths never use this path again.
        let root_fd = unsafe {
            libc::open(
                target_buffer.as_ptr(),
                libc::O_RDONLY | libc::O_DIRECTORY | libc::O_CLOEXEC,
            )
        };
        // In flattened extraction the container is not an extracted directory.
        // Preserve the syscall failure at the first selected leaf, rather than
        // reporting a synthetic directory failure when the container is a file.
        let fallback = root_fd < 0
            && !directories.contains(&0)
            && std::io::Error::last_os_error().raw_os_error() == Some(libc::ENOTDIR);
        let mut dirs = HashMap::new();
        if !fallback {
            dirs.try_insert_reserved(0, opened(root_fd, ParseError::Mkdir)?)
                .map_err(|_| ParseError::Nomem)?;
        }
        let parent_of = |index: usize| {
            selection
                .and_then(|s| s.parents.get(&index).copied())
                .or(tree.nodes[index].parent)
        };
        if fallback {
            #[cfg(target_os = "linux")]
            // SAFETY: A speculative container open must not alter errno before
            // the original operation reaches the selected leaf syscall.
            unsafe {
                *libc::__errno_location() = libc::EEXIST;
            }
            for &index in selected {
                if index != 0 && parent_of(index) == Some(0) {
                    let parent = target.as_os_str().as_bytes();
                    let leaf = names[index].to_bytes();
                    let capacity = parent
                        .len()
                        .checked_add(leaf.len())
                        .and_then(|n| n.checked_add(2))
                        .ok_or(ParseError::Nomem)?;
                    let mut path = Vec::new();
                    path.try_reserve(capacity).map_err(|_| ParseError::Nomem)?;
                    path.try_extend_from_slice(parent)
                        .map_err(|_| ParseError::Nomem)?;
                    path.try_push(b'/').map_err(|_| ParseError::Nomem)?;
                    path.try_extend_from_slice(leaf)
                        .map_err(|_| ParseError::Nomem)?;
                    path.try_push(0).map_err(|_| ParseError::Nomem)?;
                    names[index] = RetainedName(path);
                }
            }
        }
        for &index in directories.iter().filter(|&&i| i != 0) {
            let parent = parent_of(index).ok_or(ParseError::InvalidMetadataResource)?;
            let fd = create_directory(
                if fallback && parent == 0 {
                    libc::AT_FDCWD
                } else {
                    dirs.get(&parent)
                        .ok_or(ParseError::InvalidMetadataResource)?
                        .as_raw_fd()
                },
                &names[index],
            )?;
            dirs.try_insert_reserved(index, fd)
                .map_err(|_| ParseError::Nomem)?;
        }
        let parent_fd = |index: usize| -> Result<RawFd, ParseError> {
            let parent = parent_of(index).unwrap_or(0);
            if fallback && parent == 0 {
                return Ok(libc::AT_FDCWD);
            }
            Ok(dirs
                .get(&parent)
                .ok_or(ParseError::InvalidMetadataResource)?
                .as_raw_fd())
        };
        let aliases = |index: usize| {
            selected
                .iter()
                .copied()
                .filter(move |&i| i != index && tree.nodes[i].inode == tree.nodes[index].inode)
        };
        let hardlinks = |index: usize| -> Result<(), ParseError> {
            for alias in aliases(index) {
                let dest = parent_fd(alias)?;
                // SAFETY: Both descriptors are anchored directories and both names are single leaves.
                let mut result = unsafe {
                    libc::linkat(
                        parent_fd(index)?,
                        names[index].as_ptr(),
                        dest,
                        names[alias].as_ptr(),
                        0,
                    )
                };
                if result != 0
                    && std::io::Error::last_os_error().raw_os_error() == Some(libc::EEXIST)
                    && unlink(dest, &names[alias])
                {
                    // SAFETY: Same descriptor-relative link after leaf removal.
                    result = unsafe {
                        libc::linkat(
                            parent_fd(index)?,
                            names[index].as_ptr(),
                            dest,
                            names[alias].as_ptr(),
                            0,
                        )
                    };
                }
                if result != 0 {
                    return Err(ParseError::Link);
                }
            }
            Ok(())
        };
        for &index in &empty {
            let inode = tree
                .inode_entry(index)
                .ok_or(ParseError::InvalidMetadataResource)?;
            let parent = parent_fd(index)?;
            if flags & 0x20 != 0
                && let Some((_, _, mode, rdev)) = unix_data(inode)
                && mode & libc::S_IFMT != libc::S_IFREG
            {
                // SAFETY: Descriptor-relative special-file creation cannot traverse leaf symlinks.
                let mut result = unsafe {
                    libc::mknodat(parent, names[index].as_ptr(), mode, rdev as libc::dev_t)
                };
                if result != 0
                    && std::io::Error::last_os_error().raw_os_error() == Some(libc::EEXIST)
                    && unlink(parent, &names[index])
                {
                    // SAFETY: Same anchored leaf after removal.
                    result = unsafe {
                        libc::mknodat(parent, names[index].as_ptr(), mode, rdev as libc::dev_t)
                    };
                }
                if result != 0 {
                    if std::io::Error::last_os_error().raw_os_error() == Some(libc::EPERM) {
                        continue;
                    }
                    return Err(ParseError::Mknod);
                }
                metadata(inode, None, parent, &names[index], flags)?;
            } else {
                let fd = create_file(parent, &names[index])?;
                metadata(inode, Some(fd.as_raw_fd()), parent, &names[index], flags)?;
            }
            hardlinks(index)?;
        }
        progress.phase(3, structure_count, structure_count)?;
        let root_path = if flags & 0x100 != 0
            && selected
                .iter()
                .any(|&i| tree.inode_entry(i).is_some_and(symlink))
        {
            Some(crate::engine::handles::RetainedPath::new(
                &std::fs::canonicalize(target).map_err(|_| ParseError::Nomem)?,
            )?)
        } else {
            None
        };
        let mut groups = HashMap::new();
        let mut layout_groups = layout.groups;
        layout_groups.reverse();
        while let Some((hash, nodes)) = layout_groups.pop() {
            groups
                .try_insert_reserved(hash, nodes)
                .map_err(|_| ParseError::Nomem)?;
        }
        let mut parents = HashMap::new();
        if let Some(selection) = selection {
            for (&node, &parent) in &selection.parents {
                parents
                    .try_insert_reserved(node, parent)
                    .map_err(|_| ParseError::Nomem)?;
            }
        }
        Ok(Self {
            tree,
            flags,
            selected: layout.selected,
            names,
            directories,
            dirs,
            fallback,
            groups,
            root_path,
            progress,
            parents,
            path_mode: selection.is_some(),
            _filename: filename,
            _target: target_buffer,
            _image_name: image_name,
        })
    }
    fn parent_fd(&self, index: usize) -> Result<RawFd, ParseError> {
        let parent = self
            .parents
            .get(&index)
            .copied()
            .or(self.tree.nodes[index].parent)
            .unwrap_or(0);
        if self.fallback && parent == 0 {
            return Ok(libc::AT_FDCWD);
        }
        Ok(self
            .dirs
            .get(&parent)
            .ok_or(ParseError::InvalidMetadataResource)?
            .as_raw_fd())
    }
    fn hardlinks(&self, index: usize) -> Result<(), ParseError> {
        for &alias in &self.selected {
            if alias == index || self.tree.nodes[alias].inode != self.tree.nodes[index].inode {
                continue;
            }
            let dest = self.parent_fd(alias)?;
            // SAFETY: Both parents are anchored descriptors and names are validated leaves.
            let mut result = unsafe {
                libc::linkat(
                    self.parent_fd(index)?,
                    self.names[index].as_ptr(),
                    dest,
                    self.names[alias].as_ptr(),
                    0,
                )
            };
            if result != 0
                && std::io::Error::last_os_error().raw_os_error() == Some(libc::EEXIST)
                && unlink(dest, &self.names[alias])
            {
                // SAFETY: Same anchored leaves after removal.
                result = unsafe {
                    libc::linkat(
                        self.parent_fd(index)?,
                        self.names[index].as_ptr(),
                        dest,
                        self.names[alias].as_ptr(),
                        0,
                    )
                };
            }
            if result != 0 {
                return Err(ParseError::Link);
            }
        }
        Ok(())
    }
    /// Return whether this image consumes an incoming data digest.
    pub(crate) fn needs_stream(&self, hash: &[u8; 20]) -> bool {
        self.groups.contains_key(hash)
    }
    /// Report the next actual pipable WIM part before consuming its resources.
    pub(crate) fn part_begin(
        &mut self,
        part: u32,
        total: u32,
        guid: [u8; 16],
    ) -> Result<(), ParseError> {
        // SAFETY: This context always initializes the extract union member.
        let info = unsafe { &mut self.progress.info.extract };
        info.part_number = part;
        info.total_parts = total;
        info.guid = guid;
        self.progress.call(5)
    }
    /// Whether all required digests have been supplied to completed stream sinks.
    pub(crate) fn all_streams_complete(&self) -> bool {
        self.remaining_streams() == 0
    }
    /// Number of selected inode consumers awaiting incoming resource content.
    pub(crate) fn remaining_streams(&self) -> u64 {
        self.groups.values().map(|nodes| nodes.len() as u64).sum()
    }
    /// Open writable leaves for one selected stream; subsequent chunks are bounded.
    pub(crate) fn begin_stream<'sink>(
        &'sink mut self,
        hash: [u8; 20],
        size: u64,
        mismatch: ParseError,
    ) -> Result<StreamSink<'sink, 'tree, 'data>, ParseError> {
        let nodes = self
            .groups
            .remove(&hash)
            .ok_or(ParseError::ResourceNotFound)?;
        let tree = self.tree;
        let names = &self.names;
        let parent_fd = |index| self.parent_fd(index);
        let hardlinks = |index| self.hardlinks(index);
        let mut files = Vec::new();
        let mut reparse = Vec::new();
        for &index in &nodes {
            if symlink(
                tree.inode_entry(index)
                    .ok_or(ParseError::InvalidMetadataResource)?,
            ) {
                reparse.try_push(index).map_err(|_| ParseError::Nomem)?;
            } else {
                let fd = create_file(parent_fd(index)?, &names[index])?;
                if tree
                    .inode_entry(index)
                    .ok_or(ParseError::InvalidMetadataResource)?
                    .attributes
                    & 0x200
                    == 0
                {
                    // SAFETY: fd is writable and the requested size was validated by the resource reader.
                    // Like upstream, preallocation failure is only a performance hint.
                    unsafe { libc::posix_fallocate(fd.as_raw_fd(), 0, size as libc::off_t) };
                }
                files
                    .try_push((index, std::fs::File::from(fd)))
                    .map_err(|_| ParseError::Nomem)?;
                hardlinks(index)?;
            }
        }
        Ok(StreamSink {
            backend: self,
            nodes,
            files,
            reparse,
            reparse_bytes: Vec::new(),
            digest: Sha1::new(),
            offset: 0,
            size,
            hash,
            mismatch,
        })
    }
    /// Apply directory metadata and finish progress after all selected streams.
    pub(crate) fn finish(mut self) -> Result<(), ParseError> {
        if !self.all_streams_complete() {
            return Err(ParseError::ResourceNotFound);
        }
        self.progress.phase(6, 0, self.directories.len() as u64)?;
        for &index in self.directories.iter().rev() {
            let fd = self
                .dirs
                .get(&index)
                .ok_or(ParseError::InvalidMetadataResource)?
                .as_raw_fd();
            metadata(
                self.tree
                    .inode_entry(index)
                    .ok_or(ParseError::InvalidMetadataResource)?,
                Some(fd),
                fd,
                &self.names[0],
                self.flags,
            )?;
        }
        self.progress.phase(
            6,
            self.directories.len() as u64,
            self.directories.len() as u64,
        )?;
        self.progress.call(if self.path_mode { 8 } else { 7 })
    }
}
/// Incremental payload writer for one digest and all selected inode consumers.
pub(crate) struct StreamSink<'sink, 'tree, 'data> {
    backend: &'sink mut PreparedExtraction<'tree, 'data>,
    nodes: Vec<usize>,
    files: Vec<(usize, std::fs::File)>,
    reparse: Vec<usize>,
    reparse_bytes: Vec<u8>,
    digest: Sha1,
    offset: u64,
    size: u64,
    hash: [u8; 20],
    mismatch: ParseError,
}
impl StreamSink<'_, '_, '_> {
    /// Consume one sequential decoded chunk without caching regular payloads.
    pub(crate) fn consume(&mut self, bytes: &[u8]) -> Result<(), ParseError> {
        let end = self
            .offset
            .checked_add(bytes.len() as u64)
            .ok_or(ParseError::InvalidResourceHash)?;
        if end > self.size {
            return Err(ParseError::InvalidResourceHash);
        }
        self.digest.update(bytes);
        self.backend.progress.data(
            bytes.len() as u64 * self.nodes.len() as u64,
            if end == self.size {
                self.nodes.len() as u64
            } else {
                0
            },
        )?;
        for (index, file) in &mut self.files {
            let sparse = self
                .backend
                .tree
                .inode_entry(*index)
                .ok_or(ParseError::InvalidMetadataResource)?
                .attributes
                & 0x200
                != 0;
            for (unit, bytes) in bytes.chunks(4096).enumerate() {
                if !sparse || bytes.iter().any(|&b| b != 0) {
                    file.write_all_at(bytes, self.offset + unit as u64 * 4096)
                        .map_err(|_| ParseError::Write)?;
                }
            }
        }
        if !self.reparse.is_empty() {
            if self.size > 16376 {
                return Err(ParseError::InvalidReparseData);
            }
            self.reparse_bytes
                .try_extend_from_slice(bytes)
                .map_err(|_| ParseError::Nomem)?;
        }
        self.offset = end;
        Ok(())
    }
    /// Validate the digest and apply regular/reparse metadata after complete input.
    pub(crate) fn finish(self) -> Result<(), ParseError> {
        if self.offset != self.size {
            return Err(ParseError::Read);
        }
        if <[u8; 20]>::from(self.digest.finalize()) != self.hash && self.backend.flags & 2 == 0 {
            return Err(self.mismatch);
        }
        let tree = self.backend.tree;
        let names = &self.backend.names;
        let flags = self.backend.flags;
        let parent_fd = |index| self.backend.parent_fd(index);
        let hardlinks = |index| self.backend.hardlinks(index);
        let root_path = &self.backend.root_path;
        let size = self.size;
        let mut files = self.files;
        files.reverse();
        let reparse = self.reparse;
        let reparse_bytes = self.reparse_bytes;
        while let Some((index, file)) = files.pop() {
            file.set_len(size).map_err(|_| ParseError::Write)?;
            metadata(
                tree.inode_entry(index)
                    .ok_or(ParseError::InvalidMetadataResource)?,
                Some(file.as_raw_fd()),
                parent_fd(index)?,
                &names[index],
                flags,
            )?;
        }
        for &index in &reparse {
            let inode = tree
                .inode_entry(index)
                .ok_or(ParseError::InvalidMetadataResource)?;
            let target = link_target(
                inode,
                &reparse_bytes,
                root_path
                    .as_ref()
                    .map_or(b"".as_slice(), |p| p.as_os_str().as_bytes()),
                flags,
            )?;
            let parent = parent_fd(index)?;
            // SAFETY: The symlink is created as a leaf beneath an anchored parent.
            let mut result =
                unsafe { libc::symlinkat(target.as_ptr(), parent, names[index].as_ptr()) };
            if result != 0
                && std::io::Error::last_os_error().raw_os_error() == Some(libc::EEXIST)
                && unlink(parent, &names[index])
            {
                // SAFETY: Same anchored leaf after removal.
                result = unsafe { libc::symlinkat(target.as_ptr(), parent, names[index].as_ptr()) };
            }
            if result != 0 {
                return Err(ParseError::Link);
            }
            metadata(inode, None, parent, &names[index], flags)?;
            hardlinks(index)?;
        }
        Ok(())
    }
}
pub(super) fn extract(
    handle: &WimHandle,
    image: i32,
    target: &Path,
    flags: u32,
    tree: &Metadata<'_>,
    selection: Option<&super::paths::Selection>,
) -> Result<(), ParseError> {
    if tree.nodes.is_empty() {
        return Ok(());
    }
    let layout = layout(tree, flags, selection)?;
    let mut blobs = Vec::new();
    let mut total_bytes = 0;
    let total_streams = if selection.is_none() {
        required_stream_count(tree, flags)?
    } else {
        layout
            .groups
            .iter()
            .map(|(_, nodes)| nodes.len() as u64)
            .sum()
    };
    let mut layout_groups = layout.groups;
    layout_groups.reverse();
    while let Some((hash, nodes)) = layout_groups.pop() {
        let (source, size, chunk, offset) = blob_source(handle, &hash)?;
        total_bytes += size * nodes.len() as u64;
        blobs.push((offset, hash, source, size, chunk));
    }
    blobs.sort_by(|a, b| {
        a.0.cmp(&b.0).then_with(|| match (&a.2, &b.2) {
            (BlobSource::Captured { stream: a, .. }, BlobSource::Captured { stream: b, .. }) => {
                match (&a.source, &b.source) {
                    (
                        crate::engine::capture::CapturedSource::File(a),
                        crate::engine::capture::CapturedSource::File(b),
                    ) => a.as_os_str().as_bytes().cmp(b.as_os_str().as_bytes()),
                    _ => std::cmp::Ordering::Equal,
                }
            }
            _ => std::cmp::Ordering::Equal,
        })
    });
    let mut backend = if selection.is_none() {
        PreparedExtraction::prepare_image(
            handle,
            image,
            target,
            flags,
            tree,
            (total_bytes, total_streams),
        )?
    } else {
        PreparedExtraction::prepare(
            handle,
            image,
            target,
            flags,
            tree,
            selection,
            (total_bytes, total_streams),
        )?
    };
    for (_, hash, source, size, chunk) in blobs {
        debug_assert!(backend.needs_stream(&hash));
        let mismatch = if matches!(&source, BlobSource::Captured {stream,..} if matches!(stream.source, crate::engine::capture::CapturedSource::File(_)))
        {
            ParseError::ConcurrentModificationDetected
        } else {
            ParseError::InvalidResourceHash
        };
        let mut sink = backend.begin_stream(hash, size, mismatch)?;
        let mut offset = 0;
        while offset < size {
            let end = chunk_end(&source, offset, size, chunk);
            let bytes = read_chunk(&source, offset, end, flags)?;
            sink.consume(&bytes)?;
            offset = end;
        }
        sink.finish()?;
    }
    backend.finish()
}

pub(super) fn extract_stdout(
    handle: &WimHandle,
    tree: &Metadata<'_>,
    nodes: &[usize],
    flags: u32,
) -> Result<(), ParseError> {
    for &node in nodes {
        let inode = tree
            .inode_entry(node)
            .ok_or(ParseError::InvalidMetadataResource)?;
        if inode.attributes & (0x10 | 0x400 | 0x4000) != 0 {
            return Err(ParseError::NotARegularFile);
        }
        let stream = inode
            .streams
            .iter()
            .find(|s| s.kind == StreamType::Data && s.name.is_empty());
        let Some(stream) = stream.filter(|s| s.hash != [0; 20]) else {
            continue;
        };
        let (source, size, chunk, _) = blob_source(handle, &stream.hash)?;
        let mut offset = 0;
        let mut digest = Sha1::new();
        while offset < size {
            let end = chunk_end(&source, offset, size, chunk);
            let bytes = read_chunk(&source, offset, end, flags)?;
            digest.update(&bytes);
            let mut written = 0;
            while written < bytes.len() {
                // SAFETY: stdout is the caller's live output descriptor; bytes remain readable.
                let count = unsafe {
                    libc::write(
                        libc::STDOUT_FILENO,
                        bytes[written..].as_ptr().cast(),
                        bytes.len() - written,
                    )
                };
                if count < 0 && std::io::Error::last_os_error().raw_os_error() == Some(libc::EINTR)
                {
                    continue;
                }
                if count <= 0 {
                    return Err(ParseError::Write);
                }
                written += count as usize;
            }
            offset = end;
        }
        if <[u8; 20]>::from(digest.finalize()) != stream.hash && flags & 2 == 0 {
            return Err(
                if matches!(source, BlobSource::Captured {stream,..} if matches!(stream.source, crate::engine::capture::CapturedSource::File(_)))
                {
                    ParseError::ConcurrentModificationDetected
                } else {
                    ParseError::InvalidResourceHash
                },
            );
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::required_stream_count;
    use wim_format::{
        metadata::Metadata,
        metadata_write::{OwnedDentry, OwnedMetadata, OwnedStream},
    };

    #[test]
    fn stream_totals_count_identical_files_once_per_inode_and_ignore_named_streams() {
        let name = |value: &str| value.encode_utf16().flat_map(u16::to_le_bytes).collect();
        let mut root = OwnedDentry::new(Vec::new(), 0x10);
        root.children = vec![1, 2, 3, 4];
        let mut file = OwnedDentry::new(name("file"), 0x80);
        file.main_hash = [3; 20];
        file.inode_union = 7;
        let mut alias = file.clone();
        alias.name = name("alias");
        let mut duplicate = file.clone();
        duplicate.name = name("duplicate");
        duplicate.inode_union = 0;
        let mut named_only = OwnedDentry::new(name("named"), 0x80);
        named_only.extra_streams.push(OwnedStream {
            hash: [4; 20],
            name: name("alternate"),
            ..OwnedStream::default()
        });
        let bytes = OwnedMetadata {
            security_descriptors: Vec::new(),
            nodes: vec![root, file, alias, duplicate, named_only],
        }
        .encode()
        .unwrap();
        let tree = Metadata::parse(&bytes).unwrap();
        // A hardlink alias shares its inode; independent duplicate content
        // consumes another output stream even though the digest is shared.
        assert_eq!(required_stream_count(&tree, 0).unwrap(), 2);
    }
}
