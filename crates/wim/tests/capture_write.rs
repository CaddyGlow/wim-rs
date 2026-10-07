#![cfg(unix)]
use std::{
    ffi::{CString, c_int, c_void},
    ptr,
};
use wim::ffi::{ProgressInfo, WimHandle};
use wim_format::archive::Archive;
#[derive(Default)]
struct Events {
    streams: Vec<(u64, u64, u64, u64)>,
    phases: Vec<c_int>,
}
unsafe extern "C" fn progress(event: c_int, info: *mut ProgressInfo, opaque: *mut c_void) -> c_int {
    // SAFETY: Owned context remains live for synchronous writing callbacks.
    let events = unsafe { &mut *opaque.cast::<Events>() };
    if event == 12 {
        // SAFETY: WRITE_STREAMS selects the corresponding union member.
        let p = unsafe { (*info).write_streams };
        events.streams.push((
            p.total_bytes,
            p.total_streams,
            p.completed_bytes,
            p.completed_streams,
        ));
    }
    if event == 13 || event == 14 {
        assert!(info.is_null());
        events.phases.push(event);
    }
    0
}
fn fixture(label: &str) -> (std::path::PathBuf, CString, CString) {
    let directory =
        std::env::temp_dir().join(format!("wim-capture-write-{}-{label}", std::process::id()));
    std::fs::create_dir_all(directory.join("source")).unwrap();
    let source = CString::new(directory.join("source").to_str().unwrap()).unwrap();
    let output = CString::new(directory.join("output.wim").to_str().unwrap()).unwrap();
    (directory, source, output)
}
unsafe fn capture(source: &CString, events: &mut Events) -> *mut WimHandle {
    let mut handle = ptr::null_mut();
    // SAFETY: Owned handle and source string remain live until free.
    unsafe {
        assert_eq!(wim::ffi::wimlib_create_new_wim(0, &mut handle), 0);
        assert_eq!(
            wim::ffi::wimlib_add_image(handle, source.as_ptr(), ptr::null(), ptr::null(), 0),
            0
        );
        wim::ffi::wimlib_register_progress_function(
            handle,
            Some(progress),
            ptr::from_mut(events).cast(),
        );
    }
    handle
}
#[test]
fn deferred_equal_size_streams_deduplicate_after_initial_progress() {
    let (directory, source, output) = fixture("dedup");
    std::fs::write(directory.join("source/a"), b"initial data").unwrap();
    std::fs::write(directory.join("source/b"), b"initial data").unwrap();
    let mut events = Events::default();
    // SAFETY: All owned context, handle and strings remain valid for these calls.
    unsafe {
        let handle = capture(&source, &mut events);
        assert_eq!(
            wim::ffi::wimlib_write(handle.cast(), output.as_ptr(), -1, 0, 1),
            0
        );
        wim::ffi::wimlib_free(handle);
    }
    assert_eq!(
        events.streams,
        [(24, 2, 0, 0), (24, 2, 12, 1), (12, 1, 12, 1)]
    );
    assert_eq!(events.phases, [13, 14]);
    let bytes = std::fs::read(directory.join("output.wim")).unwrap();
    let archive = Archive::open(&bytes).unwrap();
    assert_eq!(archive.lookup.blobs.len(), 1);
    assert_eq!(archive.lookup.blobs[0].reference_count, 2);
    assert_eq!(
        archive.read_blob(&archive.lookup.blobs[0].hash).unwrap(),
        b"initial data"
    );
    let metadata = archive.read_metadata(1).unwrap();
    assert!(wim_format::metadata::Metadata::parse(&metadata).is_ok());
    std::fs::remove_dir_all(directory).unwrap();
}
#[test]
fn solid_equal_size_streams_deduplicate_before_completed_progress() {
    let (directory, source, output) = fixture("solid-dedup");
    std::fs::write(directory.join("source/a"), b"initial data").unwrap();
    std::fs::write(directory.join("source/b"), b"initial data").unwrap();
    let mut events = Events::default();
    // SAFETY: All owned context, handle and strings remain valid for these calls.
    unsafe {
        let handle = capture(&source, &mut events);
        assert_eq!(
            wim::ffi::wimlib_write(handle.cast(), output.as_ptr(), -1, 4096, 1),
            0
        );
        wim::ffi::wimlib_free(handle);
    }
    assert_eq!(
        events.streams,
        [(24, 2, 0, 0), (12, 1, 0, 0), (12, 1, 12, 1)]
    );
    assert_eq!(events.phases, [13, 14]);
    let bytes = std::fs::read(directory.join("output.wim")).unwrap();
    let archive = Archive::open(&bytes).unwrap();
    assert_eq!(archive.lookup.blobs.len(), 1);
    assert_eq!(archive.lookup.blobs[0].reference_count, 2);
    assert_eq!(
        archive.read_blob(&archive.lookup.blobs[0].hash).unwrap(),
        b"initial data"
    );
    let metadata = archive.read_metadata(1).unwrap();
    assert!(wim_format::metadata::Metadata::parse(&metadata).is_ok());
    std::fs::remove_dir_all(directory).unwrap();
}
#[test]
fn shortened_capture_source_fails_after_initial_progress_before_metadata() {
    let (directory, source, output) = fixture("shortened");
    std::fs::write(directory.join("source/data"), b"initial data").unwrap();
    let mut events = Events::default();
    // SAFETY: Live handle/context and path strings remain owned throughout.
    unsafe {
        let handle = capture(&source, &mut events);
        std::fs::write(directory.join("source/data"), b"later").unwrap();
        assert_eq!(
            wim::ffi::wimlib_write(handle.cast(), output.as_ptr(), -1, 0, 1),
            88
        );
        assert_eq!(*libc::__errno_location(), libc::EINVAL);
        wim::ffi::wimlib_free(handle);
    }
    assert_eq!(events.streams, [(12, 1, 0, 0)]);
    assert!(events.phases.is_empty());
    assert_eq!(
        std::fs::metadata(directory.join("output.wim"))
            .unwrap()
            .len(),
        208
    );
    std::fs::remove_dir_all(directory).unwrap();
}

