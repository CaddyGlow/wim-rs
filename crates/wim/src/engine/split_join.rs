// SPDX-License-Identifier: LGPL-2.1-or-later
//! Lazy split-set writing and joining with real progress through the native ABI.
use crate::engine::{
    TChar,
    handles::{HandleImage, WimHandle, handle_ref, path_from_pointer},
    wimlib_free,
};
use std::ffi::c_int;
use std::io::Write;
use std::path::{Path, PathBuf};
use wim_format::{Compression, PIPABLE_MAGIC};
const PUBLIC: u32 = 0xffff;
fn part_path(first: &Path, number: usize) -> Result<PathBuf, c_int> {
    if number == 1 {
        return Ok(first.to_owned());
    }
    let name = first.file_name().ok_or(24)?;
    #[cfg(unix)]
    {
        use std::os::unix::ffi::{OsStrExt, OsStringExt};
        let bytes = name.as_bytes();
        let split = bytes
            .iter()
            .rposition(|&b| b == b'.')
            .unwrap_or(bytes.len());
        let mut result = Vec::new();
        result.try_reserve(bytes.len() + 20).map_err(|_| 39)?;
        result.extend_from_slice(&bytes[..split]);
        result.extend_from_slice(number.to_string().as_bytes());
        result.extend_from_slice(&bytes[split..]);
        Ok(first.with_file_name(std::ffi::OsString::from_vec(result)))
    }
    #[cfg(windows)]
    {
        use std::os::windows::ffi::{OsStrExt, OsStringExt};
        let bytes: Vec<u16> = name.encode_wide().collect();
        let split = bytes
            .iter()
            .rposition(|&b| b == b'.' as u16)
            .unwrap_or(bytes.len());
        let mut result = Vec::new();
        result.try_reserve(bytes.len() + 20).map_err(|_| 39)?;
        result.extend_from_slice(&bytes[..split]);
        result.extend(number.to_string().encode_utf16());
        result.extend_from_slice(&bytes[split..]);
        Ok(first.with_file_name(std::ffi::OsString::from_wide(&result)))
    }
}
fn flags_checked(flags: u32) -> Result<(), c_int> {
    if flags & !PUBLIC != 0 || flags & 3 == 3 || flags & 12 == 12 || flags & 0x8000 != 0 {
        Err(24)
    } else {
        Ok(())
    }
}
/// Split unchanged on-disk image metadata into conventionally named files.
/// XML/header changes and deletion of original images are supported. Pending or
/// externally owned image metadata, recompression and pipable output are pending.
/// The source snapshot and one output part remain buffered; later parts are not
/// assembled after a callback cancels.
///
/// # Safety
/// `handle` must be live and exclusively used; `name` must be a NUL-terminated
/// platform path. Output files are created or truncated at the requested names.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn wimlib_split(
    handle: *mut WimHandle,
    name: *const TChar,
    size: u64,
    flags: c_int,
) -> c_int {
    if size == 0 {
        return 24;
    }
    // SAFETY: Platform path validity is a caller requirement.
    let path = match unsafe { path_from_pointer(name) } {
        Ok(p) => p,
        Err(e) => return e,
    };
    let flags = flags as u32;
    if flags & !PUBLIC != 0 {
        return 24;
    }
    // SAFETY: Caller supplies a live handle.
    let Some(wim) = (unsafe { handle_ref(handle) }) else {
        return 24;
    };
    if wim.header.image_count != 0 && wim.images.is_empty() {
        return 36;
    }
    if wim
        .lookup
        .as_ref()
        // Upstream tests retained blobs, not orphan resource descriptors whose
        // duplicate blob entries were ignored while reading the lookup table.
        .is_some_and(|l| {
            l.blobs
                .iter()
                .any(|blob| l.resources[blob.resource_index].solid)
        })
    {
        return 68;
    }
    if wim
        .images
        .iter()
        .any(|i| !matches!(i, HandleImage::Source(_)))
    {
        return 68;
    }
    if let Err(e) = flags_checked(flags) {
        return e;
    }
    let pipable = flags & 4 != 0 || flags & 12 == 0 && wim.header.magic == PIPABLE_MAGIC;
    if pipable && flags & 0x1000 != 0 {
        return 24;
    }
    if flags & (0x200 | 0x1000) != 0 {
        return 68;
    }
    if wim.backing.is_some()
        && (wim.output_compression
            != wim
                .header
                .validate_compression()
                .unwrap_or(Compression::None)
            || wim.output_chunk_size != wim.header.chunk_size)
    {
        return 68;
    }
    let guid = if flags & 0x800 != 0 {
        wim.header.guid
    } else {
        match crate::engine::write::generate_guid() {
            Ok(g) => g,
            Err(e) => return e,
        }
    };
    let selected = match crate::engine::write::settings(wim, -1, (flags | 0x400 | 0x800) as c_int) {
        Ok(s) => s,
        Err(e) => return e,
    };
    let mut groups: Vec<(Vec<[u8; 20]>, u64)> = vec![(Vec::new(), 0)];
    if let Some(lookup) = &wim.lookup {
        for image in &wim.images {
            let HandleImage::Source(index) = image else {
                return 68;
            };
            let Some(blob) = lookup.metadata.get((*index - 1) as usize) else {
                return 36;
            };
            groups[0].1 += lookup.resources[blob.resource_index].header.size_in_wim;
        }
        let mut blobs = lookup
            .blobs
            .iter()
            .filter(|b| !wim.removed_blobs.contains(&b.hash))
            .collect::<Vec<_>>();
        blobs.sort_by_key(|b| lookup.resources[b.resource_index].header.offset_in_wim);
        for blob in blobs {
            let stored = lookup.resources[blob.resource_index].header.size_in_wim;
            let last = groups.last_mut().unwrap();
            if last.1 != 0 && last.1.checked_add(stored).is_none_or(|total| total >= size) {
                if groups.len() == 65535 {
                    return 24;
                }
                groups.push((Vec::new(), 0));
            }
            let last = groups.last_mut().unwrap();
            last.0.push(blob.hash);
            last.1 += stored;
        }
    }
    let total_bytes = groups.iter().map(|(_, size)| size).sum();
    let total_parts = groups.len() as u32;
    let mut info = crate::engine::progress::ProgressInfo::zeroed();
    info.split = crate::engine::progress::SplitProgress {
        total_bytes,
        completed_bytes: 0,
        cur_part_number: 0,
        total_parts,
        part_name: std::ptr::null_mut(),
    };
    let mut effects = crate::engine::write::write_plan::Effects::default();
    let result = (|| {
        for (index, (hashes, stored)) in groups.iter().enumerate() {
            let part_path = match part_path(&path, index + 1) {
                Ok(p) => p,
                Err(e) => return e,
            };
            let mut name = match crate::engine::progress::filename_buffer(Some(&part_path)) {
                Ok(Some(name)) => name,
                Ok(None) => return 24,
                Err(e) => return e as c_int,
            };
            info.split.cur_part_number = index as u32 + 1;
            info.split.part_name = name.as_mut_ptr();
            // SAFETY: Payload and operation-owned filename stay live through both callbacks.
            if let Err(e) = unsafe { wim.progress.get().call(19, &mut info) } {
                return e as c_int;
            }
            // SAFETY: A caller replacement is a terminated live path through this part.
            let part_path = match unsafe { path_from_pointer(info.split.part_name) } {
                Ok(p) => p,
                Err(e) => return e,
            };
            let mut header = match crate::engine::write::write_plan::initial_header(wim, &selected)
            {
                Ok(h) => h,
                Err(e) => return e,
            };
            header.guid = guid;
            header.part_number = index as u16 + 1;
            header.total_parts = total_parts as u16;
            if total_parts != 1 {
                header.flags |= 8;
                header.boot_index = 0;
            }
            let mut file = match std::fs::OpenOptions::new()
                .create(true)
                .truncate(true)
                .read(true)
                .write(true)
                .open(part_path)
            {
                Ok(f) => f,
                Err(_) => return 47,
            };
            if file.write_all(&header.encode_canonical()).is_err() {
                return 72;
            }
            let mut plan = match crate::engine::write::write_plan::Plan::new(wim, &selected, header)
            {
                Ok(p) => p,
                Err(e) => return e,
            };
            plan.retain_part(hashes);
            if let Err(e) = plan.run(&mut file, &mut effects) {
                return e;
            }
            // SAFETY: Split member remains active and its backing name remains live.
            unsafe {
                info.split.completed_bytes += stored;
            }
            if let Err(e) = unsafe { wim.progress.get().call(20, &mut info) } {
                return e as c_int;
            }
        }
        0
    })();
    // SAFETY: Callback-bearing shared borrows have ended; only XML is changed now.
    if let Some(total) = effects.total_bytes
        && let Err(e) = unsafe { &mut (*handle).xml }.set_total_bytes(Some(total))
    {
        return e as c_int;
    }
    result
}
struct HandleOwner(*mut WimHandle);
impl Drop for HandleOwner {
    fn drop(&mut self) {
        // SAFETY: Owner holds one live native allocation.
        unsafe {
            wimlib_free(self.0);
        }
    }
}
/// Join a complete split set in arbitrary order into a new standalone file.
/// Retains the split-set GUID and applies ordinary, pipable or solid output flags.
/// Progress callbacks dispatch during actual open and writer phases; bounded
/// output memory and omitted writer flags remain partial gates.
///
/// # Safety
/// `names` must point to `count` readable terminated platform paths; `output`
/// must be a terminated platform path. The requested output is created/truncated.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn wimlib_join(
    names: *const *const TChar,
    count: u32,
    output: *const TChar,
    open_flags: c_int,
    write_flags: c_int,
) -> c_int {
    // SAFETY: The delegated operation has exactly the same path ownership requirements.
    unsafe {
        join_impl(
            names,
            count,
            output,
            open_flags,
            write_flags,
            crate::engine::progress::ProgressRegistration::default(),
        )
    }
}
/// Join split resources through real open, stream, metadata and integrity callbacks.
/// Caller-owned callback code/context remains live for the whole operation.
///
/// # Safety
/// Paths and arrays must be readable for the operation. Callback context follows
/// the registration safety contract and output is created or truncated.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn wimlib_join_with_progress(
    names: *const *const TChar,
    count: u32,
    output: *const TChar,
    open_flags: c_int,
    write_flags: c_int,
    callback: Option<crate::engine::progress::ProgressCallback>,
    context: *mut std::ffi::c_void,
) -> c_int {
    // SAFETY: Caller supplies live paths, arrays and callback registration.
    unsafe {
        join_impl(
            names,
            count,
            output,
            open_flags,
            write_flags,
            crate::engine::progress::ProgressRegistration::new(callback, context),
        )
    }
}
unsafe fn join_impl(
    names: *const *const TChar,
    count: u32,
    output: *const TChar,
    open_flags: c_int,
    write_flags: c_int,
    registration: crate::engine::progress::ProgressRegistration,
) -> c_int {
    if count == 0 || count > 65535 || names.is_null() {
        return 24;
    }
    let mut handles = Vec::new();
    if handles.try_reserve_exact(count as usize).is_err() {
        return 39;
    }
    for index in 0..count as usize {
        let mut handle = std::ptr::null_mut();
        // SAFETY: Caller guarantees readable array/path storage and callback lifetime.
        let rc = unsafe {
            crate::engine::wimlib_open_wim_with_progress(
                *names.add(index),
                open_flags,
                &mut handle,
                registration.callback,
                registration.context,
            )
        };
        if rc != 0 {
            return rc;
        }
        handles.push(HandleOwner(handle));
    }
    // SAFETY: Retained owners are live and not mutated while sorting or checking headers.
    handles.sort_by_key(|h| unsafe { (*h.0).header.part_number });
    let first = unsafe { &*handles[0].0 };
    for (index, owned) in handles.iter().enumerate() {
        // SAFETY: Each owner remains live throughout the comparison.
        let h = unsafe { &*owned.0 };
        if h.header.guid != first.header.guid
            || h.header.total_parts != count as u16
            || h.header.part_number != index as u16 + 1
        {
            return 62;
        }
    }
    let sources = handles.iter().skip(1).map(|h| h.0).collect::<Vec<_>>();
    // SAFETY: References clone independent resources; all owners remain live.
    let rc = unsafe {
        crate::engine::wimlib_reference_resources(
            handles[0].0,
            sources.as_ptr(),
            sources.len() as u32,
            0,
        )
    };
    if rc != 0 {
        return rc;
    }
    // SAFETY: No reference to mutable resource state remains across writer callbacks.
    unsafe { crate::engine::wimlib_write(handles[0].0, output, -1, write_flags | 0x400 | 0x800, 1) }
}
