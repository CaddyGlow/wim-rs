//! Live pipe extraction using sequential resource reads and incremental sinks.

#[cfg(target_os = "linux")]
use super::unix as platform;
#[cfg(windows)]
use super::windows as platform;
use crate::engine::collections::FallibleCollections as _;
use crate::engine::{ProgressCallback, TChar, WimHandle, progress::ProgressRegistration};
use std::{
    ffi::{c_int, c_void},
    io::{self, Read},
    ptr,
};
use wim_format::{
    ParseError,
    file_resource::FileReadError,
    pipable_image::with_image,
    pipable_read::{Frame, PipableReader},
};
#[cfg(windows)]
#[path = "win_error.rs"]
mod win_error;
#[cfg(windows)]
unsafe extern "C" {
    fn _errno() -> *mut c_int;
}
#[cfg(windows)]
#[link(name = "kernel32")]
unsafe extern "system" {
    fn ReadFile(
        handle: *mut c_void,
        bytes: *mut c_void,
        length: u32,
        count: *mut u32,
        overlapped: *mut c_void,
    ) -> i32;
    fn SetLastError(error: u32);
}
fn errno_pointer() -> *mut c_int {
    #[cfg(target_os = "linux")]
    // SAFETY: libc returns the calling thread's writable errno slot.
    unsafe {
        libc::__errno_location()
    }
    #[cfg(windows)]
    // SAFETY: The matching CRT returns the calling thread's writable errno slot.
    unsafe {
        _errno()
    }
}
#[cfg(windows)]
#[derive(Debug)]
struct WindowsReadError {
    errno: c_int,
}
#[cfg(windows)]
impl std::fmt::Display for WindowsReadError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "Windows input operation failed with errno {}",
            self.errno
        )
    }
}
#[cfg(windows)]
impl std::error::Error for WindowsReadError {}

