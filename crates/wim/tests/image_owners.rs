#![cfg(not(windows))]
use std::ptr;
use std::sync::Arc;
use wim::ffi::*;

#[test]
fn exported_image_owner_count_tracks_reexport_delete_and_handle_release() {
    // SAFETY: Every handle is created, accessed sequentially and freed once.
    unsafe {
        let mut source = ptr::null_mut();
        let mut destination = ptr::null_mut();
        let mut third = ptr::null_mut();
        for handle in [&mut source, &mut destination, &mut third] {
            assert_eq!(wimlib_create_new_wim(0, handle), 0);
        }
        assert_eq!(
            wimlib_add_empty_image(source, c"first".as_ptr(), ptr::null_mut()),
            0
        );
        assert_eq!(
            wimlib_add_empty_image(source, c"second".as_ptr(), ptr::null_mut()),
            0
        );
        assert_eq!(
            wimlib_export_image(source, 2, destination, ptr::null(), ptr::null(), 0),
            0
        );
        assert_eq!(
            wimlib_export_image(destination, 1, third, ptr::null(), ptr::null(), 0),
            0
        );
        assert_eq!(Arc::strong_count(&(&(*source).image_owners)[1]), 3);
        assert_eq!(Arc::strong_count(&(&(*source).image_owners)[0]), 1);
        assert_eq!(
            wimlib_export_image(
                source,
                1,
                destination,
                c"invalid\x01".as_ptr(),
                ptr::null(),
                0
            ),
            24
        );
        assert_eq!(Arc::strong_count(&(&(*source).image_owners)[0]), 1);
        assert_eq!((*destination).image_owners.len(), 1);
        assert_eq!(wimlib_delete_image(destination, 1), 0);
        assert_eq!(Arc::strong_count(&(&(*source).image_owners)[1]), 2);
        wimlib_free(third);
        assert_eq!(Arc::strong_count(&(&(*source).image_owners)[1]), 1);
        assert_eq!(wimlib_delete_image(source, 1), 0);
        assert_eq!((*source).image_owners.len(), 1);
        wimlib_free(destination);
        wimlib_free(source);
    }
}
