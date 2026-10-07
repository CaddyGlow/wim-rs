// SPDX-License-Identifier: LGPL-2.1-or-later
//! Lazy resource writing and progress from mutable image handles.
use crate::engine::{
    TChar,
    handles::{WimHandle, handle_mut, path_from_pointer, pending_metadata},
};
use sha1::{Digest, Sha1};
use std::ffi::c_int;
use std::io::Write;
#[cfg(unix)]
use std::io::{Read, Seek};
use wim_format::{
    Compression, PIPABLE_MAGIC, ParseError, ResourceHeader, lookup::LookupEntry, metadata::Metadata,
};

#[path = "write_plan.rs"]
pub(crate) mod write_plan;

const PUBLIC: u32 = 0xffff;
const UNSUPPORTED: u32 = 0x200;
pub(crate) struct Settings {
    flags: u32,
    pipable: bool,
    solid: bool,
    integrity: bool,
    images: Vec<usize>,
}
pub(crate) fn settings(wim: &WimHandle, image: c_int, flags: c_int) -> Result<Settings, c_int> {
    let flags = flags as u32;
    if flags & !PUBLIC != 0 {
        return Err(24);
    }
    if image != -1 && (image < 1 || image as u32 > wim.header.image_count) {
        return Err(18);
    }
    if wim.header.image_count != 0 && wim.images.is_empty() {
        return Err(36);
    }
    if flags & 3 == 3 || flags & 12 == 12 || flags & 0x8000 != 0 {
        return Err(24);
    }
    let pipable = flags & 4 != 0 || flags & 12 == 0 && wim.header.magic == PIPABLE_MAGIC;
    if pipable && flags & 0x1000 != 0 {
        return Err(24);
    }
    if flags & UNSUPPORTED != 0 {
        return Err(68);
    }
    let integrity =
        flags & 1 != 0 || flags & 3 == 0 && wim.header.integrity_table.offset_in_wim != 0;
    let solid = flags & 0x1000 != 0
        || (!pipable
            && wim.output_compression == Compression::Lzms
            && wim
                .lookup
                .as_ref()
                .is_some_and(|l| l.resources.iter().any(|r| r.solid)));
    let mut images = Vec::new();
    images
        .try_reserve_exact(if image == -1 { wim.images.len() } else { 1 })
        .map_err(|_| 39)?;
    if image == -1 {
        images.extend(0..wim.images.len());
    } else {
        images.push(image as usize - 1);
    }
    Ok(Settings {
        flags,
        pipable,
        solid,
        integrity,
        images,
    })
}
fn table_entry(
    table: &mut Vec<u8>,
    resource: ResourceHeader,
    hash: [u8; 20],
    references: u32,
) -> Result<(), ParseError> {
    table.try_reserve(50).map_err(|_| ParseError::Nomem)?;
    table.extend_from_slice(
        &LookupEntry {
            resource,
            part_number: 1,
            reference_count: references,
            hash,
        }
        .encode(),
    );
    Ok(())
}
fn materialize_empty_images(wim: &WimHandle, selected: &Settings) -> Result<(), c_int> {
    use wim_format::metadata_write::{OwnedDentry, OwnedMetadata};
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_err(|_| 24)?
        .as_nanos()
        / 100
        + 116444736000000000;
    let now = u64::try_from(now).map_err(|_| 24)?;
    for &index in &selected.images {
        let Some(shared) = pending_metadata(&wim.images[index]) else {
            continue;
        };
        let mut shared = shared.lock().map_err(|_| 24)?;
        let metadata = &mut shared.metadata;
        if Metadata::parse(metadata)
            .map_err(|e| e as c_int)?
            .nodes
            .is_empty()
        {
            let mut root = OwnedDentry::new(Vec::new(), 0x10);
            root.creation_time = now;
            root.last_access_time = now;
            root.last_write_time = now;
            *metadata = OwnedMetadata {
                security_descriptors: Vec::new(),
                nodes: vec![root],
            }
            .encode()
            .map_err(|e| e as c_int)?;
            // Upstream's freshly materialized root includes an explicit empty
            // child list after the root-list terminator (dentry.c). The general
            // owned serializer omits empty lists, so preserve this pending-image
            // canonical form here without changing arbitrary caller-owned trees.
            let root_offset = Metadata::parse(metadata)
                .map_err(|e| e as c_int)?
                .security
                .total_length;
            let child_offset = metadata.len() as u64;
            metadata.try_reserve(8).map_err(|_| 39)?;
            metadata.extend_from_slice(&0u64.to_le_bytes());
            metadata[root_offset + 16..root_offset + 24]
                .copy_from_slice(&child_offset.to_le_bytes());
        }
    }
    Ok(())
}
fn hash_written_empty_images(wim: &mut WimHandle, selected: &Settings) -> Result<(), c_int> {
    for &index in &selected.images {
        if let Some(shared) = pending_metadata(&wim.images[index]) {
            let mut shared = shared.lock().map_err(|_| 24)?;
            shared.hash = Sha1::digest(&shared.metadata).into();
        }
    }
    Ok(())
}
pub(crate) fn refresh_dirty_image_stats(wim: &mut WimHandle) -> Result<(), c_int> {
    for index in 0..wim.images.len() {
        let identity = crate::engine::handles::image_identity(wim, &wim.images[index]);
        if !wim.dirty_images.contains(&identity) {
            continue;
        }
        let capture = crate::engine::handles::image_capture_plan(&wim.images[index])
            .map_err(|e| e as c_int)?;
        let (directories, files, total, hardlinks) = if let Some(capture) = capture {
            let mut directories = 0u64;
            let mut files = 0u64;
            let mut total = 0u64;
            let mut hardlinks = 0u64;
            let bound = capture
                .bindings
                .iter()
                .map(|b| ((b.node, b.slot), b.stream.size))
                .collect::<std::collections::BTreeMap<_, _>>();
            let mut visited = std::collections::BTreeSet::new();
            for (node, entry) in capture.tree.nodes.iter().enumerate() {
                if entry.attributes & 0x10 != 0 {
                    directories += 1;
                } else {
                    files += 1;
                }
                let mut size = 0u64;
                for (slot, hash) in std::iter::once(entry.main_hash)
                    .chain(entry.extra_streams.iter().map(|s| s.hash))
                    .enumerate()
                {
                    let bytes = bound.get(&(node, slot)).copied().unwrap_or_else(|| {
                        wim.owned_blobs
                            .get(&hash)
                            .map(|b| b.descriptor.blob.size)
                            .or_else(|| {
                                wim.lookup
                                    .as_ref()
                                    .and_then(|l| l.find(&hash))
                                    .map(|b| b.size)
                            })
                            .unwrap_or(0)
                    });
                    size = size.checked_add(bytes).ok_or(24)?;
                }
                total = total.checked_add(size).ok_or(24)?;
                if entry.inode_union != 0
                    && entry.attributes & 0x400 == 0
                    && !visited.insert(entry.inode_union)
                {
                    hardlinks = hardlinks.checked_add(size).ok_or(24)?;
                }
            }
            (directories, files, total, hardlinks)
        } else {
            let bytes =
                crate::engine::handles::image_metadata_bytes(wim, index).map_err(|e| e as c_int)?;
            let metadata = Metadata::parse(&bytes).map_err(|e| e as c_int)?;
            let mut directories = 0u64;
            let mut files = 0u64;
            let mut total = 0u64;
            let mut hardlinks = 0u64;
            let mut visited = vec![false; metadata.nodes.len()];
            for node in 0..metadata.nodes.len() {
                let entry = metadata.inode_entry(node).ok_or(21)?;
                if entry.attributes & 0x10 != 0 {
                    directories += 1;
                } else {
                    files += 1;
                }
                let mut size = 0u64;
                for stream in &entry.streams {
                    if wim.removed_blobs.contains(&stream.hash) {
                        continue;
                    }
                    size = size
                        .checked_add(
                            wim.owned_blobs
                                .get(&stream.hash)
                                .map(|blob| blob.descriptor.blob.size)
                                .or_else(|| {
                                    wim.lookup
                                        .as_ref()
                                        .and_then(|lookup| lookup.find(&stream.hash))
                                        .map(|blob| blob.size)
                                })
                                .unwrap_or(0),
                        )
                        .ok_or(24)?;
                }
                total = total.checked_add(size).ok_or(24)?;
                let canonical = metadata.nodes[node].inode;
                if visited[canonical] {
                    hardlinks = hardlinks.checked_add(size).ok_or(24)?;
                }
                visited[canonical] = true;
            }
            drop(metadata);
            drop(bytes);
            (directories, files, total, hardlinks)
        };
        let image = index as c_int + 1;
        for (path, value) in [
            ("DIRCOUNT", directories),
            ("FILECOUNT", files),
            ("TOTALBYTES", total),
            ("HARDLINKBYTES", hardlinks),
        ] {
            wim.xml
                .set_property(image, path, Some(&value.to_string()))
                .map_err(|e| e as c_int)?;
        }
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_err(|_| 24)?;
        let timestamp =
            (now.as_secs() + 11_644_473_600) * 10_000_000 + u64::from(now.subsec_nanos()) / 100;
        wim.xml
            .set_property(
                image,
                "LASTMODIFICATIONTIME/HIGHPART",
                Some(&format!("0x{:08X}", timestamp >> 32)),
            )
            .map_err(|e| e as c_int)?;
        wim.xml
            .set_property(
                image,
                "LASTMODIFICATIONTIME/LOWPART",
                Some(&format!("0x{:08X}", timestamp as u32)),
            )
            .map_err(|e| e as c_int)?;
        wim.dirty_images.remove(&identity);
    }
    Ok(())
}
pub(crate) fn generate_guid() -> Result<[u8; 16], c_int> {
    #[cfg(unix)]
    {
        let mut guid = [0; 16];
        std::fs::File::open("/dev/urandom")
            .and_then(|mut f| f.read_exact(&mut guid))
            .map_err(|_| 50)?;
        Ok(guid)
    }
    #[cfg(windows)]
    {
        // Upstream win32_replacements.c uses the same Windows random provider.
        #[link(name = "advapi32")]
        unsafe extern "system" {
            fn SystemFunction036(buffer: *mut std::ffi::c_void, length: u32) -> u8;
        }
        let mut guid = [0; 16];
        // SAFETY: The provider receives exactly the writable stack buffer's length.
        if unsafe { SystemFunction036(guid.as_mut_ptr().cast(), guid.len() as u32) } == 0 {
            return Err(50);
        }
        Ok(guid)
    }
    #[cfg(not(any(unix, windows)))]
    {
        Err(68)
    }
}
fn update_xml_after_write(
    wim: &mut WimHandle,
    output: &write_plan::WrittenOutput,
) -> Result<(), c_int> {
    if !wim.xml.has_total_bytes() {
        wim.xml
            .set_total_bytes(Some(output.total_bytes))
            .map_err(|e| e as c_int)?;
    }
    Ok(())
}
/// Write a new file from the handle's current images, XML and output settings.
/// Uncompressed new output uses bounded payload buffers. Compressed output and
/// mutation source snapshots still buffer bytes. Real progress
/// callbacks run while bytes are written, metadata encoded and integrity hashed.
///
/// # Safety
/// `handle` must be live and exclusively used; `path` must be a terminated
/// platform string. A successful call creates or truncates the requested file.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn wimlib_write(
    handle: *mut WimHandle,
    path: *const TChar,
    image: c_int,
    flags: c_int,
    _threads: u32,
) -> c_int {
    if flags as u32 & !PUBLIC != 0 {
        return 24;
    }
    // SAFETY: Caller supplies a terminated platform path.
    let path = match unsafe { path_from_pointer(path) } {
        Ok(path) => path,
        Err(error) => return error,
    };
    // SAFETY: The caller keeps the handle live. The raw core limits exclusive
    // borrows to phases without callbacks, permitting progress re-registration.
    unsafe { write_archive_inner(handle, &path, image, flags) }
}

