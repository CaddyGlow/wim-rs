#![cfg(unix)]
//! Filesystem sources become dispensable at real writer event 26.
use std::{
    ffi::{CStr, CString, c_void},
    path::PathBuf,
};
use wim::ffi::{
    ProgressInfo, WimHandle, wimlib_add_image, wimlib_create_new_wim, wimlib_free,
    wimlib_register_progress_function, wimlib_write,
};
struct State {
    events: Vec<i32>,
    remove: bool,
    abort: bool,
    paths: Vec<Vec<u8>>,
}
unsafe extern "C" fn progress(message: i32, info: *mut ProgressInfo, context: *mut c_void) -> i32 {
    // SAFETY: The test registers this state for the synchronous write.
    let state = unsafe { &mut *context.cast::<State>() };
    state.events.push(message);
    if message == 26 {
        // SAFETY: Event26 carries a live terminated source path for this callback.
        let path = unsafe { CStr::from_ptr((*info).done_with_file.path_to_file) };
        state.paths.push(path.to_bytes().to_vec());
        if state.remove {
            // SAFETY: The borrowed path is terminated and identifies disposable test input.
            assert_eq!(unsafe { libc::unlink(path.as_ptr()) }, 0);
        }
        if state.abort {
            return 1;
        }
    }
    0
}
fn exercise(abort: bool) {
    use std::os::unix::ffi::OsStrExt;
    let directory: PathBuf =
        std::env::temp_dir().join(format!("wim-done-{}-{abort}", std::process::id()));
    std::fs::create_dir(&directory).unwrap();
    let source = directory.join("source");
    std::fs::create_dir(&source).unwrap();
    let file = source.join("a");
    std::fs::write(&file, vec![b'0'; 65537]).unwrap();
    let target = directory.join("out.wim");
    let source_text = CString::new(source.as_os_str().as_bytes()).unwrap();
    let target_text = CString::new(target.as_os_str().as_bytes()).unwrap();
    let mut handle: *mut WimHandle = std::ptr::null_mut();
    let mut state = State {
        events: Vec::new(),
        remove: !abort,
        abort,
        paths: Vec::new(),
    };
    // SAFETY: All strings/output/context are live for these synchronous calls.
    unsafe {
        assert_eq!(wimlib_create_new_wim(0, &mut handle), 0);
        assert_eq!(
            wimlib_add_image(
                handle,
                source_text.as_ptr(),
                std::ptr::null(),
                std::ptr::null(),
                0
            ),
            0
        );
        wimlib_register_progress_function(
            handle,
            Some(progress),
            (&mut state as *mut State).cast(),
        );
        assert_eq!(
            wimlib_write(handle, target_text.as_ptr(), -1, 0x2000, 1),
            if abort { 76 } else { 0 }
        );
        wimlib_free(handle);
    }
    assert_eq!(state.paths, vec![file.as_os_str().as_bytes().to_vec()]);
    if abort {
        assert_eq!(state.events, vec![12, 12, 12, 26]);
        assert_eq!(std::fs::metadata(&target).unwrap().len(), 65745);
    } else {
        assert_eq!(state.events, vec![12, 12, 12, 26, 12, 13, 14]);
        assert!(!file.exists());
        let bytes = std::fs::read(&target).unwrap();
        let archive = wim_format::archive::Archive::open(&bytes).unwrap();
        let blob = archive
            .lookup
            .blobs
            .iter()
            .find(|b| b.size == 65537)
            .unwrap();
        assert_eq!(archive.read_blob(&blob.hash).unwrap(), vec![b'0'; 65537]);
    }
    std::fs::remove_dir_all(directory).unwrap();
}
#[test]
fn done_callback_can_remove_source_without_later_reads() {
    exercise(false);
}
#[test]
fn abort_at_done_preserves_real_payload_and_prevents_later_phases() {
    exercise(true);
}

