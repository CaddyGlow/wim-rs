//! Transactional in-memory deletion and renaming of image paths.

use crate::engine::collections::FallibleCollections as _;
use crate::engine::collections::FallibleSet as _;
use crate::engine::{
    TChar,
    handles::{
        HandleImage, OwnedImage, PendingMetadata, WimHandle, image_identity, image_metadata_bytes,
    },
};
use std::ffi::c_int;
use wim_format::{ParseError, metadata::Metadata, metadata_write::OwnedMetadata};

unsafe fn path(pointer: *const TChar) -> Result<Vec<Vec<u16>>, ParseError> {
    if pointer.is_null() {
        return Ok(Vec::new());
    }
    #[cfg(not(windows))]
    // SAFETY: Caller supplies terminated readable platform text.
    let units = wim_format::platform_text::wtf8_to_utf16(
        unsafe { std::ffi::CStr::from_ptr(pointer) }.to_bytes(),
    )?;
    #[cfg(windows)]
    let units = {
        let mut len = 0;
        // SAFETY: Caller supplies terminated readable platform text.
        unsafe {
            while *pointer.add(len) != 0 {
                len += 1;
            }
            std::slice::from_raw_parts(pointer, len).to_vec()
        }
    };
    Ok(units
        .split(|&u| u == 47 || u == 92)
        .filter(|c| !c.is_empty())
        .map(<[u16]>::to_vec)
        .collect())
}
fn errno(value: c_int) {
    #[cfg(target_os = "linux")]
    // SAFETY: libc exposes writable errno storage for this thread.
    unsafe {
        *libc::__errno_location() = value;
    }
    #[cfg(not(target_os = "linux"))]
    let _ = value;
}
fn find(tree: &OwnedMetadata, parts: &[Vec<u16>]) -> Result<usize, ParseError> {
    if tree.nodes.is_empty() {
        errno(libc::ENOENT);
        return Err(ParseError::PathDoesNotExist);
    }
    let mut node = 0;
    for component in parts {
        if tree.nodes[node].attributes & 0x10 == 0 {
            errno(libc::ENOTDIR);
            return Err(ParseError::Notdir);
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
                .name
                .chunks_exact(2)
                .map(|b| fold(u16::from_le_bytes([b[0], b[1]])))
                .eq(component.iter().copied().map(fold))
        };
        node = tree.nodes[node]
            .children
            .iter()
            .copied()
            .find(|&i| matches(i, false))
            .or_else(|| {
                crate::engine::runtime::ignore_case()
                    .then(|| {
                        tree.nodes[node]
                            .children
                            .iter()
                            .copied()
                            .find(|&i| matches(i, true))
                    })
                    .flatten()
            })
            .ok_or_else(|| {
                errno(libc::ENOENT);
                ParseError::PathDoesNotExist
            })?;
    }
    Ok(node)
}
fn parent(tree: &OwnedMetadata, node: usize) -> Option<usize> {
    tree.nodes.iter().position(|n| n.children.contains(&node))
}
fn diagnostic_path(parts: &[Vec<u16>]) -> Result<Vec<u8>, ParseError> {
    let mut units = vec![47];
    for (index, part) in parts.iter().enumerate() {
        if index != 0 {
            units.push(47);
        }
        units.extend_from_slice(part);
    }
    wim_format::platform_text::utf16_to_wtf8(&units)
}
fn compact(tree: &mut OwnedMetadata) -> Result<Vec<usize>, ParseError> {
    let mut order = Vec::new();
    let mut pending = vec![0];
    let mut mapping = vec![usize::MAX; tree.nodes.len()];
    while let Some(index) = pending.pop() {
        if index >= mapping.len() || mapping[index] != usize::MAX {
            return Err(ParseError::InvalidMetadataResource);
        }
        mapping[index] = order.len();
        order.push(index);
        pending.extend(tree.nodes[index].children.iter().rev().copied());
    }
    let mut nodes = Vec::new();
    nodes
        .try_reserve_exact(order.len())
        .map_err(|_| ParseError::Nomem)?;
    for index in order {
        let mut node = tree.nodes[index].clone();
        for child in &mut node.children {
            *child = mapping[*child];
        }
        nodes.push(node);
    }
    tree.nodes = nodes;
    Ok(mapping)
}
pub(crate) fn select_for_update(handle: &WimHandle, image: c_int) -> Result<(), ParseError> {
    if image < 1 || image as u32 > handle.header.image_count {
        return Err(ParseError::InvalidImage);
    }
    let index = image as usize - 1;
    let bytes = image_metadata_bytes(handle, index)?;
    Metadata::parse(&bytes)?;
    if crate::engine::handles::image_owner_count(handle, index) > 1 {
        return Err(ParseError::ImageHasMultipleReferences);
    }
    Ok(())
}
fn mutate(
    handle: &mut WimHandle,
    image: c_int,
    source: &[Vec<u16>],
    target: Option<&[Vec<u16>]>,
    flags: c_int,
) -> Result<(), ParseError> {
    if image < 1 || image as u32 > handle.header.image_count {
        return Err(ParseError::InvalidImage);
    }
    let index = image as usize - 1;
    let bytes = image_metadata_bytes(handle, index)?;
    let parsed = Metadata::parse(&bytes)?;
    if crate::engine::handles::image_owner_count(handle, index) > 1 {
        return Err(ParseError::ImageHasMultipleReferences);
    }
    if flags & !3 != 0 {
        return Err(ParseError::InvalidParam);
    }
    let mut capture = crate::engine::handles::image_capture_plan(&handle.images[index])?;
    let mut tree = match &capture {
        Some(plan) => plan.tree.clone(),
        None => OwnedMetadata::from_metadata(&parsed)?,
    };
    let original = tree.clone();
    let src = match find(&tree, source) {
        Ok(src) => src,
        Err(e) if target.is_none() => {
            if flags & 1 != 0 {
                return Ok(());
            }
            let _ = e;
            let mut message = b"Path \"".to_vec();
            message.extend_from_slice(&diagnostic_path(source)?);
            message.extend_from_slice(format!("\" does not exist in WIM image {image}").as_bytes());
            crate::engine::diagnostics::message(false, &message, false);
            return Err(ParseError::PathDoesNotExist);
        }
        Err(e) => return Err(e),
    };
    let mut root_deleted = false;
    if let Some(target) = target {
        let destination = find(&tree, target).ok();
        if destination == Some(src) {
            return Ok(());
        }
        let destination_parent = if let Some(dst) = destination {
            let src_dir = tree.nodes[src].attributes & 0x10 != 0;
            let dst_dir = tree.nodes[dst].attributes & 0x10 != 0;
            if !src_dir && dst_dir {
                errno(libc::EISDIR);
                return Err(ParseError::IsDirectory);
            }
            if src_dir && !dst_dir {
                errno(libc::ENOTDIR);
                return Err(ParseError::Notdir);
            }
            if src_dir && !tree.nodes[dst].children.is_empty() {
                errno(libc::ENOTEMPTY);
                return Err(ParseError::Notempty);
            }
            parent(&tree, dst).unwrap_or(dst)
        } else {
            let p = find(&tree, &target[..target.len().saturating_sub(1)])?;
            if tree.nodes[p].attributes & 0x10 == 0 {
                errno(libc::ENOTDIR);
                return Err(ParseError::Notdir);
            }
            p
        };
        let mut ancestor = Some(destination_parent);
        while let Some(node) = ancestor {
            if node == src {
                errno(libc::EBUSY);
                return Err(ParseError::Notempty);
            }
            ancestor = parent(&tree, node);
        }
        if let Some(dst) = destination {
            tree.nodes[destination_parent]
                .children
                .retain(|&c| c != dst);
        }
        if let Some(p) = parent(&tree, src) {
            tree.nodes[p].children.retain(|&c| c != src);
        }
        tree.nodes[src].name = target
            .last()
            .map(|n| n.iter().flat_map(|u| u.to_le_bytes()).collect())
            .unwrap_or_default();
        tree.nodes[destination_parent].children.push(src);
    } else {
        if tree.nodes[src].attributes & 0x10 != 0 && flags & 2 == 0 {
            let mut message = b"Path \"".to_vec();
            message.extend_from_slice(&diagnostic_path(source)?);
            message.extend_from_slice(format!("\" in WIM image {image} is a directory but a recursive delete was not requested").as_bytes());
            crate::engine::diagnostics::message(false, &message, false);
            return Err(ParseError::IsDirectory);
        }
        if let Some(p) = parent(&tree, src) {
            tree.nodes[p].children.retain(|&c| c != src);
        } else {
            root_deleted = true;
        }
    }
    let output = if root_deleted {
        tree.nodes.clear();
        if let Some(plan) = &mut capture {
            plan.identities.clear();
            plan.bindings.clear();
        }
        let mut output = parsed.security.raw.to_vec();
        output.extend_from_slice(&0u64.to_le_bytes());
        output
    } else {
        let mapping = compact(&mut tree)?;
        if let Some(plan) = &mut capture {
            let mut identities = vec![None; tree.nodes.len()];
            for (old, &new) in mapping.iter().enumerate() {
                if new != usize::MAX {
                    identities[new] = plan.identities.get(old).copied().flatten();
                }
            }
            plan.identities = identities;
            plan.bindings.retain_mut(|binding| {
                let next = mapping.get(binding.node).copied().unwrap_or(usize::MAX);
                if next == usize::MAX {
                    false
                } else {
                    binding.node = next;
                    true
                }
            });
        }
        tree.encode()?
    };
    drop(parsed);
    drop(bytes);
    if let Some(plan) = &mut capture {
        plan.tree = tree.clone();
    }
    let identity = image_identity(handle, &handle.images[index]);
    handle
        .dirty_images
        .try_reserve(1)
        .map_err(|_| ParseError::Nomem)?;
    let staged_index =
        crate::engine::lookup::prepare_capture_index(handle, index, capture.as_ref())?;
    let pending = crate::engine::handles::new_pending_metadata(PendingMetadata {
        capture,
        metadata: output,
        hash: [0; 20],
    })?;
    let counts = |tree: &OwnedMetadata| {
        let mut result = std::collections::HashMap::<[u8; 20], u32>::new();
        for node in &tree.nodes {
            for hash in
                std::iter::once(node.main_hash).chain(node.extra_streams.iter().map(|s| s.hash))
            {
                if hash != [0; 20] {
                    *result.entry(hash).or_default() += 1;
                }
            }
        }
        result
    };
    let next = counts(&tree);
    let mut dead = Vec::new();
    let old_counts = counts(&original);
    for (hash, blob) in handle.owned_blobs.iter() {
        let decrement = old_counts
            .get(hash)
            .copied()
            .unwrap_or(0)
            .saturating_sub(next.get(hash).copied().unwrap_or(0));
        if (blob.bytes.is_memory() || blob.captured.is_some())
            && blob.descriptor.blob.reference_count <= decrement
        {
            staged_index.unlink(hash);
            dead.try_push(*hash).map_err(|_| ParseError::Nomem)?;
        }
    }
    for (hash, count) in old_counts {
        let decrement = count.saturating_sub(next.get(&hash).copied().unwrap_or(0));
        if let Some(blob) = handle.owned_blobs.get_mut(&hash) {
            blob.descriptor.blob.reference_count = blob
                .descriptor
                .blob
                .reference_count
                .saturating_sub(decrement);
        } else if let Some(lookup) = &mut handle.lookup
            && let Some(blob) = lookup.blobs.iter_mut().find(|b| b.hash == hash)
        {
            blob.reference_count = blob.reference_count.saturating_sub(decrement);
        }
    }
    handle.blob_index = staged_index;
    for hash in &dead {
        handle.owned_blobs.remove(hash);
    }
    handle.images[index] = HandleImage::Owned(OwnedImage {
        metadata: Vec::new(),
        hash: [0; 20],
        pending: Some(pending),
        descriptor: None,
        identity,
    });
    handle
        .dirty_images
        .try_insert(identity)
        .map_err(|_| ParseError::Nomem)?;
    Ok(())
}
/// Delete a path in one image, optionally forcing absence or recursively deleting directories.
/// # Safety
/// The handle must be exclusively live; a nonnull path must be readable terminated text.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn wimlib_delete_path(
    handle: *mut WimHandle,
    image: c_int,
    source: *const TChar,
    flags: c_int,
) -> c_int {
    // SAFETY: The C caller guarantees handle and platform string lifetimes.
    unsafe { delete_path(handle, image, source, flags) }
        .err()
        .map_or(0, |error| error as c_int)
}

