// SPDX-License-Identifier: LGPL-2.1-or-later
//! Mount entry points for the explicitly disabled FUSE backend.
//!
//! This build has no FUSE filesystem implementation. The original no-FUSE
//! configuration rejects these calls before validating or reading arguments.
//! Successful filesystem mount and commit support require a separate backend.
use crate::engine::{TChar, handles::WimHandle, progress::ProgressCallback};
use std::ffi::{c_int, c_void};
use wim_format::ParseError;

fn unavailable() -> c_int {
    crate::engine::diagnostics::ensure_default_sink();
    #[cfg(windows)]
    let message = b"Sorry-- Mounting WIM images is not supported on Windows!".as_slice();
    #[cfg(not(windows))]
    let message =
        b"wimlib was compiled with --without-fuse, which disables support for mounting WIMs."
            .as_slice();
    crate::engine::diagnostics::message(false, message, false);
    ParseError::Unsupported as c_int
}

/// Report unavailable mounting capability for this build.
///
/// Arguments are not dereferenced, and no image or filesystem state is changed.
#[unsafe(no_mangle)]
pub extern "C" fn wimlib_mount_image(
    _wim: *mut WimHandle,
    _image: c_int,
    _dir: *const TChar,
    _mount_flags: c_int,
    _staging_dir: *const TChar,
) -> c_int {
    unavailable()
}

/// Report unavailable unmounting capability for this build.
#[unsafe(no_mangle)]
pub extern "C" fn wimlib_unmount_image(dir: *const TChar, unmount_flags: c_int) -> c_int {
    wimlib_unmount_image_with_progress(dir, unmount_flags, None, std::ptr::null_mut())
}

/// Report unavailable unmounting capability without invoking the callback.
#[unsafe(no_mangle)]
pub extern "C" fn wimlib_unmount_image_with_progress(
    _dir: *const TChar,
    _unmount_flags: c_int,
    _progress: Option<ProgressCallback>,
    _context: *mut c_void,
) -> c_int {
    unavailable()
}