struct Input(c_int);
impl Read for Input {
    fn read(&mut self, bytes: &mut [u8]) -> io::Result<usize> {
        #[cfg(windows)]
        {
            // Match win32_read(): bypass CRT text translation, read at most 1 MiB,
            // and keep descriptor ownership in the matching CRT until Input drops.
            // SAFETY: CRT validates fd; buffer and output count are writable.
            let handle = unsafe { libc::get_osfhandle(self.0) };
            let errno = if handle == -1 {
                // SAFETY: Matching CRT initialized errno on the invalid descriptor.
                Some(unsafe { *errno_pointer() })
            } else {
                let mut count = 0u32;
                // SAFETY: Converted live input handle, writable bounded buffer and count.
                let success = unsafe {
                    SetLastError(0);
                    ReadFile(
                        handle as *mut c_void,
                        bytes.as_mut_ptr().cast(),
                        bytes.len().min(1 << 20) as u32,
                        &mut count,
                        ptr::null_mut(),
                    )
                };
                if success != 0 {
                    return Ok(count as usize);
                }
                let error = io::Error::last_os_error().raw_os_error().unwrap_or(0) as u32;
                Some(win_error::to_errno(error))
            };
            let errno = errno.unwrap_or(libc::EIO);
            // SAFETY: Store the actual source-compatible CRT error for this operation.
            unsafe {
                *errno_pointer() = errno;
            }
            Err(io::Error::new(
                if errno == libc::EINTR {
                    io::ErrorKind::Interrupted
                } else {
                    io::ErrorKind::Other
                },
                WindowsReadError { errno },
            ))
        }
        #[cfg(target_os = "linux")]
        {
            // SAFETY: Buffer is writable and libc validates the caller descriptor.
            let count = unsafe { libc::read(self.0, bytes.as_mut_ptr().cast(), bytes.len()) };
            if count < 0 {
                Err(io::Error::last_os_error())
            } else {
                Ok(count as usize)
            }
        }
    }
}
impl Drop for Input {
    fn drop(&mut self) {
        // SAFETY: Original pipe API takes ownership after its initial flag checks.
        unsafe {
            libc::close(self.0);
        }
    }
}
struct Handle(*mut WimHandle);
impl Drop for Handle {
    fn drop(&mut self) {
        // SAFETY: This private handle is owned until the operation returns.
        unsafe {
            crate::engine::wimlib_free(self.0);
        }
    }
}
fn number(xml: &wim_format::xml::XmlInfo, image: i32, key: &[u8]) -> u64 {
    xml.get_property_bytes(image, key)
        .and_then(|text| std::str::from_utf8(text).ok())
        .and_then(|text| text.parse().ok())
        .unwrap_or(0)
}
fn status(error: FileReadError) -> c_int {
    match error {
        FileReadError::Format(error) => {
            if matches!(
                error,
                ParseError::UnexpectedEndOfFile
                    | ParseError::Decompression
                    | ParseError::InvalidChunkSize
            ) {
                // SAFETY: Writable thread-local errno is provided by libc.
                unsafe {
                    *errno_pointer() = libc::EINVAL;
                }
            }
            error as c_int
        }
        FileReadError::Io(error) => {
            // SAFETY: Restore the actual failed input operation's errno.
            unsafe {
                #[cfg(target_os = "linux")]
                let errno = error.raw_os_error().unwrap_or(libc::EIO);
                #[cfg(windows)]
                let errno = error
                    .get_ref()
                    .and_then(|error| error.downcast_ref::<WindowsReadError>())
                    .map_or(libc::EIO, |error| error.errno);
                *errno_pointer() = errno;
            }
            ParseError::Read as c_int
        }
    }
}
/// Extract one image from a live pipable WIM descriptor, taking descriptor ownership.
/// # Safety
/// Strings must be readable terminated platform text. Callback and context must
/// remain valid throughout this synchronous call.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn wimlib_extract_image_from_pipe_with_progress(
    fd: c_int,
    selector: *const TChar,
    target: *const TChar,
    flags: c_int,
    callback: Option<ProgressCallback>,
    context: *mut c_void,
) -> c_int {
    if flags as u32 & !super::PUBLIC_FLAGS != 0 {
        return ParseError::InvalidParam as c_int;
    }
    let result = (|| -> Result<(), FileReadError> {
        // Allocate only the actual outer handle before accepting descriptor ownership.
        // Upstream reads/validates the header before allocating retained image state.
        if crate::engine::wimlib_global_init(0) != 0 {
            return Err(ParseError::InvalidParam.into());
        }
        let storage = crate::engine::handles::HandleStorage::new().ok_or(ParseError::Nomem)?;
        let mut reader = PipableReader::new(Input(fd))?;
        let mut images = Vec::new();
        if reader.header().part_number == 1 {
            images
                .try_reserve(reader.header().image_count as usize)
                .map_err(|_| ParseError::Nomem)?;
            for index in 1..=reader.header().image_count {
                images
                    .try_push(crate::engine::handles::HandleImage::Source(index))
                    .map_err(|_| ParseError::Nomem)?;
            }
        }
        let owned_blobs = crate::engine::handles::new_owned_blob_table()?;
        #[cfg(windows)]
        let selector_storage = if selector.is_null() {
            None
        } else {
            let mut length = 0;
            // SAFETY: Caller supplies a readable terminated UTF-16 selector.
            while unsafe { *selector.add(length) } != 0 {
                length += 1;
            }
            // SAFETY: The terminated string establishes this readable range.
            Some(wim_format::platform_text::utf16_to_wtf8(unsafe {
                std::slice::from_raw_parts(selector, length)
            })?)
        };
        #[cfg(windows)]
        let selector = selector_storage.as_deref();
        #[cfg(target_os = "linux")]
        let selector = if selector.is_null() {
            None
        } else {
            // SAFETY: Caller supplies readable terminated text; Linux TChar is char.
            Some(unsafe { std::ffi::CStr::from_ptr(selector) }.to_bytes())
        };
        with_image(
            &mut reader,
            selector,
            |header, xml, index, metadata, reader| {
                let compression = header.validate_compression()?;
                let handle_value = WimHandle {
                    progress: std::cell::Cell::new(ProgressRegistration::new(callback, context)),
                    identity: crate::engine::handles::new_identity(),
                    header: header.clone(),
                    xml: crate::engine::handles::own_xml(xml)?,
                    backing: None,
                    lookup: None,
                    blob_index: crate::engine::blob_index::BlobIndex::new(64)?,
                    filename: None,
                    xml_strings: std::collections::HashMap::new(),
                    images,
                    image_owners: Vec::new(),
                    dirty_images: hashbrown::HashSet::new(),
                    image_deletion_occurred: false,
                    removed_blobs: hashbrown::HashSet::new(),
                    owned_blobs,
                    output_compression: compression,
                    output_chunk_size: header.chunk_size,
                    output_solid_compression: wim_format::Compression::Lzms,
                    output_solid_chunk_size: 67108864,
                };
                let mut pointer = ptr::null_mut();
                // SAFETY: Actual initialized state and writable local output are owned here.
                unsafe {
                    storage.publish(handle_value, &mut pointer);
                }
                let owner = Handle(pointer);
                // SAFETY: Private live handle is exclusively accessed before callbacks.
                let handle = unsafe { &mut *owner.0 };
                // Full-image policies are rejected after pipe preflight, before the
                // target/backend checks in upstream do_wimlib_extract_image().
                if flags as u32 & (0x400 | 0x0004_0000 | 0x0020_0000) != 0 {
                    return Err(ParseError::InvalidParam.into());
                }
                // SAFETY: Caller supplied the target string for this synchronous operation.
                #[cfg(windows)]
                let target_storage = unsafe { crate::engine::handles::path_from_pointer(target) }
                    .map_err(|code| {
                    ParseError::from_i32(code).unwrap_or(ParseError::InvalidParam)
                })?;
                #[cfg(windows)]
                let target = target_storage.as_path();
                #[cfg(target_os = "linux")]
                let target = unsafe { crate::engine::handles::borrowed_path_from_pointer(target) }
                    .map_err(|code| {
                        ParseError::from_i32(code).unwrap_or(ParseError::InvalidParam)
                    })?;
                if target.as_os_str().is_empty() {
                    return Err(ParseError::InvalidParam.into());
                }
                let flags = super::checked_flags(handle, flags as u32, true)?;
                let total_bytes = number(&handle.xml, index as i32, b"TOTALBYTES")
                    .wrapping_sub(number(&handle.xml, index as i32, b"HARDLINKBYTES"));
                let count = platform::required_stream_count(metadata, flags)?;
                let mut backend = platform::PreparedExtraction::prepare_image(
                    handle,
                    index as i32,
                    target,
                    flags,
                    metadata,
                    (total_bytes, count),
                )?;
                let mut part = (
                    u32::from(header.part_number),
                    u32::from(header.total_parts),
                    header.guid,
                );
                backend.part_begin(part.0, part.1, part.2)?;
                while !backend.all_streams_complete() {
                    match reader.next_frame(true)? {
                        Frame::Part(header) => {
                            let next = (
                                u32::from(header.part_number),
                                u32::from(header.total_parts),
                                header.guid,
                            );
                            if part != next {
                                backend.part_begin(next.0, next.1, next.2)?;
                                part = next;
                            }
                        }
                        Frame::Blob(blob)
                            if blob.flags & 2 == 0 && backend.needs_stream(&blob.hash) =>
                        {
                            let mut sink = backend.begin_stream(
                                blob.hash,
                                blob.uncompressed_size,
                                ParseError::InvalidResourceHash,
                            )?;
                            reader.read_resource(blob, false, flags & 2 != 0, |bytes| {
                                sink.consume(bytes)
                            })?;
                            sink.finish()?;
                        }
                        Frame::Blob(blob) => reader.skip_resource(blob)?,
                    }
                }
                backend.finish()?;
                Ok(())
            },
        )
    })();
    result.err().map_or(0, status)
}
/// Extract one image from a live pipe without progress callbacks.
/// # Safety
/// The input descriptor and terminated selector/target strings satisfy the
/// same contract as `wimlib_extract_image_from_pipe_with_progress`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn wimlib_extract_image_from_pipe(
    fd: c_int,
    selector: *const TChar,
    target: *const TChar,
    flags: c_int,
) -> c_int {
    // SAFETY: The caller's descriptor/text contract is forwarded unchanged.
    unsafe {
        wimlib_extract_image_from_pipe_with_progress(
            fd,
            selector,
            target,
            flags,
            None,
            ptr::null_mut(),
        )
    }
}