#[test]
fn emission_digest_detects_same_size_changes_after_prehashing() {
    struct Mutation {
        paths: [std::path::PathBuf; 2],
        changed: bool,
    }
    unsafe extern "C" fn mutate(
        event: c_int,
        info: *mut ProgressInfo,
        opaque: *mut c_void,
    ) -> c_int {
        // SAFETY: The callback context and selected progress member are live.
        let mutation = unsafe { &mut *opaque.cast::<Mutation>() };
        if event == 12 {
            // SAFETY: WRITE_STREAMS selects this union member.
            let progress = unsafe { (*info).write_streams };
            if !mutation.changed && progress.completed_bytes > 0 {
                use std::os::unix::fs::FileExt;
                for path in &mutation.paths {
                    let file = std::fs::OpenOptions::new().write(true).open(path).unwrap();
                    // Change unread bytes without shortening the scanned stream.
                    file.write_all_at(&[0xEE; 4096], 96 * 1024).unwrap();
                }
                mutation.changed = true;
            }
        }
        0
    }
    for codec in 0..=3 {
        for flags in [0, 4] {
            let (directory, source, output) = fixture(&format!("emission-{codec}-{flags}"));
            let paths = [directory.join("source/a"), directory.join("source/b")];
            for (index, path) in paths.iter().enumerate() {
                std::fs::write(path, vec![index as u8; 128 * 1024]).unwrap();
            }
            let mut mutation = Mutation {
                paths,
                changed: false,
            };
            let mut handle = ptr::null_mut();
            // SAFETY: Handle, strings and callback context remain live until free.
            unsafe {
                assert_eq!(wim::ffi::wimlib_create_new_wim(codec, &mut handle), 0);
                if codec != 0 {
                    assert_eq!(
                        wim::ffi::wimlib_set_output_chunk_size(handle.cast(), 32768),
                        0
                    );
                }
                assert_eq!(
                    wim::ffi::wimlib_add_image(
                        handle,
                        source.as_ptr(),
                        ptr::null(),
                        ptr::null(),
                        0
                    ),
                    0
                );
                wim::ffi::wimlib_register_progress_function(
                    handle,
                    Some(mutate),
                    ptr::from_mut(&mut mutation).cast(),
                );
                assert_eq!(
                    wim::ffi::wimlib_write(handle.cast(), output.as_ptr(), -1, flags, 1),
                    88,
                    "codec {codec}, flags {flags}"
                );
                wim::ffi::wimlib_free(handle);
            }
            assert!(mutation.changed);
            std::fs::remove_dir_all(directory).unwrap();
        }
    }
}
