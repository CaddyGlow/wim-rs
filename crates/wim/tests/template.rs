#![cfg(target_os = "linux")]
use std::{ffi::CString, os::unix::ffi::OsStrExt, ptr};
use wim::ffi::{
    WimHandle, WimResourceEntry, wimlib_add_image, wimlib_create_new_wim, wimlib_free,
    wimlib_iterate_lookup_table, wimlib_reference_template_image, wimlib_write,
};

unsafe extern "C" fn collect(
    entry: *const WimResourceEntry,
    context: *mut std::ffi::c_void,
) -> i32 {
    // SAFETY: Synchronous lookup provides a live entry and our exclusive vector context.
    let (entry, hashes) = unsafe { (&*entry, &mut *context.cast::<Vec<[u8; 20]>>()) };
    if entry.flags & 2 == 0 {
        hashes.push(entry.sha1_hash);
    }
    0
}

#[test]
fn template_copies_real_checksum_without_opening_deleted_capture_source() {
    let root = std::env::temp_dir().join(format!("wim-template-regression-{}", std::process::id()));
    std::fs::create_dir(&root).unwrap();
    let source = root.join("source");
    std::fs::create_dir(&source).unwrap();
    let file = source.join("payload");
    std::fs::write(&file, b"incremental backup payload").unwrap();
    let source_text = CString::new(source.as_os_str().as_bytes()).unwrap();
    let output = CString::new(root.join("template.wim").as_os_str().as_bytes()).unwrap();
    let mut template: *mut WimHandle = ptr::null_mut();
    let mut destination: *mut WimHandle = ptr::null_mut();
    // SAFETY: Local outputs and borrowed strings remain live; each returned handle is freed below.
    unsafe {
        assert_eq!(wimlib_create_new_wim(0, &mut template), 0);
        assert_eq!(
            wimlib_add_image(
                template,
                source_text.as_ptr(),
                ptr::null(),
                ptr::null(),
                0x20
            ),
            0
        );
        assert_eq!(wimlib_write(template, output.as_ptr(), -1, 0, 1), 0);
        assert_eq!(wimlib_create_new_wim(0, &mut destination), 0);
        assert_eq!(
            wimlib_add_image(
                destination,
                source_text.as_ptr(),
                ptr::null(),
                ptr::null(),
                0x20
            ),
            0
        );
        std::fs::remove_file(file).unwrap();
        assert_eq!(
            wimlib_reference_template_image(destination, 1, template, 1, 0),
            0
        );
        let mut expected: Vec<[u8; 20]> = Vec::new();
        let mut actual: Vec<[u8; 20]> = Vec::new();
        assert_eq!(
            wimlib_iterate_lookup_table(
                template,
                0,
                Some(collect),
                (&mut expected as *mut Vec<[u8; 20]>).cast()
            ),
            0
        );
        assert_eq!(
            wimlib_iterate_lookup_table(
                destination,
                0,
                Some(collect),
                (&mut actual as *mut Vec<[u8; 20]>).cast()
            ),
            0
        );
        assert_eq!(actual, expected);
        assert_eq!(actual.len(), 1);
        assert_ne!(actual[0], [0; 20]);
        wimlib_free(destination);
        wimlib_free(template);
    }
    std::fs::remove_dir_all(root).unwrap();
}
