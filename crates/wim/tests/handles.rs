use std::ptr;
#[cfg(not(windows))]
use wim::ffi::wimlib_open_wim;
use wim::ffi::{WimHandle, wimlib_create_new_wim, wimlib_free};

#[test]
fn create_failure_preserves_output_and_parameter_precedence() {
    let marker = ptr::dangling_mut::<WimHandle>();
    let mut handle = marker;
    // SAFETY: Output is valid storage; null output explicitly exercises validation.
    unsafe {
        assert_eq!(wimlib_create_new_wim(-1, &mut handle), 16);
        assert_eq!(handle, marker);
        assert_eq!(wimlib_create_new_wim(-1, ptr::null_mut()), 24);
        wimlib_free(ptr::null_mut());
    }
}

#[test]
fn new_handles_keep_input_and_output_compression_separate() {
    for compression in 0..=3 {
        let mut handle = ptr::null_mut();
        // SAFETY: Output storage is live and each successful handle freed once.
        unsafe {
            assert_eq!(wimlib_create_new_wim(compression, &mut handle), 0);
            assert_eq!((*handle).header.chunk_size, 0);
            assert_eq!((*handle).header.flags, 0);
            assert_eq!((*handle).output_compression as i32, compression);
            assert_eq!((*handle).header.guid, [0; 16]);
            assert_eq!((*handle).xml.image_count(), 0);
            assert!((*handle).backing.is_none());
            wimlib_free(handle);
        }
    }
}

#[cfg(not(windows))]
#[test]
fn invalid_open_preserves_output() {
    let marker = ptr::dangling_mut::<WimHandle>();
    let mut handle = marker;
    // SAFETY: The string and output pointer are valid; null path is validated.
    unsafe {
        assert_eq!(wimlib_open_wim(ptr::null(), 0, &mut handle), 24);
        assert_eq!(wimlib_open_wim(c"".as_ptr(), 0, &mut handle), 24);
        assert_eq!(
            wimlib_open_wim(c"/missing-native-wim".as_ptr(), 8, &mut handle),
            24
        );
        assert_eq!(handle, marker);
    }
}
