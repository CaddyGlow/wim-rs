// SPDX-License-Identifier: LGPL-2.1-or-later
//! Path selection, wildcard expansion, and original text path-list semantics.
use super::{PUBLIC_FLAGS, checked_flags};
use crate::engine::{
    TChar,
    handles::{WimHandle, image_metadata_bytes, path_from_pointer},
};
#[cfg(any(unix, windows))]
use std::collections::{HashMap, HashSet};
use std::ffi::c_int;
use wim_format::{
    ParseError,
    metadata::Metadata,
    platform_text::{utf16_to_wtf8, wtf8_to_utf16z},
};

#[cfg(any(unix, windows))]
pub(super) struct Selection {
    pub nodes: Vec<usize>,
    pub parents: HashMap<usize, usize>,
}
unsafe fn bytes_from_pointer(pointer: *const TChar) -> Result<Vec<u8>, ParseError> {
    if pointer.is_null() {
        return Ok(Vec::new());
    }
    #[cfg(not(windows))]
    {
        // SAFETY: The API caller guarantees a readable terminated string.
        Ok(unsafe { std::ffi::CStr::from_ptr(pointer) }
            .to_bytes()
            .to_vec())
    }
    #[cfg(windows)]
    {
        let mut length = 0;
        // SAFETY: The API caller guarantees a readable terminated string.
        while unsafe { *pointer.add(length) } != 0 {
            length += 1;
        }
        // SAFETY: All units preceding the terminator are readable.
        utf16_to_wtf8(unsafe { std::slice::from_raw_parts(pointer, length) })
    }
}
fn entry_name(tree: &Metadata<'_>, node: usize) -> Result<Vec<u8>, ParseError> {
    let units: Vec<_> = tree.nodes[node]
        .entry
        .name
        .chunks_exact(2)
        .map(|b| u16::from_le_bytes([b[0], b[1]]))
        .collect();
    utf16_to_wtf8(&units)
}
fn components(path: &[u8]) -> Vec<&[u8]> {
    path.split(|&b| b == b'/' || b == b'\\')
        .filter(|b| !b.is_empty())
        .collect()
}
fn literal(tree: &Metadata<'_>, path: &[u8]) -> Result<usize, Option<c_int>> {
    // The original lookup turns conversion failure into a missing-path result;
    // allocation failure itself leaves the host allocation function's errno unchanged.
    let units = wtf8_to_utf16z(path).map_err(|error| {
        if error == ParseError::Nomem {
            None
        } else {
            Some(libc::EILSEQ)
        }
    })?;
    if tree.nodes.is_empty() {
        return Err(Some(libc::ENOENT));
    }
    let units = &units[..units.len() - 1];
    let mut node = 0;
    for component in units
        .split(|&u| u == b'/' as u16 || u == b'\\' as u16)
        .filter(|b| !b.is_empty())
    {
        let inode = tree.inode_entry(node).ok_or(Some(libc::ENOENT))?;
        if !inode.is_directory() || inode.attributes & 0x400 != 0 {
            return Err(Some(libc::ENOTDIR));
        }
        let matches = |index: usize, folded: bool| {
            let fold = |u| {
                if folded {
                    wim_format::ntfs_upcase::uppercase(u)
                } else {
                    u
                }
            };
            tree.nodes[index]
                .entry
                .name
                .chunks_exact(2)
                .map(|b| fold(u16::from_le_bytes([b[0], b[1]])))
                .eq(component.iter().copied().map(fold))
        };
        node = *tree.nodes[node]
            .children
            .iter()
            .find(|&&i| matches(i, false))
            .or_else(|| {
                crate::engine::runtime::ignore_case()
                    .then(|| {
                        tree.nodes[node]
                            .children
                            .iter()
                            .find(|&&i| matches(i, true))
                    })
                    .flatten()
            })
            .ok_or(Some(libc::ENOENT))?;
    }
    Ok(node)
}
fn lookup_error(error: Option<c_int>) -> ParseError {
    #[cfg(target_os = "linux")]
    if let Some(error) = error {
        // SAFETY: Linux exposes a writable thread-local errno slot.
        unsafe {
            *libc::__errno_location() = error;
        }
    }
    #[cfg(not(target_os = "linux"))]
    let _ = error;
    ParseError::PathDoesNotExist
}
#[cfg(unix)]
pub(super) fn whole_image_root(tree: &Metadata<'_>) -> Result<usize, ParseError> {
    literal(tree, b"/").map_err(lookup_error)
}
fn wildcard(name: &[u8], pattern: &[u8]) -> bool {
    let (mut n, mut p, mut star, mut restart) = (0, 0, None, 0);
    let fold = |b: u8| {
        if crate::engine::runtime::ignore_case() {
            b.to_ascii_lowercase()
        } else {
            b
        }
    };
    while n < name.len() {
        if p < pattern.len() && (pattern[p] == b'?' || fold(pattern[p]) == fold(name[n])) {
            n += 1;
            p += 1;
        } else if pattern.get(p) == Some(&b'*') {
            star = Some(p);
            p += 1;
            restart = n;
        } else if let Some(s) = star {
            restart += 1;
            n = restart;
            p = s + 1;
        } else {
            return false;
        }
    }
    while pattern.get(p) == Some(&b'*') {
        p += 1;
    }
    p == pattern.len()
}
fn glob(tree: &Metadata<'_>, path: &[u8]) -> Result<Vec<usize>, ParseError> {
    if tree.nodes.is_empty() {
        return Ok(Vec::new());
    }
    let mut matched = vec![0];
    for pattern in components(path) {
        let mut next = Vec::new();
        for parent in matched {
            for &child in &tree.nodes[parent].children {
                if wildcard(&entry_name(tree, child)?, pattern) {
                    next.push(child);
                }
            }
        }
        matched = next;
    }
    Ok(matched)
}
#[cfg(any(unix, windows))]
fn descendants(tree: &Metadata<'_>, root: usize, output: &mut Vec<usize>) {
    output.push(root);
    for &child in &tree.nodes[root].children {
        descendants(tree, child, output);
    }
}
#[cfg(any(unix, windows))]
fn selection(tree: &Metadata<'_>, mut roots: Vec<usize>, flatten: bool) -> Selection {
    let mut distinct = HashSet::new();
    roots.retain(|r| distinct.insert(*r));
    roots.retain(|&root| {
        let mut parent = tree.nodes[root].parent;
        while let Some(index) = parent {
            if distinct.contains(&index) {
                return false;
            }
            parent = tree.nodes[index].parent;
        }
        true
    });
    let mut nodes = Vec::new();
    for &root in &roots {
        descendants(tree, root, &mut nodes);
    }
    let mut parents = HashMap::new();
    if flatten {
        for root in roots {
            if root != 0 {
                parents.insert(root, 0);
            }
        }
    } else {
        for root in roots {
            let mut pending = Vec::new();
            let mut parent = tree.nodes[root].parent;
            let mut position = 0;
            while let Some(index) = parent {
                if let Some(found) = nodes.iter().position(|&i| i == index) {
                    position = found + 1;
                    break;
                }
                pending.push(index);
                parent = tree.nodes[index].parent;
            }
            pending.reverse();
            nodes.splice(position..position, pending);
        }
    }
    Selection { nodes, parents }
}

