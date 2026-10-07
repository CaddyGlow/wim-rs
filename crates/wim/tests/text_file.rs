#![cfg(unix)]
use std::ffi::{CString, c_void};
use std::io::Write;
use std::os::unix::ffi::OsStrExt;
use wim::ffi::wimlib_load_text_file;
unsafe extern "C" {
    fn free(pointer: *mut c_void);
}
fn fixture(bytes: &[u8]) -> std::path::PathBuf {
    static NEXT: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
    let path = std::env::temp_dir().join(format!(
        "wim-text-test-{}-{}",
        std::process::id(),
        NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
    ));
    std::fs::File::create_new(&path)
        .unwrap()
        .write_all(bytes)
        .unwrap();
    path
}
#[test]
fn output_length_counts_embedded_nuls_and_c_free_releases_the_allocation() {
    let path = fixture(b"hello\0world\r\n");
    let filename = CString::new(path.as_os_str().as_bytes()).unwrap();
    let mut output = std::ptr::null_mut();
    let mut length = 0;
    // SAFETY: Live terminated filename and writable outputs are supplied.
    assert_eq!(
        unsafe { wimlib_load_text_file(filename.as_ptr(), &mut output, &mut length) },
        0
    );
    assert_eq!(length, 13);
    // SAFETY: Success returns length bytes plus a readable terminating NUL.
    let bytes = unsafe { std::slice::from_raw_parts(output.cast::<u8>(), length + 1) };
    assert_eq!(bytes, b"hello\0world\r\n\0");
    // SAFETY: Success allocates with the matching host C allocator.
    unsafe {
        free(output.cast());
    }
    std::fs::remove_file(path).unwrap();
}
#[test]
fn malformed_utf16_keeps_both_output_parameters_unchanged() {
    let path = fixture(b"\xff\xfeA");
    let filename = CString::new(path.as_os_str().as_bytes()).unwrap();
    let mut output = std::ptr::dangling_mut::<std::ffi::c_char>();
    let original = output;
    let mut length = 999;
    // SAFETY: Live terminated filename and writable outputs are supplied.
    assert_eq!(
        unsafe { wimlib_load_text_file(filename.as_ptr(), &mut output, &mut length) },
        30
    );
    assert_eq!(output, original);
    assert_eq!(length, 999);
    #[cfg(target_os = "linux")]
    // SAFETY: The calling thread's errno pointer is live and readable.
    assert_eq!(unsafe { *libc::__errno_location() }, libc::EILSEQ);
    std::fs::remove_file(path).unwrap();
}