#[test]
fn compressed_duplicate_finishes_before_pending_original_chunk() {
    use std::os::unix::ffi::OsStrExt;
    let directory = std::env::temp_dir().join(format!("wim-done-duplicate-{}", std::process::id()));
    std::fs::create_dir(&directory).unwrap();
    let source = directory.join("source");
    std::fs::create_dir(&source).unwrap();
    let a = source.join("a");
    let b = source.join("b");
    std::fs::write(&a, vec![b'0'; 65537]).unwrap();
    std::fs::write(&b, vec![b'0'; 65537]).unwrap();
    let target = directory.join("out.wim");
    let source_text = CString::new(source.as_os_str().as_bytes()).unwrap();
    let target_text = CString::new(target.as_os_str().as_bytes()).unwrap();
    let mut handle = std::ptr::null_mut();
    let mut state = State {
        events: Vec::new(),
        remove: true,
        abort: false,
        paths: Vec::new(),
    };
    // SAFETY: Strings, output and callback state are live until write returns.
    unsafe {
        assert_eq!(wimlib_create_new_wim(1, &mut handle), 0);
        assert_eq!(
            wimlib_add_image(
                handle,
                source_text.as_ptr(),
                std::ptr::null(),
                std::ptr::null(),
                0
            ),
            0
        );
        wimlib_register_progress_function(
            handle,
            Some(progress),
            (&mut state as *mut State).cast(),
        );
        assert_eq!(wimlib_write(handle, target_text.as_ptr(), -1, 0x2000, 1), 0);
        wimlib_free(handle);
    }
    assert_eq!(state.events, vec![12, 12, 12, 26, 26, 12, 13, 14]);
    assert_eq!(
        state.paths,
        vec![
            b.as_os_str().as_bytes().to_vec(),
            a.as_os_str().as_bytes().to_vec()
        ]
    );
    let bytes = std::fs::read(target).unwrap();
    let archive = wim_format::archive::Archive::open(&bytes).unwrap();
    assert_eq!(archive.lookup.blobs.len(), 1);
    assert_eq!(archive.lookup.blobs[0].reference_count, 2);
    assert_eq!(
        archive.read_blob(&archive.lookup.blobs[0].hash).unwrap(),
        vec![b'0'; 65537]
    );
    std::fs::remove_dir_all(directory).unwrap();
}

unsafe extern "C" fn collect_hash(
    entry: *const wim::ffi::WimResourceEntry,
    context: *mut c_void,
) -> i32 {
    // SAFETY: iterate_lookup supplies a live descriptor and the test's live vector.
    let (entry, hashes) = unsafe { (&*entry, &mut *context.cast::<Vec<[u8; 20]>>()) };
    if entry.flags & 2 == 0 {
        hashes.push(entry.sha1_hash);
    }
    0
}
#[test]
fn abort_hash_publication_matches_real_raw_and_compressed_read_end() {
    use std::os::unix::ffi::OsStrExt;
    for codec in [0, 1] {
        let directory =
            std::env::temp_dir().join(format!("wim-done-hash-{}-{codec}", std::process::id()));
        std::fs::create_dir(&directory).unwrap();
        let source = directory.join("source");
        std::fs::create_dir(&source).unwrap();
        let file = source.join("a");
        std::fs::write(&file, vec![b'0'; 65537]).unwrap();
        let target = directory.join("out.wim");
        let source_text = CString::new(source.as_os_str().as_bytes()).unwrap();
        let target_text = CString::new(target.as_os_str().as_bytes()).unwrap();
        let mut handle = std::ptr::null_mut();
        let mut state = State {
            events: Vec::new(),
            remove: false,
            abort: true,
            paths: Vec::new(),
        };
        let mut hashes = Vec::<[u8; 20]>::new();
        // SAFETY: All handle, text, descriptor and callback storage remain live.
        unsafe {
            assert_eq!(wimlib_create_new_wim(codec, &mut handle), 0);
            assert_eq!(
                wimlib_add_image(
                    handle,
                    source_text.as_ptr(),
                    std::ptr::null(),
                    std::ptr::null(),
                    0
                ),
                0
            );
            wimlib_register_progress_function(
                handle,
                Some(progress),
                (&mut state as *mut State).cast(),
            );
            assert_eq!(
                wimlib_write(handle, target_text.as_ptr(), -1, 0x2000, 1),
                76
            );
            wimlib_register_progress_function(handle, None, std::ptr::null_mut());
            assert_eq!(
                wim::ffi::wimlib_iterate_lookup_table(
                    handle,
                    0,
                    Some(collect_hash),
                    (&mut hashes as *mut Vec<[u8; 20]>).cast()
                ),
                0
            );
            assert_eq!(wim::ffi::wimlib_verify_wim(handle, 0), 0);
        }
        // Original C post-abort oracle: raw callback interrupts read-end; compressed has finished it.
        let expected = if codec == 0 {
            [0; 20]
        } else {
            [
                0x03, 0xc0, 0xff, 0x6a, 0x0a, 0x98, 0xdb, 0xcd, 0x1a, 0x6b, 0xb8, 0xd6, 0x89, 0x60,
                0x33, 0x36, 0xc5, 0xdd, 0x50, 0x0c,
            ]
        };
        assert_eq!(hashes, vec![expected]);
        std::fs::write(&file, vec![b'1'; 65537]).unwrap();
        // SAFETY: The retained handle remains live; verify callbacks are unregistered.
        unsafe {
            assert_eq!(
                wim::ffi::wimlib_verify_wim(handle, 0),
                if codec == 0 { 0 } else { 88 }
            );
            wimlib_free(handle);
        }
        std::fs::remove_dir_all(directory).unwrap();
    }
}