/// Extract selected image paths, with component wildcards and optional flattened roots.
///
/// # Safety
/// Handle, target, pointer array, and nonnull path strings must be readable and
/// remain valid for the call. Registered callback code/context must remain live.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn wimlib_extract_paths(
    handle: *mut WimHandle,
    image: c_int,
    target: *const TChar,
    paths: *const *const TChar,
    num_paths: usize,
    flags: c_int,
) -> c_int {
    let result = (|| {
        if flags as u32 & !PUBLIC_FLAGS != 0
            || (num_paths != 0 && paths.is_null())
            || num_paths > isize::MAX as usize / std::mem::size_of::<*const TChar>()
        {
            return Err(ParseError::InvalidParam);
        }
        // SAFETY: The caller supplies an exclusive live handle and terminated target.
        let handle = unsafe { handle.as_ref() }.ok_or(ParseError::InvalidParam)?;
        let target = unsafe { path_from_pointer(target) }
            .map_err(|e| ParseError::from_i32(e).unwrap_or(ParseError::InvalidParam))?;
        let mut decoded = Vec::new();
        decoded
            .try_reserve_exact(num_paths)
            .map_err(|_| ParseError::Nomem)?;
        for index in 0..num_paths {
            // SAFETY: The caller guarantees the pointer array and terminated strings.
            decoded.push(unsafe { bytes_from_pointer(*paths.add(index)) }?);
        }
        extract_path_bytes(handle, image, &target, &decoded, flags as u32)
    })();
    result.err().map_or(0, |error| error as c_int)
}

pub(crate) fn extract_paths(
    handle: &WimHandle,
    image: i32,
    target: &std::path::Path,
    paths: &[&std::ffi::OsStr],
    flags: u32,
) -> Result<(), ParseError> {
    let mut decoded = Vec::new();
    decoded
        .try_reserve_exact(paths.len())
        .map_err(|_| ParseError::Nomem)?;
    for path in paths {
        #[cfg(unix)]
        let bytes = {
            use std::os::unix::ffi::OsStrExt;
            path.as_bytes().to_vec()
        };
        #[cfg(windows)]
        let bytes = {
            use std::os::windows::ffi::OsStrExt;
            utf16_to_wtf8(&path.encode_wide().collect::<Vec<_>>())?
        };
        #[cfg(not(any(unix, windows)))]
        let bytes = path
            .to_str()
            .ok_or(ParseError::InvalidUtf8String)?
            .as_bytes()
            .to_vec();
        if bytes.contains(&0) {
            return Err(ParseError::InvalidParam);
        }
        decoded.push(bytes);
    }
    extract_path_bytes(handle, image, target, &decoded, flags)
}

