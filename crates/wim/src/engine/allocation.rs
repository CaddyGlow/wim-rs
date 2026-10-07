// SPDX-License-Identifier: LGPL-2.1-or-later
//! Fixed host C allocation for C-owned output buffers and opaque ABI objects.
//! Rust containers use the global Rust allocator; allocator hooks are unsupported.
use std::ffi::c_void;

unsafe extern "C" {
    #[link_name = "malloc"]
    fn host_malloc(size: usize) -> *mut c_void;
    #[link_name = "free"]
    fn host_free(pointer: *mut c_void);
}

/// Allocate through the host C runtime, normalizing empty requests to one byte.
///
/// # Safety
/// Successful storage must be initialized before reading and released with
/// this module's free function or the matching host C runtime.
pub(crate) unsafe fn malloc(size: usize) -> *mut c_void {
    // SAFETY: The host allocator accepts any byte count.
    unsafe { host_malloc(size.max(1)) }
}

/// Release a host C allocation; NULL is accepted.
///
/// # Safety
/// The pointer must be NULL or a live allocation from the matching C runtime.
pub(crate) unsafe fn free(pointer: *mut c_void) {
    // SAFETY: Caller provides a matching live allocation or NULL.
    unsafe { host_free(pointer) }
}
