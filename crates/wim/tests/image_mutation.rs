#![cfg(not(windows))]
use std::ptr;
use wim::ffi::{
    WimHandle, wimlib_add_empty_image, wimlib_create_new_wim, wimlib_delete_image, wimlib_free,
};

#[test]
fn empty_image_owns_a_real_security_table_and_null_root() {
    let mut handle = ptr::null_mut::<WimHandle>();
    // SAFETY: Output storage is valid and the live handle is freed once.
    unsafe {
        assert_eq!(wimlib_create_new_wim(0, &mut handle), 0);
        let mut index = 99;
        assert_eq!(
            wimlib_add_empty_image(handle, c"empty".as_ptr(), &mut index),
            0
        );
        assert_eq!(index, 1);
        let HandleImage::Empty(image) = &(&(*handle).images)[0] else {
            panic!("expected owned metadata");
        };
        let metadata = image.shared.lock().unwrap();
        let parsed = wim_format::metadata::Metadata::parse(&metadata.metadata).unwrap();
        assert!(parsed.nodes.is_empty());
        assert!(parsed.security.descriptors.is_empty());
        assert_eq!((*handle).header.image_count, 1);
        assert_eq!((*handle).xml.image_count(), 1);
        assert_eq!((*handle).xml.get_property(1, "DIRCOUNT"), Some("0"));
        drop(parsed);
        drop(metadata);
        wimlib_free(handle);
    }
}

#[test]
fn invalid_name_does_not_append_an_image_or_change_index() {
    let mut handle = ptr::null_mut();
    // SAFETY: Strings and output storage are valid; the handle is freed once.
    unsafe {
        assert_eq!(wimlib_create_new_wim(0, &mut handle), 0);
        let mut index = 123;
        assert_eq!(
            wimlib_add_empty_image(handle, c"invalid\x01".as_ptr(), &mut index),
            24
        );
        assert_eq!(index, 123);
        assert_eq!((*handle).header.image_count, 0);
        assert!((*handle).images.is_empty());
        assert_eq!((*handle).xml.image_count(), 0);
        wimlib_free(handle);
    }
}

#[test]
fn empty_image_deletion_aligns_xml_owned_images_and_boot_index() {
    let mut handle = ptr::null_mut();
    // SAFETY: Strings/output storage and the exclusively used handle are valid.
    unsafe {
        assert_eq!(wimlib_create_new_wim(0, &mut handle), 0);
        for name in [c"first", c"second", c"third"] {
            assert_eq!(
                wimlib_add_empty_image(handle, name.as_ptr(), ptr::null_mut()),
                0
            );
        }
        (*handle).header.boot_index = 3;
        (*handle).header.flags |= 4; // Original add/delete permit readonly handles.
        assert_eq!(wimlib_delete_image(handle, 2), 0);
        assert_eq!((*handle).header.boot_index, 2);
        assert_eq!((*handle).xml.name(2), Some("third"));
        assert_eq!(wimlib_delete_image(handle, -1), 0);
        assert_eq!((*handle).header.image_count, 0);
        assert_eq!((*handle).header.boot_index, 0);
        assert!((*handle).images.is_empty());
        assert_eq!((*handle).xml.image_count(), 0);
        assert_eq!(wimlib_delete_image(handle, -1), 0);
        wimlib_free(handle);
    }
}

use wim::engine::handles::HandleImage;
