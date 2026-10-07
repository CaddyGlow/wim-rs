// SPDX-License-Identifier: LGPL-2.1-or-later
//! Global diagnostics sinks and real native error/warning emission.
use crate::engine::TChar;
use std::ffi::{c_int, c_void};
use std::sync::Mutex;
struct State {
    show: bool,
    sink: usize,
    owned: bool,
}
static STATE: Mutex<State> = Mutex::new(State {
    show: false,
    sink: 0,
    owned: false,
});
unsafe extern "C" {
    fn fclose(stream: *mut c_void) -> c_int;
    fn fflush(stream: *mut c_void) -> c_int;
    fn fwrite(data: *const c_void, size: usize, count: usize, stream: *mut c_void) -> usize;
    #[cfg(not(windows))]
    fn fopen(path: *const TChar, mode: *const std::ffi::c_char) -> *mut c_void;
    #[cfg(unix)]
    static mut stderr: *mut c_void;
    #[cfg(unix)]
    static mut stdout: *mut c_void;
    #[cfg(windows)]
    fn __acrt_iob_func(index: u32) -> *mut c_void;
    #[cfg(windows)]
    fn _wfopen(path: *const TChar, mode: *const u16) -> *mut c_void;
}
fn lock() -> std::sync::MutexGuard<'static, State> {
    STATE
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
}
pub(crate) fn ensure_default_sink() {
    let mut state = lock();
    if state.sink == 0 {
        #[cfg(unix)]
        // SAFETY: stderr is a host C standard FILE, borrowed for process lifetime.
        {
            state.sink = unsafe { stderr } as usize;
        }
        #[cfg(windows)]
        // SAFETY: UCRT's index 2 is its standard error FILE.
        {
            state.sink = unsafe { __acrt_iob_func(2) } as usize;
        }
    }
}
/// Enable or suppress global errors and warnings without changing their sink.
#[unsafe(no_mangle)]
pub extern "C" fn wimlib_set_print_errors(show: bool) -> c_int {
    lock().show = show;
    0
}
/// Replace the diagnostic sink, closing only a previous library-owned sink.
/// A nonnull stream enables diagnostics; NULL disables them.
/// # Safety
/// `stream` must be NULL or a live host C FILE open for writing. The caller
/// retains ownership and must keep the stream live until replaced or cleaned up.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn wimlib_set_error_file(stream: *mut c_void) -> c_int {
    let mut state = lock();
    if state.owned {
        // SAFETY: Owned sinks are created by this module and released exactly once.
        unsafe {
            fclose(state.sink as *mut c_void);
        }
    }
    state.sink = stream as usize;
    state.show = !stream.is_null();
    state.owned = false;
    0
}
/// Open an append-mode diagnostic sink; failure leaves the previous sink intact.
/// # Safety
/// `path` must point to a readable terminated platform path. Windows logfile
/// sharing/CRT behavior requires separate validation.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn wimlib_set_error_file_by_name(path: *const TChar) -> c_int {
    if path.is_null() {
        return 24;
    }
    #[cfg(not(windows))]
    // SAFETY: Caller supplies the path; mode is a terminated static C string.
    let stream = unsafe { fopen(path, c"a".as_ptr()) };
    #[cfg(windows)]
    // SAFETY: Caller supplies the wide path; mode is a terminated wide string.
    let stream = unsafe { _wfopen(path, [b'a' as u16, 0].as_ptr()) };
    if stream.is_null() {
        return 47;
    }
    let mut state = lock();
    if state.owned {
        // SAFETY: Previous owned sink has not been released elsewhere.
        unsafe {
            fclose(state.sink as *mut c_void);
        }
    }
    state.sink = stream as usize;
    state.show = true;
    state.owned = true;
    0
}
pub(crate) fn cleanup() {
    // SAFETY: NULL installs no borrowed stream, and closes only an owned sink.
    unsafe {
        wimlib_set_error_file(std::ptr::null_mut());
    }
}
pub(crate) fn message(warning: bool, text: &[u8], with_errno: bool) {
    #[cfg(target_os = "linux")]
    // SAFETY: The errno location is calling-thread storage.
    let saved_errno = unsafe { *libc::__errno_location() };
    let state = lock();
    if !state.show || state.sink == 0 {
        return;
    }
    let sink = state.sink as *mut c_void;
    #[cfg(unix)]
    // SAFETY: stdout is the borrowed standard C stream.
    let output = unsafe { stdout };
    #[cfg(windows)]
    // SAFETY: UCRT index 1 is its standard output FILE.
    let output = unsafe { __acrt_iob_func(1) };
    let tag = if warning {
        b"\r[WARNING] ".as_slice()
    } else {
        b"\r[ERROR] ".as_slice()
    };
    // SAFETY: State holds the sink ownership lock, both FILEs and byte slices are live.
    unsafe {
        fflush(output);
        fwrite(tag.as_ptr().cast(), 1, tag.len(), sink);
        fwrite(text.as_ptr().cast(), 1, text.len(), sink);
    }
    #[cfg(target_os = "linux")]
    if with_errno && saved_errno != 0 {
        // SAFETY: strerror returns a readable terminated error string on this host.
        let description = unsafe { libc::strerror(saved_errno) };
        let fallback;
        let bytes = if description.is_null() {
            fallback = format!("unknown error (errno={saved_errno})").into_bytes();
            fallback.as_slice()
        } else {
            let raw = unsafe { std::ffi::CStr::from_ptr(description) }.to_bytes();
            if raw.len() < 64 {
                raw
            } else {
                fallback = format!("unknown error (errno={saved_errno})").into_bytes();
                fallback.as_slice()
            }
        };
        // SAFETY: Borrowed message/error strings and locked sink remain live.
        unsafe {
            fwrite(b": ".as_ptr().cast(), 1, 2, sink);
            fwrite(bytes.as_ptr().cast(), 1, bytes.len(), sink);
        }
    }
    #[cfg(not(target_os = "linux"))]
    let _ = with_errno;
    // SAFETY: Sink remains live under the ownership lock.
    unsafe {
        fwrite(b"\n".as_ptr().cast(), 1, 1, sink);
        fflush(sink);
    }
    #[cfg(target_os = "linux")]
    // SAFETY: Restore the calling thread's original errno after all stdio calls.
    unsafe {
        *libc::__errno_location() = saved_errno;
    }
}
