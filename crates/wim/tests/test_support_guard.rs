//! The unsupported optional generator flag must never dereference a fake source.
#![cfg(not(feature = "test-support"))]
use wim::ffi::{
    CaptureSource, wimlib_add_image, wimlib_add_image_multisource, wimlib_create_new_wim,
    wimlib_free,
};

#[test]
fn unavailable_test_generator_rejects_before_source_pointer_is_read() {
    let mut handle = std::ptr::null_mut();
    // SAFETY: Output is valid storage; the created handle remains live below.
    assert_eq!(unsafe { wimlib_create_new_wim(0, &mut handle) }, 0);
    // Original fuzz callers use a function pointer as their generated source.
    // This deliberately unreadable pointer detects accidental string decoding.
    let source = std::ptr::dangling();
    // SAFETY: Unsupported flags must be rejected without reading source;
    // all other arguments meet the public contract.
    let status = unsafe {
        wimlib_add_image(
            handle,
            source,
            std::ptr::null(),
            std::ptr::null(),
            0x0800_0000,
        )
    };
    assert_eq!(status, 24);
    // SAFETY: The original handle remains owned by this test after rollback.
    unsafe { wimlib_free(handle) };
}

#[test]
fn unavailable_test_generator_rejects_multisource_before_decoding_strings() {
    let mut handle = std::ptr::null_mut();
    // SAFETY: Output points to live storage for the new handle.
    assert_eq!(unsafe { wimlib_create_new_wim(0, &mut handle) }, 0);
    let source = CaptureSource {
        fs_source_path: std::ptr::dangling_mut(),
        wim_target_path: std::ptr::dangling_mut(),
        reserved: 0,
    };
    // SAFETY: The live source record contains deliberately unreadable strings;
    // unsupported generator flags must be rejected before decoding any of them.
    let status = unsafe {
        wimlib_add_image_multisource(
            handle,
            &source,
            1,
            std::ptr::dangling(),
            std::ptr::dangling(),
            0x0800_0000,
        )
    };
    assert_eq!(status, 24);
    // SAFETY: The handle is still owned by this test after rejected capture.
    unsafe { wimlib_free(handle) };
}
