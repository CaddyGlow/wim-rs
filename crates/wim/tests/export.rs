#![cfg(not(windows))]
use std::ptr;
use wim::ffi::{
    WimHandle, wimlib_add_empty_image, wimlib_create_new_wim, wimlib_export_image, wimlib_free,
};

unsafe fn new_source() -> *mut WimHandle {
    let mut handle = ptr::null_mut();
    // SAFETY: Caller owns the returned handle and its output pointer is valid.
    unsafe {
        assert_eq!(wimlib_create_new_wim(0, &mut handle), 0);
        assert_eq!(
            wimlib_add_empty_image(handle, c"Source".as_ptr(), ptr::null_mut()),
            0
        );
    }
    handle
}
#[test]
fn exported_pending_metadata_survives_source_release() {
    // SAFETY: Handles are exclusively accessed and each freed exactly once.
    unsafe {
        let source = new_source();
        let mut destination = ptr::null_mut();
        assert_eq!(wimlib_create_new_wim(0, &mut destination), 0);
        assert_eq!(
            wimlib_export_image(source, 1, destination, ptr::null(), ptr::null(), 0),
            0
        );
        wimlib_free(source);
        let HandleImage::Owned(image) = &(&(*destination).images)[0] else {
            panic!("expected independent owned image");
        };
        let metadata = image.pending.as_ref().unwrap().lock().unwrap();
        assert!(
            wim_format::metadata::Metadata::parse(&metadata.metadata)
                .unwrap()
                .nodes
                .is_empty()
        );
        drop(metadata);
        assert!(image.descriptor.is_none());
        assert_eq!((*destination).xml.name(1), Some("Source"));
        wimlib_free(destination);
    }
}
#[test]
fn repeated_and_round_trip_exports_reject_shared_image_identity() {
    // SAFETY: Handles are live, exclusively accessed, and freed once.
    unsafe {
        let source = new_source();
        let mut destination = ptr::null_mut();
        assert_eq!(wimlib_create_new_wim(0, &mut destination), 0);
        assert_eq!(
            wimlib_export_image(source, 1, destination, ptr::null(), ptr::null(), 0),
            0
        );
        assert_eq!(
            wimlib_export_image(
                source,
                1,
                destination,
                c"Different".as_ptr(),
                ptr::null(),
                0
            ),
            87
        );
        assert_eq!(
            wimlib_export_image(destination, 1, source, ptr::null(), ptr::null(), 2),
            87
        );
        assert_eq!((*destination).header.image_count, 1);
        wimlib_free(source);
        wimlib_free(destination);
    }
}
#[test]
fn invalid_export_name_leaves_destination_unchanged() {
    // SAFETY: The name and handles are valid; each handle is freed once.
    unsafe {
        let source = new_source();
        let mut destination = ptr::null_mut();
        assert_eq!(wimlib_create_new_wim(0, &mut destination), 0);
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
        assert_eq!((*destination).header.image_count, 0);
        assert!((*destination).images.is_empty());
        assert!((*destination).owned_blobs.is_empty());
        assert_eq!((*destination).xml.image_count(), 0);
        wimlib_free(source);
        wimlib_free(destination);
    }
}
#[test]
fn export_suppression_boot_and_wimboot_flags_change_owned_xml() {
    // SAFETY: Handles and text pointers are valid; each handle is freed once.
    unsafe {
        let source = new_source();
        let mut destination = ptr::null_mut();
        assert_eq!(wimlib_create_new_wim(0, &mut destination), 0);
        assert_eq!(
            wimlib_export_image(
                source,
                1,
                destination,
                c"Ignored".as_ptr(),
                c"Ignored".as_ptr(),
                1 | 2 | 4 | 16
            ),
            0
        );
        assert_eq!((*destination).xml.name(1), Some(""));
        assert_eq!((*destination).xml.description(1), None);
        assert_eq!((*destination).xml.get_property(1, "WIMBOOT"), Some("1"));
        assert_eq!((*destination).header.boot_index, 1);
        wimlib_free(source);
        wimlib_free(destination);
    }
}

#[test]
fn invalid_flags_and_empty_same_handle_exports_follow_validation_order() {
    let mut handle = ptr::null_mut();
    // SAFETY: Output storage is valid; optional pointers are null and supported.
    unsafe {
        assert_eq!(
            wimlib_export_image(
                ptr::null_mut(),
                1,
                ptr::null_mut(),
                ptr::null(),
                ptr::null(),
                32
            ),
            24
        );
        assert_eq!(wimlib_create_new_wim(0, &mut handle), 0);
        assert_eq!(
            wimlib_export_image(handle, -1, handle, ptr::null(), ptr::null(), 0),
            0
        );
        for flags in [32, 64, -1] {
            assert_eq!(
                wimlib_export_image(handle, -1, handle, ptr::null(), ptr::null(), flags),
                24
            );
        }
        assert_eq!(
            wimlib_export_image(handle, -1, handle, c"override".as_ptr(), ptr::null(), 0),
            24
        );
        wimlib_free(handle);
    }
}

#[test]
fn writing_one_export_owner_materializes_the_same_root_in_every_owner() {
    let source_path =
        std::env::temp_dir().join(format!("wim-shared-source-{}.wim", std::process::id()));
    let destination_path =
        std::env::temp_dir().join(format!("wim-shared-destination-{}.wim", std::process::id()));
    let source_name = std::ffi::CString::new(source_path.to_str().unwrap()).unwrap();
    let destination_name = std::ffi::CString::new(destination_path.to_str().unwrap()).unwrap();
    // SAFETY: Handles and strings are live, exclusively used and freed once.
    unsafe {
        let source = new_source();
        let mut destination = ptr::null_mut();
        assert_eq!(wimlib_create_new_wim(0, &mut destination), 0);
        assert_eq!(
            wimlib_export_image(source, 1, destination, ptr::null(), ptr::null(), 0),
            0
        );
        assert_eq!(
            wim::ffi::wimlib_write(source, source_name.as_ptr(), -1, 0, 1),
            0
        );
        let source_bytes = std::fs::read(&source_path).unwrap();
        let source_archive = wim_format::archive::Archive::open(&source_bytes).unwrap();
        let source_metadata = source_archive.read_metadata(1).unwrap();
        let HandleImage::Owned(image) = &(&(*destination).images)[0] else {
            panic!("expected exported owner");
        };
        {
            let shared = image.pending.as_ref().unwrap().lock().unwrap();
            assert_eq!(shared.metadata, source_metadata);
            assert_eq!(shared.hash, source_archive.lookup.metadata[0].hash);
            assert_eq!(
                wim_format::metadata::Metadata::parse(&shared.metadata)
                    .unwrap()
                    .nodes
                    .len(),
                1
            );
        }
        wimlib_free(source);
        assert_eq!(
            wim::ffi::wimlib_write(destination, destination_name.as_ptr(), -1, 0, 1),
            0
        );
        let destination_bytes = std::fs::read(&destination_path).unwrap();
        let destination_archive = wim_format::archive::Archive::open(&destination_bytes).unwrap();
        assert_eq!(
            destination_archive.read_metadata(1).unwrap(),
            source_metadata
        );
        assert_eq!(
            destination_archive.lookup.metadata[0].hash,
            source_archive.lookup.metadata[0].hash
        );
        wimlib_free(destination);
    }
    std::fs::remove_file(source_path).unwrap();
    std::fs::remove_file(destination_path).unwrap();
}

use wim::engine::handles::HandleImage;
