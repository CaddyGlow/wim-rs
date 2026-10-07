#![cfg(unix)]
use std::{
    ffi::{CString, c_int, c_void},
    ptr,
};
use wim::ffi::{ProgressInfo, WimHandle, WimResourceEntry};
struct Context {
    handle: *mut WimHandle,
    stop: c_int,
    status: c_int,
    unregister: bool,
    events: Vec<c_int>,
}
unsafe extern "C" fn progress(event: c_int, info: *mut ProgressInfo, opaque: *mut c_void) -> c_int {
    // SAFETY: Test context is live and exclusively accessed by synchronous callbacks.
    let context = unsafe { &mut *opaque.cast::<Context>() };
    if event == 13 || event == 14 {
        assert!(info.is_null());
    } else {
        assert!(!info.is_null());
    }
    context.events.push(event);
    if context.unregister && context.events.len() == 1 {
        // SAFETY: Only interior-mutable registration is changed during the callback.
        unsafe {
            wim::ffi::wimlib_register_progress_function(context.handle, None, ptr::null_mut())
        };
    }
    if event == context.stop {
        context.status
    } else {
        0
    }
}
unsafe extern "C" fn resource(entry: *const WimResourceEntry, opaque: *mut c_void) -> c_int {
    // SAFETY: Both pointers are live for this synchronous iterator call.
    let entry = unsafe { &*entry };
    let rows = unsafe { &mut *opaque.cast::<Vec<(u64, [u8; 20])>>() };
    if entry.flags & 2 != 0 {
        rows.push((entry.uncompressed_size, entry.sha1_hash));
    }
    0
}
#[test]
fn metadata_abort_preserves_original_materialization_and_partial_file_boundaries() {
    let directory =
        std::env::temp_dir().join(format!("wim-write-progress-pending-{}", std::process::id()));
    std::fs::create_dir_all(&directory).unwrap();
    let target = directory.join("output.wim");
    let name = CString::new(target.to_str().unwrap()).unwrap();
    let mut handle = ptr::null_mut();
    // SAFETY: Owned handle, callback contexts and terminated paths stay live throughout.
    unsafe {
        assert_eq!(wim::ffi::wimlib_create_new_wim(0, &mut handle), 0);
        assert_eq!(
            wim::ffi::wimlib_add_empty_image(handle, ptr::null(), ptr::null_mut()),
            0
        );
        for (stop, size, metadata_size, zero_hash, events) in [
            (13, 208, 0, true, vec![13]),
            (14, 336, 0, false, vec![13, 14]),
        ] {
            let mut context = Context {
                handle,
                stop,
                status: 1,
                unregister: false,
                events: Vec::new(),
            };
            wim::ffi::wimlib_register_progress_function(
                handle,
                Some(progress),
                ptr::from_mut(&mut context).cast(),
            );
            assert_eq!(
                wim::ffi::wimlib_write(handle, name.as_ptr(), -1, 0x800, 1),
                76
            );
            assert_eq!(std::fs::metadata(&target).unwrap().len(), size);
            assert_eq!(context.events, events);
            let mut rows: Vec<(u64, [u8; 20])> = Vec::new();
            assert_eq!(
                wim::ffi::wimlib_iterate_lookup_table(
                    handle,
                    0,
                    Some(resource),
                    ptr::from_mut(&mut rows).cast()
                ),
                0
            );
            assert_eq!(rows.len(), 1);
            assert_eq!(rows[0].0, metadata_size);
            assert_eq!(rows[0].1 == [0; 20], zero_hash);
        }
        wim::ffi::wimlib_register_progress_function(handle, None, ptr::null_mut());
        wim::ffi::wimlib_free(handle);
    }
}
#[test]
fn unregister_during_initial_stream_event_keeps_snapshot_until_phase_returns() {
    let directory =
        std::env::temp_dir().join(format!("wim-write-progress-streams-{}", std::process::id()));
    std::fs::create_dir_all(&directory).unwrap();
    let source = directory.join("source.wim");
    let target = directory.join("output.wim");
    let bytes = include_bytes!("../../wim-format/tests/fixtures/xpress-resource.wim");
    std::fs::write(&source, bytes).unwrap();
    let source_name = CString::new(source.to_str().unwrap()).unwrap();
    let target_name = CString::new(target.to_str().unwrap()).unwrap();
    let mut handle = ptr::null_mut();
    // SAFETY: Context and file paths remain live until the synchronous writer returns.
    unsafe {
        assert_eq!(
            wim::ffi::wimlib_open_wim(source_name.as_ptr(), 0, &mut handle),
            0
        );
        let mut context = Context {
            handle,
            stop: 0,
            status: 0,
            unregister: true,
            events: Vec::new(),
        };
        wim::ffi::wimlib_register_progress_function(
            handle,
            Some(progress),
            ptr::from_mut(&mut context).cast(),
        );
        assert_eq!(
            wim::ffi::wimlib_write(handle, target_name.as_ptr(), -1, 0x801, 1),
            0
        );
        assert_eq!(context.events, [12, 12]);
        let mut output = ptr::null_mut();
        assert_eq!(
            wim::ffi::wimlib_open_wim(target_name.as_ptr(), 1, &mut output),
            0
        );
        assert_eq!(wim::ffi::wimlib_verify_wim(output, 0), 0);
        wim::ffi::wimlib_free(output);
        wim::ffi::wimlib_free(handle);
    }
    assert_eq!(std::fs::read(source).unwrap(), bytes);
}

