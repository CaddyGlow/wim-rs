// SPDX-License-Identifier: LGPL-2.1-or-later
//! C decompressor exports using native Rust codecs.
use ms_compress::context::{ContextError, Decompressor};
use std::ffi::{c_int, c_void};

/// Opaque handle; its layout is deliberately absent from the public C header.
pub struct WimlibDecompressor {
    decoder: Decompressor,
    maximum: usize,
}

/// Create an owned decompressor, leaving `dec_ret` unchanged on failure.
///
/// # Safety
/// A nonnull `dec_ret` must point to writable pointer storage.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn wimlib_create_decompressor(
    codec: c_int,
    maximum: usize,
    dec_ret: *mut *mut WimlibDecompressor,
) -> c_int {
    let initialization = crate::engine::runtime::wimlib_global_init(0);
    if initialization != 0 {
        return initialization;
    }
    // Upstream checks compression type before output storage and allocates the
    // outer context before codec-specific bounds and workspace initialization.
    if ms_compress::context::Codec::from_wimlib(codec).is_err() {
        return 16;
    }
    if dec_ret.is_null() || maximum == 0 {
        return 24;
    }
    // SAFETY: Host C malloc provides aligned storage; failure is checked.
    let pointer =
        unsafe { crate::engine::allocation::malloc(std::mem::size_of::<WimlibDecompressor>()) }
            .cast::<WimlibDecompressor>();
    if pointer.is_null() {
        return 39;
    }
    let decoder = match Decompressor::new(codec, maximum) {
        Ok(decoder) => decoder,
        Err(error) => {
            // SAFETY: Context storage is uninitialized and owned by this call.
            unsafe {
                crate::engine::allocation::free(pointer.cast());
            }
            return if error == ContextError::OutOfMemory {
                39
            } else {
                24
            };
        }
    };
    // SAFETY: Fresh allocation is aligned and writable; dec_ret is caller-owned.
    unsafe {
        pointer.write(WimlibDecompressor { decoder, maximum });
        dec_ret.write(pointer);
    }
    0
}

/// Decode a block, returning 0, -1 for invalid data, or -2 for oversized output.
///
/// # Safety
/// `dec` must be a live handle from this library and exclusively used during this
/// call. Input and output must be disjoint valid buffers of their stated sizes,
/// with lengths at most `isize::MAX`. Zero-length buffers may be null.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn wimlib_decompress(
    compressed: *const c_void,
    compressed_size: usize,
    uncompressed: *mut c_void,
    uncompressed_size: usize,
    dec: *mut WimlibDecompressor,
) -> c_int {
    // SAFETY: The caller supplies a live exclusively accessed handle.
    let handle = unsafe { &mut *dec };
    // Upstream does not inspect either buffer on an oversized request.
    if uncompressed_size > handle.maximum {
        return -2;
    }
    // Rust permits null C buffers of length zero without constructing a null slice.
    let input = if compressed_size == 0 {
        &[]
    } else {
        // SAFETY: Input validity and disjointness are requirements on the caller.
        unsafe { std::slice::from_raw_parts(compressed.cast::<u8>(), compressed_size) }
    };
    let output = if uncompressed_size == 0 {
        &mut []
    } else {
        // SAFETY: Output validity and exclusive access are caller requirements.
        unsafe { std::slice::from_raw_parts_mut(uncompressed.cast::<u8>(), uncompressed_size) }
    };
    match handle.decoder.decompress(input, output) {
        Ok(()) => 0,
        Err(ContextError::OutputExceedsMaximum) => -2,
        Err(_) => -1,
    }
}

/// Free a live decompressor; a null pointer is accepted.
///
/// # Safety
/// A nonnull pointer must be an unfreed handle allocated by this library, with
/// no outstanding calls or references.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn wimlib_free_decompressor(dec: *mut WimlibDecompressor) {
    if !dec.is_null() {
        // SAFETY: The caller transfers exclusive ownership of this allocation.
        unsafe {
            std::ptr::drop_in_place(dec);
            crate::engine::allocation::free(dec.cast());
        }
    }
}
