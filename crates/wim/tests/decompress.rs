use std::ptr;
use wim::engine::decompress::*;

#[test]
fn factory_preserves_failed_output_and_prioritizes_invalid_codec() {
    let sentinel = ptr::dangling_mut::<WimlibDecompressor>();
    let mut handle = sentinel;
    // SAFETY: Output is writable; null output is expressly supported by factory.
    unsafe {
        assert_eq!(wimlib_create_decompressor(0, 0, ptr::null_mut()), 16);
        assert_eq!(wimlib_create_decompressor(1, 0, &mut handle), 24);
        assert_eq!(handle, sentinel);
        assert_eq!(wimlib_create_decompressor(1, 65537, &mut handle), 24);
        assert_eq!(handle, sentinel);
    }
}

#[test]
fn oversized_request_does_not_dereference_null_buffers() {
    let mut handle = ptr::null_mut();
    // SAFETY: Factory output is writable; oversized requests never touch buffers.
    unsafe {
        assert_eq!(wimlib_create_decompressor(1, 1, &mut handle), 0);
        assert_eq!(
            wimlib_decompress(ptr::null(), usize::MAX, ptr::null_mut(), 2, handle),
            -2
        );
        wimlib_free_decompressor(handle);
        wimlib_free_decompressor(ptr::null_mut());
    }
}