fn extract_path_bytes(
    handle: &WimHandle,
    image: i32,
    target: &std::path::Path,
    paths: &[Vec<u8>],
    flags: u32,
) -> Result<(), ParseError> {
    (|| {
        if flags & !PUBLIC_FLAGS != 0 {
            return Err(ParseError::InvalidParam);
        }
        if target.as_os_str().is_empty() {
            return Err(ParseError::InvalidParam);
        }
        let flags = checked_flags(handle, flags, false)?;
        if image <= 0 || image as u32 > handle.header.image_count {
            return Err(ParseError::InvalidImage);
        }
        let selected_bytes = image_metadata_bytes(handle, image as usize - 1)?;
        Metadata::parse(&selected_bytes)?;
        crate::engine::capture::checksum_pending(handle)?;
        let bytes = image_metadata_bytes(handle, image as usize - 1)?;
        let metadata = Metadata::parse(&bytes)?;
        if flags & 0x0020_0000 != 0
            && let Err(e) = std::fs::create_dir(target)
            && e.kind() != std::io::ErrorKind::AlreadyExists
        {
            return Err(ParseError::Mkdir);
        }
        let mut roots = Vec::new();
        for path in paths {
            if flags & 0x40000 != 0 {
                let matched = glob(&metadata, path)?;
                if matched.is_empty() && flags & 0x80000 != 0 {
                    return Err(ParseError::PathDoesNotExist);
                }
                roots.extend(matched);
            } else {
                roots.push(literal(&metadata, path).map_err(lookup_error)?);
            }
        }
        if roots.is_empty() {
            return Ok(());
        }
        #[cfg(unix)]
        {
            if flags & 0x400 != 0 {
                return super::unix::extract_stdout(handle, &metadata, &roots, flags);
            }
            let selection = selection(&metadata, roots, flags & 0x0020_0000 != 0);
            super::unix::extract(handle, image, target, flags, &metadata, Some(&selection))
        }
        #[cfg(windows)]
        {
            if flags & 0x400 != 0 {
                return Err(ParseError::Unsupported);
            }
            let selection = selection(&metadata, roots, flags & 0x0020_0000 != 0);
            super::windows::extract(handle, image, target, flags, &metadata, Some(&selection))
        }
        #[cfg(not(any(unix, windows)))]
        {
            let _ = (roots, metadata);
            Err(ParseError::Unsupported)
        }
    })()
}

/// Read an original-format text path list and extract its selected image paths.
/// A NULL list filename reads the host standard-input stream; `-` is a literal filename.
///
/// # Safety
/// Handle and target must be live, and a nonnull list filename must be a readable
/// terminated platform string. Standard input and callback storage must remain valid.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn wimlib_extract_pathlist(
    handle: *mut WimHandle,
    image: c_int,
    target: *const TChar,
    path_list_file: *const TChar,
    flags: c_int,
) -> c_int {
    // SAFETY: The caller supplies a terminated filename or a live stdin stream.
    let input = match unsafe { crate::engine::text_file::load_pathlist_text(path_list_file) } {
        Ok(input) => input,
        Err(error) => return error,
    };
    let mut lines: Vec<Vec<TChar>> = Vec::new();
    let whitespace = |b: TChar| matches!(b as u32, 9..=13 | 32);
    for raw in input.split(|&b| b == b'\n' as TChar) {
        let start = raw
            .iter()
            .position(|&b| !whitespace(b))
            .unwrap_or(raw.len());
        let end = raw
            .iter()
            .rposition(|&b| !whitespace(b))
            .map_or(start, |i| i + 1);
        let mut line = &raw[start..end];
        if line.is_empty() || matches!(line[0] as u32, 35 | 59) {
            continue;
        }
        if line.len() >= 2 && matches!(line[0] as u32, 34 | 39) && line.last() == line.first() {
            line = &line[1..line.len() - 1];
        }
        let mut output = line[..line.iter().position(|&b| b == 0).unwrap_or(line.len())].to_vec();
        output.push(0);
        lines.push(output);
    }
    let pointers: Vec<_> = lines.iter().map(|line| line.as_ptr()).collect();
    // SAFETY: Every path and pointer-array entry remains live throughout extraction.
    unsafe {
        wimlib_extract_paths(
            handle,
            image,
            target,
            pointers.as_ptr(),
            pointers.len(),
            flags,
        )
    }
}
