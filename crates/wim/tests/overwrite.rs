#![cfg(unix)]
use std::{
    ffi::{CString, c_int, c_void},
    os::unix::fs::MetadataExt,
    ptr,
};
use wim::ffi::{ProgressInfo, WimHandle};
use wim_format::archive::Archive;
unsafe extern "C" fn stop_rename(
    event: c_int,
    info: *mut ProgressInfo,
    context: *mut c_void,
) -> c_int {
    // SAFETY: Test context is owned for this synchronous operation.
    let events = unsafe { &mut *context.cast::<Vec<c_int>>() };
    events.push(event);
    if event == 15 {
        // SAFETY: The rename event selects this union member and borrowed strings.
        let rename = unsafe { (*info).rename };
        assert!(!rename.from.is_null());
        assert!(!rename.to.is_null());
        1
    } else {
        0
    }
}
fn source(label: &str) -> (std::path::PathBuf, CString) {
    let path =
        std::env::temp_dir().join(format!("wim-overwrite-{}-{label}.wim", std::process::id()));
    let name = CString::new(path.to_str().unwrap()).unwrap();
    let mut handle = ptr::null_mut();
    // SAFETY: Test owns the live handle and NUL terminated filename.
    unsafe {
        assert_eq!(wim::ffi::wimlib_create_new_wim(0, &mut handle), 0);
        assert_eq!(
            wim::ffi::wimlib_add_empty_image(handle, ptr::null(), ptr::null_mut()),
            0
        );
        assert_eq!(wim::ffi::wimlib_write(handle, name.as_ptr(), -1, 0, 1), 0);
        wim::ffi::wimlib_free(handle);
    }
    (path, name)
}
#[test]
fn append_solid_image_uses_combined_offsets_for_multiple_resources() {
    let root = std::env::temp_dir().join(format!("wim-multi-solid-{}", std::process::id()));
    let first = root.join("first");
    let second = root.join("second");
    let third = root.join("third");
    std::fs::create_dir_all(&first).unwrap();
    std::fs::create_dir_all(&second).unwrap();
    std::fs::create_dir_all(&third).unwrap();
    std::fs::write(first.join("one"), vec![b'a'; 1024]).unwrap();
    std::fs::write(second.join("two"), vec![b'b'; 512]).unwrap();
    std::fs::write(third.join("three"), vec![b'c'; 256]).unwrap();
    let path = root.join("archive.wim");
    let name = CString::new(path.to_str().unwrap()).unwrap();
    let first_name = CString::new(first.to_str().unwrap()).unwrap();
    let second_name = CString::new(second.to_str().unwrap()).unwrap();
    let third_name = CString::new(third.to_str().unwrap()).unwrap();
    let copy_path = root.join("copy.wim");
    let copy_name = CString::new(copy_path.to_str().unwrap()).unwrap();
    let mut handle = ptr::null_mut();
    // SAFETY: Owned handles and terminated paths remain valid for each call.
    unsafe {
        assert_eq!(wim::ffi::wimlib_create_new_wim(2, &mut handle), 0);
        assert_eq!(
            wim::ffi::wimlib_add_image(handle, first_name.as_ptr(), ptr::null(), ptr::null(), 0),
            0
        );
        assert_eq!(
            wim::ffi::wimlib_write(handle, name.as_ptr(), -1, 0x1000, 1),
            0
        );
        wim::ffi::wimlib_free(handle);
        assert_eq!(wim::ffi::wimlib_open_wim(name.as_ptr(), 0, &mut handle), 0);
        assert_eq!(
            wim::ffi::wimlib_add_image(handle, second_name.as_ptr(), ptr::null(), ptr::null(), 0),
            0
        );
        assert_eq!(wim::ffi::wimlib_overwrite(handle, 0x1001, 1), 0);
        wim::ffi::wimlib_free(handle);
        assert_eq!(wim::ffi::wimlib_open_wim(name.as_ptr(), 0, &mut handle), 0);
        assert_eq!(wim::ffi::wimlib_verify_wim(handle, 0), 0);
        assert_eq!(
            wim::ffi::wimlib_add_image(handle, third_name.as_ptr(), ptr::null(), ptr::null(), 0),
            0
        );
        assert_eq!(wim::ffi::wimlib_overwrite(handle, 0x1001, 1), 0);
        wim::ffi::wimlib_free(handle);
        assert_eq!(wim::ffi::wimlib_open_wim(name.as_ptr(), 0, &mut handle), 0);
        assert_eq!(wim::ffi::wimlib_verify_wim(handle, 0), 0);
        assert_eq!(
            wim::ffi::wimlib_write(handle, copy_name.as_ptr(), -1, 0x1000, 1),
            0
        );
        wim::ffi::wimlib_free(handle);
        assert_eq!(
            wim::ffi::wimlib_open_wim(copy_name.as_ptr(), 0, &mut handle),
            0
        );
        assert_eq!(wim::ffi::wimlib_verify_wim(handle, 0), 0);
        wim::ffi::wimlib_free(handle);
    }
    let bytes = std::fs::read(path).unwrap();
    let archive = Archive::open(&bytes).unwrap();
    assert_eq!(archive.header.image_count, 3);
    assert_ne!(archive.header.integrity_table.size_in_wim, 0);
    archive.check_integrity().unwrap();
    assert_eq!(
        archive.lookup.resources.iter().filter(|r| r.solid).count(),
        3
    );
    let copied = std::fs::read(copy_path).unwrap();
    assert_eq!(Archive::open(&copied).unwrap().header.image_count, 3);
    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn append_empty_image_preserves_solid_resource_version_without_solid_flag() {
    let root = std::env::temp_dir().join(format!("wim-overwrite-solid-{}", std::process::id()));
    std::fs::create_dir_all(&root).unwrap();
    let payload = root.join("payload");
    std::fs::create_dir_all(&payload).unwrap();
    std::fs::write(payload.join("file"), b"retained solid resource payload").unwrap();
    let path = root.join("archive.wim");
    let name = CString::new(path.to_str().unwrap()).unwrap();
    let source = CString::new(payload.to_str().unwrap()).unwrap();
    let mut handle = ptr::null_mut();
    // SAFETY: The test owns the handles and terminated paths for every call.
    unsafe {
        assert_eq!(wim::ffi::wimlib_create_new_wim(2, &mut handle), 0);
        assert_eq!(
            wim::ffi::wimlib_add_image(handle, source.as_ptr(), ptr::null(), ptr::null(), 0),
            0
        );
        assert_eq!(
            wim::ffi::wimlib_write(handle, name.as_ptr(), -1, 0x1000, 1),
            0
        );
        wim::ffi::wimlib_free(handle);
        assert_eq!(wim::ffi::wimlib_open_wim(name.as_ptr(), 0, &mut handle), 0);
        assert_eq!(
            wim::ffi::wimlib_add_empty_image(handle, ptr::null(), ptr::null_mut()),
            0
        );
        assert_eq!(wim::ffi::wimlib_overwrite(handle, 0x4900, 1), 0);
        wim::ffi::wimlib_free(handle);
        assert_eq!(wim::ffi::wimlib_open_wim(name.as_ptr(), 0, &mut handle), 0);
        assert_eq!(wim::ffi::wimlib_verify_wim(handle, 0), 0);
        wim::ffi::wimlib_free(handle);
    }
    let bytes = std::fs::read(&path).unwrap();
    let archive = Archive::open(&bytes).unwrap();
    assert_eq!(archive.header.version, 0xe00);
    assert_eq!(archive.header.image_count, 2);
    assert!(
        archive
            .lookup
            .resources
            .iter()
            .any(|resource| resource.solid)
    );
    std::fs::remove_dir_all(root).unwrap();
}
#[test]
fn append_updates_xml_without_replacing_original_inode_or_blob_table() {
    let (path, name) = source("append");
    let before = std::fs::read(&path).unwrap();
    let old_table = Archive::open(&before).unwrap().header.blob_table;
    let inode = std::fs::metadata(&path).unwrap().ino();
    let mut handle = ptr::null_mut();
    let key = CString::new("DESCRIPTION").unwrap();
    let value = CString::new("append description").unwrap();
    // SAFETY: Owned handle and terminated strings remain live.
    unsafe {
        assert_eq!(wim::ffi::wimlib_open_wim(name.as_ptr(), 0, &mut handle), 0);
        assert_eq!(
            wim::ffi::wimlib_set_image_property(handle.cast(), 1, key.as_ptr(), value.as_ptr()),
            0
        );
        assert_eq!(wim::ffi::wimlib_overwrite(handle, 0, 1), 0);
        wim::ffi::wimlib_free(handle);
    }
    let bytes = std::fs::read(&path).unwrap();
    let archive = Archive::open(&bytes).unwrap();
    assert_eq!(archive.header.blob_table, old_table);
    assert_eq!(std::fs::metadata(&path).unwrap().ino(), inode);
    assert_eq!(
        archive
            .xml()
            .unwrap()
            .get_property(1, "DESCRIPTION")
            .unwrap(),
        "append description"
    );
    std::fs::remove_file(path).unwrap();
}
#[test]
fn abort_after_atomic_rename_keeps_committed_rebuild_and_reports_abort() {
    let (path, name) = source("rebuild");
    let inode = std::fs::metadata(&path).unwrap().ino();
    let mut handle: *mut WimHandle = ptr::null_mut();
    let mut events = Vec::<c_int>::new();
    // SAFETY: Handle and callback context remain live throughout synchronous overwrite.
    unsafe {
        assert_eq!(wim::ffi::wimlib_open_wim(name.as_ptr(), 0, &mut handle), 0);
        assert_eq!(
            wim::ffi::wimlib_add_empty_image(handle, ptr::null(), ptr::null_mut()),
            0
        );
        wim::ffi::wimlib_register_progress_function(
            handle,
            Some(stop_rename),
            ptr::from_mut(&mut events).cast(),
        );
        assert_eq!(wim::ffi::wimlib_overwrite(handle, 64, 1), 76);
        wim::ffi::wimlib_free(handle);
    }
    assert_eq!(events, vec![13, 14, 15]);
    assert_ne!(std::fs::metadata(&path).unwrap().ino(), inode);
    let bytes = std::fs::read(&path).unwrap();
    assert_eq!(Archive::open(&bytes).unwrap().header.image_count, 2);
    std::fs::remove_file(path).unwrap();
}

#[test]
fn append_conflicting_lock_returns_already_locked_without_changing_file() {
    use std::os::fd::AsRawFd;
    unsafe extern "C" {
        fn flock(fd: c_int, operation: c_int) -> c_int;
    }
    let (path, name) = source("locked");
    let before = std::fs::read(&path).unwrap();
    let file = std::fs::File::open(&path).unwrap();
    let mut handle = ptr::null_mut();
    // SAFETY: Live OS descriptor and owned archive remain valid until unlock.
    unsafe {
        assert_eq!(flock(file.as_raw_fd(), 2 | 4), 0);
        assert_eq!(wim::ffi::wimlib_open_wim(name.as_ptr(), 0, &mut handle), 0);
        assert_eq!(wim::ffi::wimlib_overwrite(handle, 0, 1), 1);
        wim::ffi::wimlib_free(handle);
        assert_eq!(flock(file.as_raw_fd(), 8), 0);
    }
    assert_eq!(std::fs::read(&path).unwrap(), before);
    std::fs::remove_file(path).unwrap();
}
