// SPDX-License-Identifier: LGPL-2.1-or-later
//! Compressor ownership and validation for the original C ABI.
//!
//! Compression levels are validated and defaults retained, but current native
//! encoders use a fixed search strategy. Real codec workspaces use the Rust global
//! allocator and persist across calls; level tuning remains a validation gate.
use ms_compress::context::{CompressionDefaults, FixedStrategyCompressor};
use std::ffi::{c_int, c_void};
use std::sync::{LazyLock, Mutex};

static DEFAULTS: LazyLock<Mutex<CompressionDefaults>> =
    LazyLock::new(|| Mutex::new(CompressionDefaults::default()));

pub(crate) fn default_compression_level(codec: i32) -> u32 {
    let values = DEFAULTS
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    values.raw_level(codec).unwrap_or(0)
}

/// Set the process-wide raw default for one codec, or all codecs for `-1`.
#[unsafe(no_mangle)]
pub extern "C" fn wimlib_set_default_compression_level(codec: c_int, level: u32) -> c_int {
    let mut defaults = DEFAULTS
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    if defaults.set(codec, level).is_err() {
        return 16;
    }
    0
}

/// Allocate a reusable native compressor. Levels currently select no tuning.
///
/// # Safety
/// `output`, when non-null, must be writable for one opaque pointer. The returned
/// handle must be released once with `wimlib_free_compressor`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn wimlib_create_compressor(
    codec: c_int,
    maximum: usize,
    level: u32,
    output: *mut *mut c_void,
) -> c_int {
    let initialization = crate::engine::runtime::wimlib_global_init(0);
    if initialization != 0 {
        return initialization;
    }
    if ms_compress::context::Codec::from_wimlib(codec).is_err() {
        return 16;
    }
    if level & !0x8000_0000 > 0x00ff_ffff || output.is_null() || maximum == 0 {
        return 24;
    }
    // Upstream allocates the outer object before codec-specific size validation.
    // SAFETY: C callbacks provide aligned nonzero storage; failure is checked.
    let pointer = unsafe {
        crate::engine::allocation::malloc(std::mem::size_of::<FixedStrategyCompressor>())
    }
    .cast::<FixedStrategyCompressor>();
    if pointer.is_null() {
        return 39;
    }
    let config = {
        let defaults = DEFAULTS
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        defaults.resolve(codec, maximum, level)
    };
    let compressor = config.and_then(|config| FixedStrategyCompressor::new(codec, config.maximum));
    let compressor = match compressor {
        Ok(value) => value,
        Err(error) => {
            // SAFETY: Allocation is still uninitialized, so only storage is freed.
            unsafe {
                crate::engine::allocation::free(pointer.cast());
            }
            return if error == ms_compress::context::ContextError::OutOfMemory {
                39
            } else {
                24
            };
        }
    };
    // SAFETY: Allocation is valid, output is writable per caller contract.
    unsafe {
        pointer.write(compressor);
        output.write(pointer.cast());
    }
    0
}

/// Encode a block; return zero for capacity, maximum-size or allocation failure.
///
/// # Safety
/// `handle` must be live and exclusively used during the call. Input must be
/// readable for `input_size` bytes and output writable for `capacity` bytes;
/// input and output must not overlap. Successful destructive-mode compression
/// currently preserves input, as permitted by the original contract.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn wimlib_compress(
    input: *const c_void,
    input_size: usize,
    output: *mut c_void,
    capacity: usize,
    handle: *mut c_void,
) -> usize {
    if handle.is_null()
        || input_size == 0
        || capacity == 0
        || input.is_null()
        || output.is_null()
        || input_size > isize::MAX as usize
        || capacity > isize::MAX as usize
    {
        return 0;
    }
    // SAFETY: Caller supplies a live handle and readable input; no output
    // reference is made while the encoder borrows input.
    let compressor = unsafe { &mut *handle.cast::<FixedStrategyCompressor>() };
    // Reject oversize before reading potentially inaccessible input bytes.
    // FixedStrategyCompressor repeats this check, but slice creation requires it
    // at the pointer boundary as well.
    let maximum = compressor.maximum();
    if input_size > maximum {
        return 0;
    }
    // SAFETY: Input has the validated caller-provided readable extent.
    let input = unsafe { std::slice::from_raw_parts(input.cast::<u8>(), input_size) };
    let encoded = match compressor.compress_borrowed(input, capacity) {
        Ok(Some(bytes)) => bytes,
        _ => return 0,
    };
    // SAFETY: Encoder bounds returned length by capacity; buffers are disjoint.
    unsafe {
        std::ptr::copy_nonoverlapping(encoded.as_ptr(), output.cast::<u8>(), encoded.len());
    }
    encoded.len()
}

/// Release one compressor; a null pointer is ignored.
///
/// # Safety
/// Non-null `handle` must have been returned by this library and not yet freed.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn wimlib_free_compressor(handle: *mut c_void) {
    if handle.is_null() {
        return;
    }
    let pointer = handle.cast::<FixedStrategyCompressor>();
    // SAFETY: Caller transfers unique ownership of a matching allocation.
    unsafe {
        std::ptr::drop_in_place(pointer);
        crate::engine::allocation::free(pointer.cast());
    }
}
