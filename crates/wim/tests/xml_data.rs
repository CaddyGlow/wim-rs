#![cfg(unix)]
use std::ffi::c_void;
use wim::ffi::{wimlib_create_new_wim, wimlib_free, wimlib_get_xml_data};

#[test]
fn new_handle_xml_returns_empty_original_resource_and_validates_outputs() {
    // SAFETY: Creation and getter receive writable outputs; each handle remains live.
    unsafe {
        let mut handle = std::ptr::null_mut();
        assert_eq!(wimlib_create_new_wim(0, &mut handle), 0);
        let mut buffer = std::ptr::dangling_mut::<c_void>();
        let mut size = 123;
        assert_eq!(wimlib_get_xml_data(handle, &mut buffer, &mut size), 0);
        assert_eq!(size, 0);
        assert!(!buffer.is_null());
        unsafe extern "C" {
            fn free(pointer: *mut c_void);
        }
        free(buffer);
        assert_eq!(
            wimlib_get_xml_data(handle, std::ptr::null_mut(), std::ptr::null_mut()),
            24
        );
        wimlib_free(handle);
    }
}
