// SPDX-License-Identifier: LGPL-2.1-or-later
//! Standalone text-file loading translated from upstream textfile.c.
use crate::engine::{TChar, handles::path_from_pointer};
use std::ffi::c_int;
#[cfg(unix)]
use std::ffi::c_void;
use std::io::Read;
use wim_format::ParseError;
#[cfg(unix)]
unsafe extern "C" {
    static mut stdin: *mut c_void;
    fn fread(buffer: *mut c_void, size: usize, count: usize, stream: *mut c_void) -> usize;
    fn feof(stream: *mut c_void) -> c_int;
}
#[cfg(unix)]
fn stdin_nomem() -> c_int {
    crate::engine::diagnostics::message(false, b"Too much data sent on stdin!", false);
    24
}
fn file_message(prefix: &[u8], path: &std::path::Path, with_errno: bool) {
    let mut message = prefix.to_vec();
    message.extend_from_slice(path.as_os_str().as_encoded_bytes());
    message.push(b'"');
    crate::engine::diagnostics::message(false, &message, with_errno);
}
fn read_stdin() -> Result<Vec<u8>, c_int> {
    #[cfg(unix)]
    {
        let mut output = Vec::new();
        let mut capacity = 0usize;
        let mut filled = 0usize;
        loop {
            let next = capacity
                .checked_mul(2)
                .and_then(|n| n.checked_add(256))
                .ok_or_else(stdin_nomem)?;
            output
                .try_reserve_exact(next - output.len())
                .map_err(|_| stdin_nomem())?;
            output.resize(next, 0);
            capacity = next;
            // SAFETY: stdin is the host C FILE; writable buffer covers the requested extent.
            let count = unsafe {
                fread(
                    output.as_mut_ptr().add(filled).cast(),
                    1,
                    capacity - filled,
                    stdin,
                )
            };
            filled += count;
            if filled != capacity {
                // SAFETY: stdin remains a live host C FILE.
                if unsafe { feof(stdin) } == 0 {
                    crate::engine::diagnostics::message(false, b"Error reading stdin", true);
                    return Err(50);
                }
                output.truncate(filled);
                return Ok(output);
            }
        }
    }
    #[cfg(not(unix))]
    {
        let mut output = Vec::new();
        std::io::stdin().read_to_end(&mut output).map_err(|_| 50)?;
        Ok(output)
    }
}
unsafe fn read_input(path: *const TChar, dash_is_stdin: bool) -> Result<Vec<u8>, c_int> {
    // SAFETY: A nonnull path points to a readable terminated platform string.
    let standard_input =
        path.is_null() || dash_is_stdin && unsafe { *path == b'-' as TChar && *path.add(1) == 0 };
    if standard_input {
        return read_stdin();
    }
    // SAFETY: The caller supplies the terminated platform path.
    let path = if unsafe { *path } == 0 {
        // Unlike the WIM open API, the text loader passes empty paths to open().
        std::path::PathBuf::new()
    } else {
        // SAFETY: The caller supplies the terminated platform path.
        unsafe { path_from_pointer(path) }?
    };
    let mut file = std::fs::File::open(&path).map_err(|_| {
        file_message(b"Can't open \"", &path, true);
        47
    })?;
    let size = match file.metadata() {
        Ok(metadata) => metadata.len(),
        Err(_error) => {
            drop(file);
            #[cfg(target_os = "linux")]
            if let Some(errno) = _error.raw_os_error() {
                // SAFETY: __errno_location returns the calling thread's writable errno.
                unsafe {
                    *libc::__errno_location() = errno;
                }
            }
            file_message(b"Can't stat \"", &path, true);
            return Err(63);
        }
    };
    let size = usize::try_from(size).map_err(|_| {
        file_message(b"Not enough memory to read \"", &path, false);
        39
    })?;
    let mut bytes = Vec::new();
    bytes.try_reserve_exact(size).map_err(|_| {
        file_message(b"Not enough memory to read \"", &path, false);
        39
    })?;
    bytes.resize(size, 0);
    let result = file.read_exact(bytes.as_mut_slice());
    drop(file);
    if let Err(error) = result {
        let eof = error.kind() == std::io::ErrorKind::UnexpectedEof;
        #[cfg(target_os = "linux")]
        if let Some(errno) = if eof {
            Some(libc::EINVAL)
        } else {
            error.raw_os_error()
        } {
            // SAFETY: __errno_location returns the calling thread's writable errno.
            unsafe {
                *libc::__errno_location() = errno;
            }
        }
        file_message(b"Error reading \"", &path, true);
        return Err(if eof { 65 } else { 50 });
    }
    Ok(bytes)
}
fn translate(raw: &[u8]) -> Result<Vec<TChar>, ParseError> {
    let (utf16, offset) = if raw.starts_with(&[0xff, 0xfe]) {
        (true, 2)
    } else if raw.len() >= 2 && raw[0] <= 0x7f && raw[1] == 0 {
        (true, 0)
    } else if raw.starts_with(&[0xef, 0xbb, 0xbf]) {
        (false, 3)
    } else {
        (false, 0)
    };
    let bytes = &raw[offset..];
    #[cfg(not(windows))]
    {
        let bytes = if utf16 {
            if !bytes.len().is_multiple_of(2) {
                return Err(ParseError::InvalidUtf16String);
            }
            let units = bytes
                .chunks_exact(2)
                .map(|b| u16::from_le_bytes([b[0], b[1]]))
                .collect::<Vec<_>>();
            wim_format::platform_text::utf16_to_wtf8(&units)?
        } else {
            bytes.to_vec()
        };
        Ok(bytes.into_iter().map(|b| b as TChar).collect())
    }
    #[cfg(windows)]
    {
        if utf16 {
            Ok(bytes
                .chunks_exact(2)
                .map(|b| u16::from_le_bytes([b[0], b[1]]))
                .collect())
        } else {
            wim_format::platform_text::wtf8_to_utf16(bytes)
        }
    }
}
/// Internal path-list loader treats a literal dash as a disk filename.
/// # Safety
/// A nonnull path must be a readable terminated platform string.
pub(crate) unsafe fn load_pathlist_text(path: *const TChar) -> Result<Vec<TChar>, c_int> {
    // SAFETY: Caller supplies the platform string contract.
    let bytes = unsafe { read_input(path, false) }?;
    translate(bytes.as_slice()).map_err(|error| {
        #[cfg(target_os = "linux")]
        if matches!(
            error,
            ParseError::InvalidUtf16String | ParseError::InvalidUtf8String
        ) {
            // SAFETY: errno is calling-thread storage.
            unsafe {
                *libc::__errno_location() = libc::EILSEQ;
            }
        }
        error as c_int
    })
}
/// Internal capture configuration loader preserves public dash-as-stdin behavior.
/// # Safety
/// A nonnull path must be a readable terminated platform string.
#[cfg(any(unix, windows))]
pub(crate) unsafe fn load_capture_text(path: *const TChar) -> Result<Vec<TChar>, c_int> {
    // SAFETY: Caller supplies the platform string contract.
    let bytes = unsafe { read_input(path, true) }?;
    translate(bytes.as_slice()).map_err(|error| {
        #[cfg(target_os = "linux")]
        if matches!(
            error,
            ParseError::InvalidUtf16String | ParseError::InvalidUtf8String
        ) {
            // SAFETY: errno is calling-thread storage.
            unsafe {
                *libc::__errno_location() = libc::EILSEQ;
            }
        }
        error as c_int
    })
}
/// Load an entire text file as a C-owned terminated platform string.
/// NULL and `-` paths consume standard input; disk files use their initial
/// advertised byte length. BOM/ASCII-NUL detection selects UTF-16LE, otherwise
/// UTF-8 (an identity byte copy on Unix). Newlines and embedded NULs are retained.
/// The caller must release successful output with the matching host C runtime
/// `free`. Windows behavior remains unverified.
///
/// # Safety
/// `path` must be NULL or a readable terminated platform string. `output` and
/// `length` must point to writable storage. Standard input must remain a live
/// host C FILE, with no unsynchronized access by other code during this call.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn wimlib_load_text_file(
    path: *const TChar,
    output: *mut *mut TChar,
    length: *mut usize,
) -> c_int {
    if output.is_null() || length.is_null() {
        return 24;
    }
    // SAFETY: Path validity is the caller's responsibility.
    let bytes = match unsafe { read_input(path, true) } {
        Ok(bytes) => bytes,
        Err(error) => return error,
    };
    let text = match translate(bytes.as_slice()) {
        Ok(text) => text,
        Err(error) => {
            #[cfg(target_os = "linux")]
            if matches!(
                error,
                ParseError::InvalidUtf16String | ParseError::InvalidUtf8String
            ) {
                // SAFETY: __errno_location returns the calling thread's writable errno.
                unsafe {
                    *libc::__errno_location() = libc::EILSEQ;
                }
            }
            return error as c_int;
        }
    };
    let Some(size) = text
        .len()
        .checked_add(1)
        .and_then(|n| n.checked_mul(std::mem::size_of::<TChar>()))
    else {
        return 39;
    };
    // SAFETY: malloc accepts the checked allocation size, and failure is checked.
    let pointer = unsafe { crate::engine::allocation::malloc(size) }.cast::<TChar>();
    if pointer.is_null() {
        return 39;
    }
    // SAFETY: Allocation holds the complete text and terminator; outputs are writable.
    unsafe {
        std::ptr::copy_nonoverlapping(text.as_ptr(), pointer, text.len());
        pointer.add(text.len()).write(0);
        output.write(pointer);
        length.write(text.len());
    }
    0
}
#[cfg(all(test, not(windows)))]
mod tests {
    use super::*;
    fn bytes(raw: &[u8]) -> Result<Vec<u8>, ParseError> {
        translate(raw).map(|v| v.into_iter().map(|c| c as u8).collect())
    }
    #[test]
    fn bom_and_initial_ascii_nul_detection_match_original() {
        assert_eq!(bytes(b"\xef\xbb\xbfhello\r\n"), Ok(b"hello\r\n".to_vec()));
        assert_eq!(bytes(b"A\0B\0"), Ok(b"AB".to_vec()));
        assert_eq!(bytes(b"\xff\xfeA\0\0\0B\0"), Ok(b"A\0B".to_vec()));
        assert_eq!(bytes(b"\xfe\xffA"), Ok(b"\xfe\xffA".to_vec()));
    }
    #[test]
    fn unix_utf8_copy_keeps_invalid_bytes_and_embedded_nuls() {
        assert_eq!(
            bytes(b"\xff\xc0\x80\0\xed\xa0\x80"),
            Ok(b"\xff\xc0\x80\0\xed\xa0\x80".to_vec())
        );
        assert_eq!(bytes(b"\xff\xfe\0\xd8"), Ok(b"\xed\xa0\x80".to_vec()));
        assert_eq!(bytes(b"\xff\xfeA"), Err(ParseError::InvalidUtf16String));
    }
}
