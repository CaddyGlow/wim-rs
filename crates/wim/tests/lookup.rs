use std::mem::{offset_of, size_of};
use wim::ffi::{WimResourceEntry, wimlib_iterate_lookup_table};

#[test]
fn resource_entry_matches_host_c_layout() {
    assert_eq!(size_of::<WimResourceEntry>(), 88);
    assert_eq!(offset_of!(WimResourceEntry, sha1_hash), 24);
    assert_eq!(offset_of!(WimResourceEntry, part_number), 44);
    assert_eq!(offset_of!(WimResourceEntry, flags), 52);
    assert_eq!(offset_of!(WimResourceEntry, raw_resource_offset_in_wim), 56);
}

#[test]
fn invalid_lookup_flags_are_rejected_before_handle_access() {
    // SAFETY: Invalid flags reject the request before pointers are inspected.
    assert_eq!(
        unsafe { wimlib_iterate_lookup_table(std::ptr::null_mut(), 1, None, std::ptr::null_mut()) },
        24
    );
}

#[cfg(unix)]
#[test]
fn pending_lookup_keeps_equal_content_files_separate_and_counts_hardlink_aliases() {
    use std::ffi::{CString, c_int, c_void};
    use wim::ffi::{
        WimResourceEntry, wimlib_add_image, wimlib_create_new_wim, wimlib_free,
        wimlib_iterate_lookup_table,
    };
    unsafe extern "C" fn collect(entry: *const WimResourceEntry, context: *mut c_void) -> c_int {
        // SAFETY: The iterator supplies a borrowed entry and our live result vector.
        let (entry, results) =
            unsafe { (&*entry, &mut *context.cast::<Vec<(u64, u32, [u8; 20])>>()) };
        if entry.flags & 2 == 0 {
            results.push((
                entry.uncompressed_size,
                entry.reference_count,
                entry.sha1_hash,
            ));
        }
        0
    }
    let root = std::env::temp_dir().join(format!("wim-pending-lookup-{}", std::process::id()));
    std::fs::create_dir(&root).unwrap();
    std::fs::write(root.join("first"), b"same").unwrap();
    std::fs::hard_link(root.join("first"), root.join("alias")).unwrap();
    std::fs::write(root.join("independent"), b"same").unwrap();
    let source = CString::new(root.as_os_str().as_encoded_bytes()).unwrap();
    let mut handle = std::ptr::null_mut();
    let mut results: Vec<(u64, u32, [u8; 20])> = Vec::new();
    // SAFETY: Output handle, source text, callback and result context stay live.
    unsafe {
        assert_eq!(wimlib_create_new_wim(0, &mut handle), 0);
        assert_eq!(
            wimlib_add_image(
                handle,
                source.as_ptr(),
                std::ptr::null(),
                std::ptr::null(),
                0
            ),
            0
        );
        assert_eq!(
            wimlib_iterate_lookup_table(
                handle,
                0,
                Some(collect),
                std::ptr::from_mut(&mut results).cast()
            ),
            0
        );
        wimlib_free(handle);
    }
    results.sort();
    assert_eq!(results, [(4, 1, [0; 20]), (4, 2, [0; 20])]);
    std::fs::remove_dir_all(root).unwrap();
}