/// Write a typed archive handle to a filesystem path.
pub(crate) fn write_archive(
    handle: &mut WimHandle,
    path: &std::path::Path,
    image: c_int,
    flags: c_int,
    _threads: u32,
) -> Result<(), c_int> {
    if path.as_os_str().is_empty() || path.as_os_str().as_encoded_bytes().contains(&0) {
        return Err(24);
    }
    // SAFETY: The exclusive Rust borrow keeps the handle alive; native Rust
    // cancellation callbacks cannot reenter the handle.
    let status = unsafe { write_archive_inner(handle, path, image, flags) };
    if status == 0 { Ok(()) } else { Err(status) }
}

pub(super) unsafe fn write_archive_inner(
    handle: *mut WimHandle,
    path: &std::path::Path,
    image: c_int,
    flags: c_int,
) -> c_int {
    let selected = {
        // SAFETY: Live handle is exclusively used before callbacks begin.
        let Some(wim) = (unsafe { handle_mut(handle) }) else {
            return 24;
        };
        let selected = match settings(wim, image, flags) {
            Ok(s) => s,
            Err(e) => return e,
        };
        if let Err(e) = refresh_dirty_image_stats(wim) {
            return e;
        }
        selected
    };
    // SAFETY: Resource state remains immutable during writing; callbacks may only replace registration.
    let Some(wim) = (unsafe { handle.as_ref() }) else {
        return 24;
    };
    let header = match write_plan::initial_header(wim, &selected) {
        Ok(h) => h,
        Err(e) => return e,
    };
    let mut file = match std::fs::OpenOptions::new()
        .create(true)
        .truncate(true)
        .read(true)
        .write(true)
        .open(path)
    {
        Ok(f) => f,
        Err(_) => return 47,
    };
    if file.write_all(&header.encode_canonical()).is_err() {
        return 72;
    }
    let plan = match write_plan::Plan::new(wim, &selected, header) {
        Ok(p) => p,
        Err(e) => return e,
    };
    let mut effects = write_plan::Effects::default();
    let result = plan.run(&mut file, &mut effects);
    // SAFETY: Callback-bearing phase has ended and shared references are released.
    let wim = unsafe { &mut *handle };
    if let Some(total) = effects.total_bytes
        && let Err(e) = wim.xml.set_total_bytes(Some(total))
    {
        return e as c_int;
    }
    let bytes = match result {
        Ok(bytes) => bytes,
        Err(e) => return e,
    };
    if let Err(e) = hash_written_empty_images(wim, &selected) {
        return e;
    }
    update_xml_after_write(wim, &bytes).map_or_else(|e| e, |()| 0)
}
/// Write to a caller-owned descriptor without closing it. Nonseekable output
/// requires pipable format without integrity. Nonzero initial descriptor offsets
/// and non-Unix descriptor ownership are currently unsupported.
///
/// # Safety
/// `handle` must be live and exclusively used; nonnegative `fd` must remain open
/// and exclusively used throughout this call.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn wimlib_write_to_fd(
    handle: *mut WimHandle,
    fd: c_int,
    image: c_int,
    flags: c_int,
    _threads: u32,
) -> c_int {
    if flags as u32 & !PUBLIC != 0 || fd < 0 {
        return 24;
    }
    let selected = {
        // SAFETY: The handle is exclusively used before callbacks begin.
        let Some(wim) = (unsafe { handle_mut(handle) }) else {
            return 24;
        };
        let selected = match settings(wim, image, flags) {
            Ok(s) => s,
            Err(e) => return e,
        };
        if let Err(e) = refresh_dirty_image_stats(wim) {
            return e;
        }
        selected
    };
    #[cfg(unix)]
    {
        use std::os::fd::{AsFd, BorrowedFd};
        // SAFETY: Descriptor is open and borrowed for the duration required by caller.
        let descriptor = unsafe { BorrowedFd::borrow_raw(fd) };
        let owned = match descriptor.as_fd().try_clone_to_owned() {
            Ok(fd) => fd,
            Err(_) => return 72,
        };
        let mut file = std::fs::File::from(owned);
        match file.stream_position() {
            Ok(0) => {}
            Ok(_) => return 68,
            Err(_) => {
                if !selected.pipable || selected.integrity {
                    return 24;
                }
            }
        }
        // SAFETY: Resource state stays immutable until all callbacks finish.
        let wim = unsafe { &*handle };
        let header = match write_plan::initial_header(wim, &selected) {
            Ok(h) => h,
            Err(e) => return e,
        };
        if file.write_all(&header.encode_canonical()).is_err() {
            return 72;
        }
        let plan = match write_plan::Plan::new(wim, &selected, header) {
            Ok(p) => p,
            Err(e) => return e,
        };
        let mut effects = write_plan::Effects::default();
        let result = plan.run(&mut file, &mut effects);
        // SAFETY: Shared plan and callback phase have ended.
        let wim = unsafe { &mut *handle };
        if let Some(total) = effects.total_bytes
            && let Err(e) = wim.xml.set_total_bytes(Some(total))
        {
            return e as c_int;
        }
        let bytes = match result {
            Ok(bytes) => bytes,
            Err(e) => return e,
        };
        if let Err(error) = hash_written_empty_images(wim, &selected) {
            return error;
        }
        update_xml_after_write(wim, &bytes).map_or_else(|e| e, |()| 0)
    }
    #[cfg(not(unix))]
    {
        let _ = selected;
        68
    }
}