#[test]
fn only_dirty_metadata_refreshes_statistics_before_writing() {
    use std::ffi::CStr;
    let directory =
        std::env::temp_dir().join(format!("wim-write-progress-stats-{}", std::process::id()));
    std::fs::create_dir_all(&directory).unwrap();
    let source = directory.join("source.wim");
    let target = directory.join("output.wim");
    let bytes = include_bytes!("../../wim-format/tests/fixtures/xpress-resource.wim");
    std::fs::write(&source, bytes).unwrap();
    let archive = wim_format::archive::Archive::open(bytes).unwrap();
    let metadata_bytes = archive.read_metadata(1).unwrap();
    let metadata = wim_format::metadata::Metadata::parse(&metadata_bytes).unwrap();
    let file = metadata
        .nodes
        .iter()
        .find(|node| node.entry.attributes & 0x10 == 0)
        .unwrap();
    let filename = String::from_utf16(
        &file
            .entry
            .name
            .chunks_exact(2)
            .map(|bytes| u16::from_le_bytes([bytes[0], bytes[1]]))
            .collect::<Vec<_>>(),
    )
    .unwrap();
    let old = CString::new(format!("/{filename}")).unwrap();
    let renamed = CString::new("/renamed").unwrap();
    let source_name = CString::new(source.to_str().unwrap()).unwrap();
    let target_name = CString::new(target.to_str().unwrap()).unwrap();
    let property = CString::new("FILECOUNT").unwrap();
    let stale = CString::new("9000").unwrap();
    let mut handle = ptr::null_mut();
    // SAFETY: Owned handle and terminated strings remain live; all calls are sequential.
    unsafe {
        assert_eq!(
            wim::ffi::wimlib_open_wim(source_name.as_ptr(), 0, &mut handle),
            0
        );
        assert_eq!(
            wim::ffi::wimlib_set_image_property(handle, 1, property.as_ptr(), stale.as_ptr()),
            0
        );
        assert_eq!(
            wim::ffi::wimlib_write(handle, target_name.as_ptr(), -1, 0x800, 1),
            0
        );
        assert_eq!(
            CStr::from_ptr(wim::ffi::wimlib_get_image_property(
                handle,
                1,
                property.as_ptr()
            ))
            .to_bytes(),
            b"9000"
        );
        assert_eq!(
            wim::ffi::wimlib_rename_path(handle, 1, old.as_ptr(), renamed.as_ptr()),
            0
        );
        assert_eq!(
            wim::ffi::wimlib_write(handle, target_name.as_ptr(), -1, 0x800, 1),
            0
        );
        assert_eq!(
            CStr::from_ptr(wim::ffi::wimlib_get_image_property(
                handle,
                1,
                property.as_ptr()
            ))
            .to_bytes(),
            b"1"
        );
        assert_eq!(
            wim::ffi::wimlib_set_image_property(handle, 1, property.as_ptr(), stale.as_ptr()),
            0
        );
        assert_eq!(
            wim::ffi::wimlib_write(handle, target_name.as_ptr(), -1, 0x800, 1),
            0
        );
        assert_eq!(
            CStr::from_ptr(wim::ffi::wimlib_get_image_property(
                handle,
                1,
                property.as_ptr()
            ))
            .to_bytes(),
            b"9000"
        );
        wim::ffi::wimlib_free(handle);
    }
    assert_eq!(std::fs::read(source).unwrap(), bytes);
}
