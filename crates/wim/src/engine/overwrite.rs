// SPDX-License-Identifier: LGPL-2.1-or-later
//! Commit mutable archives through append or atomic replacement.
use crate::engine::{
    handles::{WimHandle, can_modify, handle_mut, handle_ref},
    progress::{ProgressInfo, RenameProgress, filename_buffer},
    write::{
        self,
        write_plan::{Effects, Plan},
    },
};
use std::{
    ffi::c_int,
    fs::{File, OpenOptions},
    io::{Seek, SeekFrom, Write},
    path::Path,
};
use wim_format::PIPABLE_MAGIC;

/// Commit changes to the backing file. Only `wimlib_free` is valid after success.
///
/// # Safety
/// The handle must remain live throughout this call and its callbacks. A callback
/// may replace progress registration, but must not free or mutate archive resources.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn wimlib_overwrite(
    pointer: *mut WimHandle,
    flags: c_int,
    threads: u32,
) -> c_int {
    if flags as u32 & !0xffff != 0 {
        return 24;
    }
    // SAFETY: The caller keeps the handle live. The raw core limits exclusive
    // borrows to phases without callbacks, permitting progress re-registration.
    unsafe { overwrite_archive_inner(pointer, flags, threads) }
}

/// Commit a typed archive handle to its retained backing path.
pub(crate) fn overwrite_archive(
    handle: &mut WimHandle,
    flags: c_int,
    threads: u32,
) -> Result<(), c_int> {
    // SAFETY: The exclusive Rust borrow keeps the handle alive; native Rust
    // cancellation callbacks cannot reenter the handle.
    let status = unsafe { overwrite_archive_inner(handle, flags, threads) };
    if status == 0 { Ok(()) } else { Err(status) }
}

