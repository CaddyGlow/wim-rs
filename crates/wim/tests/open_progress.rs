#![cfg(unix)]
use std::ffi::{CString, c_int, c_void};
use wim::ffi::{ProgressInfo, wimlib_free, wimlib_open_wim_with_progress};

#[derive(Default)]
struct Context {
    chunks: Vec<(u32, u64)>,
    stop: usize,
    status: c_int,
}
unsafe extern "C" fn callback(
    event: c_int,
    info: *mut ProgressInfo,
    context: *mut c_void,
) -> c_int {
    assert_eq!(event, 16);
    // SAFETY: Test passes a live context; event 16 identifies the integrity member.
    let context = unsafe { &mut *context.cast::<Context>() };
    let info = unsafe { (*info).integrity };
    assert!(!info.filename.is_null());
    context
        .chunks
        .push((info.completed_chunks, info.completed_bytes));
    if context.chunks.len() == context.stop {
        context.status
    } else {
        0
    }
}

#[test]
fn successful_open_reports_chunks_and_retains_borrowed_registration() {
    let path =
        std::fs::canonicalize("tests/fixtures/wim-format/integrity-small-chunks.wim").unwrap();
    let name = CString::new(path.as_os_str().as_encoded_bytes()).unwrap();
    let mut context = Context::default();
    let mut handle = std::ptr::null_mut();
    // SAFETY: Filename, output and caller context stay live until handle release.
    unsafe {
        assert_eq!(
            wimlib_open_wim_with_progress(
                name.as_ptr(),
                1,
                &mut handle,
                Some(callback),
                (&mut context as *mut Context).cast()
            ),
            0
        );
        assert_eq!(
            context.chunks,
            [(0, 0), (1, 128), (2, 256), (3, 384), (4, 397)]
        );
        assert!((*handle).progress.get().callback.is_some());
        wimlib_free(handle);
    }
}

#[test]
fn callback_abort_and_unknown_status_leave_output_unchanged() {
    let path =
        std::fs::canonicalize("tests/fixtures/wim-format/integrity-small-chunks.wim").unwrap();
    let name = CString::new(path.as_os_str().as_encoded_bytes()).unwrap();
    for (status, expected) in [(1, 76), (2, 77), (-1, 77)] {
        let mut context = Context {
            stop: 1,
            status,
            ..Context::default()
        };
        let mut handle = std::ptr::dangling_mut();
        // SAFETY: Output pointer storage and context are live; failed handle is never used.
        unsafe {
            assert_eq!(
                wimlib_open_wim_with_progress(
                    name.as_ptr(),
                    1,
                    &mut handle,
                    Some(callback),
                    (&mut context as *mut Context).cast()
                ),
                expected
            );
        }
        assert_eq!(handle, std::ptr::dangling_mut());
        assert_eq!(context.chunks, [(0, 0)]);
    }
}
