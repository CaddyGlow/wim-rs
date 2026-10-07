#![cfg(target_os = "linux")]

use std::{
    ffi::{CString, c_int, c_void},
    io::{Read, Write},
    os::fd::{AsRawFd, IntoRawFd},
    os::unix::{ffi::OsStrExt, net::UnixStream},
};
use wim::ffi::{ProgressInfo, wimlib_extract_image_from_pipe_with_progress};

const INPUT: &[u8] = include_bytes!("fixtures/wim-format/pipable-resource.wim");

unsafe extern "C" fn stop_at_part(message: c_int, _: *mut ProgressInfo, _: *mut c_void) -> c_int {
    i32::from(message == 5)
}

#[test]
fn live_pipe_closes_owned_descriptor_and_leaves_unused_tail_unread() {
    for cancel in [false, true] {
        let target = std::env::temp_dir().join(format!(
            "wim-pipe-regression-{}-{cancel}",
            std::process::id()
        ));
        let (input, mut writer) = UnixStream::pair().unwrap();
        let mut observer = input.try_clone().unwrap();
        let thread = std::thread::spawn(move || {
            for fragment in INPUT.chunks(7) {
                writer.write_all(fragment).unwrap();
            }
        });
        let fd = input.into_raw_fd();
        let path = CString::new(target.as_os_str().as_bytes()).unwrap();
        // SAFETY: The API owns fd and borrows the live target and callback.
        let result = unsafe {
            wimlib_extract_image_from_pipe_with_progress(
                fd,
                std::ptr::null(),
                path.as_ptr(),
                0,
                cancel.then_some(stop_at_part),
                std::ptr::null_mut(),
            )
        };
        assert_eq!(result, if cancel { 76 } else { 0 });
        // SAFETY: fcntl validates an integer descriptor without dereferencing memory.
        assert_eq!(unsafe { libc::fcntl(fd, libc::F_GETFD) }, -1);
        let mut unread = Vec::new();
        observer.read_to_end(&mut unread).unwrap();
        thread.join().unwrap();
        assert!(!unread.is_empty(), "the footer must remain unread");
        if cancel {
            assert!(
                unread.len() > 1000,
                "cancellation must precede payload reads"
            );
        } else {
            let expected: Vec<_> = (0..300)
                .flat_map(|_| 0..=255u8)
                .chain(b"last chunk".iter().copied())
                .collect();
            assert_eq!(std::fs::read(target.join("payload.bin")).unwrap(), expected);
        }
        std::fs::remove_dir_all(target).unwrap();
    }
}

#[test]
fn invalid_public_flags_leave_caller_descriptor_open() {
    let (input, _writer) = UnixStream::pair().unwrap();
    // SAFETY: Flag rejection precedes use of the null text arguments and fd ownership.
    let result = unsafe {
        wimlib_extract_image_from_pipe_with_progress(
            input.as_raw_fd(),
            std::ptr::null(),
            std::ptr::null(),
            -1,
            None,
            std::ptr::null_mut(),
        )
    };
    assert_eq!(result, 24);
    assert!(unsafe { libc::fcntl(input.as_raw_fd(), libc::F_GETFD) } >= 0);
}

#[test]
fn image_policy_and_target_failures_consume_only_original_preflight() {
    let empty = CString::new("").unwrap();
    let target = CString::new(format!("/tmp/wim-pipe-policy-{}", std::process::id())).unwrap();
    let cases = [
        (1, std::ptr::null()),
        (1, empty.as_ptr()),
        (0x400000, std::ptr::null()),
        (0x1000000, std::ptr::null()),
        (0x400, target.as_ptr()),
        (0x40000, target.as_ptr()),
        (0x200000, target.as_ptr()),
    ];
    for (flags, target) in cases {
        let (input, mut writer) = UnixStream::pair().unwrap();
        let mut observer = input.try_clone().unwrap();
        writer.write_all(INPUT).unwrap();
        writer.shutdown(std::net::Shutdown::Write).unwrap();
        let fd = input.into_raw_fd();
        // SAFETY: Owned fd and live text arguments; target validation stops before output.
        assert_eq!(
            unsafe {
                wimlib_extract_image_from_pipe_with_progress(
                    fd,
                    std::ptr::null(),
                    target,
                    flags,
                    None,
                    std::ptr::null_mut(),
                )
            },
            24
        );
        assert_eq!(unsafe { libc::fcntl(fd, libc::F_GETFD) }, -1);
        let mut unread = Vec::new();
        observer.read_to_end(&mut unread).unwrap();
        // Golden original C observations in differential-policy-original.json.
        assert_eq!(INPUT.len() - unread.len(), 1288);
    }
}

unsafe extern "C" fn record_message(
    message: c_int,
    _: *mut ProgressInfo,
    context: *mut c_void,
) -> c_int {
    // SAFETY: The synchronous caller retains this Vec throughout extraction.
    unsafe { &mut *context.cast::<Vec<c_int>>() }.push(message);
    0
}

#[test]
fn matching_hash_metadata_frame_is_skipped_until_eof_without_writing_payload() {
    let archive = wim_format::archive::Archive::open(INPUT).unwrap();
    let payload = &archive.lookup.blobs[0];
    let resource = archive.lookup.resources[payload.resource_index].header;
    let flags_offset = resource.offset_in_wim as usize - 4;
    let mut bytes = INPUT.to_vec();
    bytes[flags_offset..flags_offset + 4]
        .copy_from_slice(&(u32::from(resource.flags) | 2).to_le_bytes());
    let target =
        std::env::temp_dir().join(format!("wim-pipe-metadata-frame-{}", std::process::id()));
    let (input, mut writer) = UnixStream::pair().unwrap();
    let mut observer = input.try_clone().unwrap();
    let thread = std::thread::spawn(move || {
        for fragment in bytes.chunks(7) {
            writer.write_all(fragment).unwrap();
        }
    });
    let fd = input.into_raw_fd();
    let path = CString::new(target.as_os_str().as_bytes()).unwrap();
    let mut messages = Vec::<c_int>::new();
    // SAFETY: The API owns fd and synchronously borrows the path and Vec context.
    let result = unsafe {
        wimlib_extract_image_from_pipe_with_progress(
            fd,
            std::ptr::null(),
            path.as_ptr(),
            0,
            Some(record_message),
            std::ptr::from_mut(&mut messages).cast(),
        )
    };
    assert_eq!(result, 65);
    assert_eq!(unsafe { *libc::__errno_location() }, libc::EINVAL);
    assert_eq!(unsafe { libc::fcntl(fd, libc::F_GETFD) }, -1);
    assert_eq!(messages, [0, 3, 3, 5]);
    assert!(target.is_dir());
    assert!(!target.join("payload.bin").exists());
    let mut unread = Vec::new();
    observer.read_to_end(&mut unread).unwrap();
    thread.join().unwrap();
    assert!(
        unread.is_empty(),
        "missing data must scan through the footer to EOF"
    );
    std::fs::remove_dir_all(target).unwrap();
}
