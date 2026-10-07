use wim::ffi::{wimlib_create_new_wim, wimlib_free, wimlib_verify_wim};

#[test]
fn verify_rejects_invalid_arguments_and_accepts_empty_wim() {
    // SAFETY: Null input is explicitly validated; create publishes to writable storage.
    unsafe {
        assert_eq!(wimlib_verify_wim(std::ptr::null_mut(), 0), 24);
        let mut handle = std::ptr::null_mut();
        assert_eq!(wimlib_create_new_wim(0, &mut handle), 0);
        assert_eq!(wimlib_verify_wim(handle, 1), 24);
        assert_eq!(wimlib_verify_wim(handle, 0), 0);
        wimlib_free(handle);
    }
}