/// Apply the mutation using checked engine-owned Rust operations.
/// # Safety
/// Handle and platform text pointers must be live for the operation.
pub(crate) unsafe fn delete_path(
    handle: *mut WimHandle,
    image: c_int,
    source: *const TChar,
    flags: c_int,
) -> Result<(), ParseError> {
    // SAFETY: Handle and text validity are caller contracts.
    unsafe { handle.as_mut() }
        .ok_or(ParseError::InvalidParam)
        .and_then(|h| {
            select_for_update(h, image)?;
            if flags & !3 != 0 {
                return Err(ParseError::InvalidParam);
            }
            // SAFETY: The caller supplies readable platform text.
            unsafe { path(source) }.and_then(|p| mutate(h, image, &p, None, flags))
        })
}
/// Rename a path in one image, replacing compatible existing destinations transactionally.
/// # Safety
/// The handle must be exclusively live; nonnull paths must be readable terminated text.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn wimlib_rename_path(
    handle: *mut WimHandle,
    image: c_int,
    source: *const TChar,
    target: *const TChar,
) -> c_int {
    // SAFETY: The C caller guarantees handle and platform string lifetimes.
    unsafe { rename_path(handle, image, source, target) }
        .err()
        .map_or(0, |error| error as c_int)
}

/// Apply the mutation using checked engine-owned Rust operations.
/// # Safety
/// Handle and platform text pointers must be live for the operation.
pub(crate) unsafe fn rename_path(
    handle: *mut WimHandle,
    image: c_int,
    source: *const TChar,
    target: *const TChar,
) -> Result<(), ParseError> {
    // SAFETY: Handle and text validity are caller contracts.
    unsafe { handle.as_mut() }
        .ok_or(ParseError::InvalidParam)
        .and_then(|h| {
            select_for_update(h, image)?;
            unsafe { path(source) }.and_then(|p| {
                unsafe { path(target) }.and_then(|t| {
                    let result = mutate(h, image, &p, Some(&t), 0);
                    if result.is_err() {
                        let mut message = b"Can't rename \"".to_vec();
                        message.extend_from_slice(&diagnostic_path(&p)?);
                        message.extend_from_slice(b"\" to \"");
                        message.extend_from_slice(&diagnostic_path(&t)?);
                        message.push(b'"');
                        crate::engine::diagnostics::message(false, &message, true);
                    }
                    result
                })
            })
        })
}
