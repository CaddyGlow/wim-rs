use std::ffi::c_void;
use wim::ffi::{wimlib_compress, wimlib_create_compressor, wimlib_free_compressor};

#[test]
fn compressor_reuses_handle_after_capacity_and_maximum_failures() {
    for codec in 1..=3 {
        let mut handle = std::ptr::null_mut();
        let input = vec![b'a'; 4096];
        let mut output = vec![0; 8192];
        // SAFETY: Live disjoint buffers and a handle released exactly once.
        unsafe {
            assert_eq!(
                wimlib_create_compressor(codec, input.len(), 50, &mut handle),
                0
            );
            assert_eq!(
                wimlib_compress(
                    input.as_ptr().cast(),
                    0,
                    output.as_mut_ptr().cast(),
                    output.len(),
                    handle
                ),
                0
            );
            assert_eq!(
                wimlib_compress(
                    input.as_ptr().cast(),
                    input.len() + 1,
                    output.as_mut_ptr().cast(),
                    output.len(),
                    handle
                ),
                0
            );
            assert_eq!(
                wimlib_compress(
                    input.as_ptr().cast(),
                    input.len(),
                    output.as_mut_ptr().cast(),
                    0,
                    handle
                ),
                0
            );
            let size = wimlib_compress(
                input.as_ptr().cast(),
                input.len(),
                output.as_mut_ptr().cast(),
                output.len(),
                handle,
            );
            assert!(size > 0);
            let mut decoder = ms_compress::context::Decompressor::new(codec, input.len()).unwrap();
            let mut decoded = vec![0; input.len()];
            decoder.decompress(&output[..size], &mut decoded).unwrap();
            assert_eq!(decoded, input);
            wimlib_free_compressor(handle);
            wimlib_free_compressor(std::ptr::null_mut());
        }
    }
}

#[test]
fn compressor_factory_preserves_output_pointer_on_validation_failure() {
    let sentinel = std::ptr::dangling_mut::<c_void>();
    for (codec, size, level, expected) in [
        (0, 4096, 50, 16),
        (1, 0, 50, 24),
        (1, 65537, 50, 24),
        (1, 4096, 0x1000000, 24),
    ] {
        let mut handle = sentinel;
        // SAFETY: Failure must not inspect the output pointer's initial value.
        assert_eq!(
            unsafe { wimlib_create_compressor(codec, size, level, &mut handle) },
            expected
        );
        assert_eq!(handle, sentinel);
    }
}