unsafe fn overwrite_archive_inner(pointer: *mut WimHandle, flags: c_int, threads: u32) -> c_int {
    if flags as u32 & !0xffff != 0 {
        return 24;
    }
    let Some(wim) = (unsafe { handle_ref(pointer) }) else {
        return 24;
    };
    if wim.filename.is_none() {
        return 45;
    }
    if flags & 0x8000 != 0 && flags & 16 != 0 {
        return 85;
    }
    let flags = if flags & 0x8000 != 0 {
        (flags & !64) | 128
    } else {
        flags
    };
    let saved = wim.header.flags;
    // No callback occurs while temporarily excluding the logical readonly flag.
    let Some(wim) = (unsafe { handle_mut(pointer) }) else {
        return 24;
    };
    if flags & 0x100 != 0 {
        wim.header.flags &= !4;
    }
    let writable = can_modify(wim);
    wim.header.flags = saved;
    if !writable {
        return 71;
    }
    let append = flags & 64 == 0
        && (!wim.image_deletion_occurred || flags & 128 != 0)
        && wim.header.magic != PIPABLE_MAGIC
        && flags & 4 == 0
        && wim.header.validate_compression().ok() == Some(wim.output_compression)
        && wim.header.chunk_size == wim.output_chunk_size;
    if flags & 0x8000 != 0 && !append {
        return 85;
    }
    let path = match wim.filename.as_ref().map(|path| path.try_clone()) {
        Some(Ok(path)) => path,
        Some(Err(_)) => return 39,
        None => return 45,
    };
    if let Err(error) = write::refresh_dirty_image_stats(wim) {
        return error;
    }
    let status = if append {
        unsafe { append_archive(pointer, &path, flags) }
    } else {
        unsafe { rebuild(pointer, &path, flags, threads) }
    };
    if status == 0
        && let Some(wim) = unsafe { handle_mut(pointer) }
    {
        wim.backing = None;
        wim.lookup = None;
        wim.owned_blobs.clear();
    }
    status
}
unsafe fn rebuild(pointer: *mut WimHandle, path: &Path, flags: c_int, _threads: u32) -> c_int {
    let guid = match write::generate_guid() {
        Ok(guid) => guid,
        Err(e) => return e,
    };
    let suffix: String = guid[..9]
        .iter()
        .map(|b| char::from(b'a' + b % 26))
        .collect();
    let mut temporary = path.as_os_str().to_owned();
    temporary.push(suffix);
    let temporary = std::path::PathBuf::from(temporary);
    let from = match filename_buffer(Some(&temporary)) {
        Ok(Some(v)) => v,
        Ok(None) => return 24,
        Err(e) => return e as c_int,
    };
    let to = match filename_buffer(Some(path)) {
        Ok(Some(v)) => v,
        Ok(None) => return 24,
        Err(e) => return e as c_int,
    };
    // SAFETY: The raw write core creates no exclusive borrow across callbacks.
    let status = unsafe { write::write_archive_inner(pointer, &temporary, -1, flags | 0x820) };
    if status != 0 {
        let _ = std::fs::remove_file(&temporary);
        return status;
    }
    if std::fs::rename(&temporary, path).is_err() {
        let _ = std::fs::remove_file(&temporary);
        return 52;
    }
    let Some(wim) = (unsafe { handle_ref(pointer) }) else {
        return 24;
    };
    let mut info = ProgressInfo::zeroed();
    info.rename = RenameProgress {
        from: from.as_ptr(),
        to: to.as_ptr(),
    };
    match unsafe { wim.progress.get().call(15, &mut info) } {
        Ok(()) => 0,
        Err(e) => e as c_int,
    }
}
unsafe fn append_archive(pointer: *mut WimHandle, path: &Path, flags: c_int) -> c_int {
    let Some(wim) = (unsafe { handle_ref(pointer) }) else {
        return 24;
    };
    let selected = match write::settings(
        wim,
        -1,
        (flags & !0x8000) | 0x800 | if flags & 0x8000 == 0 { 0x400 } else { 0 },
    ) {
        Ok(s) => s,
        Err(e) => return e,
    };
    let unchanged = !wim.image_deletion_occurred && wim.images.iter().enumerate().all(|(i,image)| matches!(image, crate::engine::handles::HandleImage::Source(n) if *n == i as u32 + 1));
    let table_end = if wim.header.blob_table.offset_in_wim == 0 {
        208
    } else {
        wim.header.blob_table.offset_in_wim + wim.header.blob_table.size_in_wim
    };
    let xml_end = wim.header.xml_data.offset_in_wim + wim.header.xml_data.size_in_wim;
    let end = if unchanged {
        table_end
    } else if wim.header.integrity_table.offset_in_wim != 0 {
        wim.header.integrity_table.offset_in_wim + wim.header.integrity_table.size_in_wim
    } else {
        xml_end
    };
    if table_end > wim.header.xml_data.offset_in_wim
        || wim.lookup.as_ref().is_some_and(|l| {
            l.resources
                .iter()
                .any(|r| r.header.offset_in_wim + r.header.size_in_wim > end)
        })
    {
        if flags & 0x8000 != 0 {
            return 85;
        }
        return unsafe { rebuild(pointer, path, flags, 1) };
    }
    let mut header = match write::write_plan::initial_header(wim, &selected) {
        Ok(h) => h,
        Err(e) => return e,
    };
    // Appending retains source resources. Upstream write.c preserves the source
    // version here, since ordinary headers make readers ignore solid flags.
    header.version = if flags & 0x1000 != 0 {
        0xe00
    } else {
        wim.header.version
    };
    let plan = match if unchanged && flags & 0x8000 == 0 {
        Plan::unchanged_append(wim, &selected, header)
    } else if flags & 0x8000 == 0 {
        Plan::new_inplace(wim, &selected, header)
    } else {
        Plan::new(wim, &selected, header)
    } {
        Ok(p) => p,
        Err(e) => return e,
    };
    let mut file = match OpenOptions::new().read(true).write(true).open(path) {
        Ok(f) => f,
        Err(_) => return 47,
    };
    let _lock = match Lock::new(&file) {
        Ok(l) => l,
        Err(e) => return e,
    };
    if file
        .seek(SeekFrom::Start(16))
        .and_then(|_| file.write_all(&(wim.header.flags | 0x40).to_le_bytes()))
        .and_then(|_| file.seek(SeekFrom::Start(end)))
        .is_err()
    {
        return 72;
    }
    let mut effects = Effects::default();
    let compact = flags & 0x8000 != 0;
    let result = if compact {
        file.seek(SeekFrom::Start(208))
            .map_err(|_| 72)
            .and_then(|_| plan.run_compact(&mut file, &mut effects))
    } else {
        plan.run_append(&mut file, &mut effects, end as usize, unchanged)
    };
    if compact
        && let Ok(output) = &result
        && file.set_len(output.len() as u64).is_err()
    {
        return 72;
    }
    if result.is_err() {
        if !compact && !unchanged {
            let _ = file.set_len(end);
        }
        let _ = file
            .seek(SeekFrom::Start(16))
            .and_then(|_| file.write_all(&wim.header.flags.to_le_bytes()));
    }
    #[cfg(unix)]
    drop(_lock);
    if let Some(total) = effects.total_bytes {
        let Some(wim) = (unsafe { handle_mut(pointer) }) else {
            return 24;
        };
        if let Err(e) = wim.xml.set_total_bytes(Some(total)) {
            return e as c_int;
        }
    }
    match result {
        Ok(_) => 0,
        Err(e) => e,
    }
}
#[cfg(unix)]
struct Lock(i32);
#[cfg(unix)]
unsafe extern "C" {
    fn flock(fd: i32, operation: i32) -> i32;
}
#[cfg(unix)]
impl Lock {
    fn new(file: &File) -> Result<Self, c_int> {
        use std::os::fd::AsRawFd;
        let fd = file.as_raw_fd();
        if unsafe { flock(fd, 2 | 4) } == 0 {
            Ok(Self(fd))
        } else if std::io::Error::last_os_error().kind() == std::io::ErrorKind::WouldBlock {
            Err(1)
        } else {
            // Original locking rejects contention but tolerates unsupported locks.
            Ok(Self(fd))
        }
    }
}
#[cfg(unix)]
impl Drop for Lock {
    fn drop(&mut self) {
        unsafe {
            flock(self.0, 8);
        }
    }
}
#[cfg(not(unix))]
struct Lock;
#[cfg(not(unix))]
impl Lock {
    fn new(_: &File) -> Result<Self, c_int> {
        // Original wimlib's flock-based append locking is compiled only on Unix.
        // Windows callers must serialize mutation (the CLI holds an exclusive job lock).
        #[cfg(windows)]
        return Ok(Self);
        #[cfg(not(windows))]
        Err(68)
    }
}
