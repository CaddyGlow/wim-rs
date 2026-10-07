// SPDX-License-Identifier: LGPL-2.1-or-later
//! Raw original XML access and extraction through the platform C runtime.
use crate::engine::handles::WimHandle;
use std::ffi::{c_int, c_void};

unsafe extern "C" {
    fn fwrite(buffer: *const c_void, size: usize, count: usize, stream: *mut c_void) -> usize;
}

/// Allocate the original file's raw UTF-16LE XML bytes using the host C allocator.
/// Edits to the in-memory XML tree do not change this original resource.
/// The caller releases successful output with the host C runtime `free` function.
/// Returned buffers use the host C allocation function.
///
/// # Safety
/// `handle` must be live and exclusively accessed. Nonnull output arguments
/// must be writable for their respective pointer and length values.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn wimlib_get_xml_data(
    handle: *mut WimHandle,
    output: *mut *mut c_void,
    size: *mut usize,
) -> c_int {
    // SAFETY: Caller guarantees a live handle when nonnull.
    let Some(handle) = (unsafe { handle.as_ref() }) else {
        return 24;
    };
    #[cfg(target_os = "linux")]
    if handle.filename.is_none() {
        // Upstream checks seekability of the absent input descriptor (-1) before
        // validating output arguments. Preserve the resulting EBADF side effect.
        // SAFETY: errno is writable storage belonging to the calling thread.
        unsafe {
            *libc::__errno_location() = libc::EBADF;
        }
    }
    if output.is_null() || size.is_null() {
        return 24;
    }
    // Upstream publishes the advertised size before resource reading.
    // SAFETY: The caller supplies writable size storage.
    unsafe {
        size.write(handle.header.xml_data.uncompressed_size as usize);
    }
    let bytes = match handle.backing.as_deref() {
        Some(backing) => match backing.header().and_then(|header| {
            wim_format::file_archive::read_resource(
                &mut backing.reader(),
                &header,
                &header.xml_data,
            )
        }) {
            Ok(bytes) => bytes,
            Err(error) => return error as c_int,
        },
        None => Vec::new(),
    };
    // SAFETY: C allocation accepts every size, including zero; failure is checked.
    let pointer = unsafe { crate::engine::allocation::malloc(bytes.len()) };
    if pointer.is_null() {
        return 39;
    }
    // SAFETY: Allocation has bytes.len() writable bytes and is disjoint from input.
    unsafe {
        std::ptr::copy_nonoverlapping(bytes.as_ptr(), pointer.cast::<u8>(), bytes.len());
        output.write(pointer);
    }
    0
}

/// Append the original XML resource to a caller-owned C `FILE` stream.
/// The stream remains open and positioned immediately after the written bytes.
///
/// # Safety
/// `handle` must be live and exclusively accessed. `stream` must refer to a
/// live writable host C `FILE`, exclusively used for this call.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn wimlib_extract_xml_data(
    handle: *mut WimHandle,
    stream: *mut c_void,
) -> c_int {
    let mut pointer = std::ptr::null_mut();
    let mut size = 0;
    // SAFETY: Handle validity is caller-owned; local output storage is writable.
    let result = unsafe { wimlib_get_xml_data(handle, &mut pointer, &mut size) };
    if result != 0 {
        return result;
    }
    // SAFETY: Getter allocated size bytes; stream validity is caller-owned.
    let written = unsafe { fwrite(pointer, 1, size, stream) };
    // SAFETY: Getter allocation uses the matching host C allocation family.
    unsafe {
        crate::engine::allocation::free(pointer);
    }
    if written == size { 0 } else { 72 }
}
