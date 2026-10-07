// SPDX-License-Identifier: LGPL-2.1-or-later
//! Borrowed native cancellation and C progress callbacks with the ABI payload layout.
use crate::engine::TChar;
use std::ffi::{c_int, c_void};
use wim_format::ParseError;

/// Filesystem extraction progress payload, matching the original public ABI.
#[repr(C)]
#[derive(Clone, Copy, Debug)]
pub struct ExtractProgress {
    /// Image.
    pub image: u32,
    /// Extract flags.
    pub extract_flags: u32,
    /// Wimfile name.
    pub wimfile_name: *const TChar,
    /// Image name.
    pub image_name: *const TChar,
    /// Target.
    pub target: *const TChar,
    /// Reserved.
    pub reserved: *const TChar,
    /// Total bytes.
    pub total_bytes: u64,
    /// Completed bytes.
    pub completed_bytes: u64,
    /// Total streams.
    pub total_streams: u64,
    /// Completed streams.
    pub completed_streams: u64,
    /// Part number.
    pub part_number: u32,
    /// Total parts.
    pub total_parts: u32,
    /// Guid.
    pub guid: [u8; 16],
    /// Current file count.
    pub current_file_count: u64,
    /// End file count.
    pub end_file_count: u64,
}

/// Per-image verification event payload.
#[repr(C)]
#[derive(Clone, Copy, Debug)]
pub struct VerifyImageProgress {
    /// Borrowed canonical source filename, or null for a new handle.
    pub wimfile: *const TChar,
    /// Current number of images.
    pub total_images: u32,
    /// One-based image being verified.
    pub current_image: u32,
}
/// Data-verification event payload.
#[repr(C)]
#[derive(Clone, Copy, Debug)]
pub struct VerifyStreamsProgress {
    /// Borrowed canonical source filename, or null for a new handle.
    pub wimfile: *const TChar,
    /// Distinct retained data blobs.
    pub total_streams: u64,
    /// Uncompressed bytes in all retained blobs.
    pub total_bytes: u64,
    /// Complete verified/read streams so far.
    pub completed_streams: u64,
    /// Uncompressed bytes read so far.
    pub completed_bytes: u64,
}
/// Data-writing event payload.
#[repr(C)]
#[derive(Clone, Copy, Debug)]
pub struct WriteStreamsProgress {
    /// Upper bound of uncompressed bytes to write.
    pub total_bytes: u64,
    /// Upper bound of distinct streams to write.
    pub total_streams: u64,
    /// Uncompressed bytes written so far.
    pub completed_bytes: u64,
    /// Streams written so far.
    pub completed_streams: u64,
    /// Actual compression worker count.
    pub num_threads: u32,
    /// Original numeric codec.
    pub compression_type: i32,
    /// Original backing source WIM count.
    pub total_parts: u32,
    /// Reserved/broken upstream completed part count, always zero.
    pub completed_parts: u32,
    /// Compressed bytes written so far.
    pub completed_compressed_bytes: u64,
}
/// Integrity checking/calculation event payload.
#[repr(C)]
#[derive(Clone, Copy, Debug)]
pub struct IntegrityProgress {
    /// Covered resource bytes.
    pub total_bytes: u64,
    /// Bytes checksummed so far.
    pub completed_bytes: u64,
    /// Individually checksummed chunks.
    pub total_chunks: u32,
    /// Completed chunks.
    pub completed_chunks: u32,
    /// Uncompressed coverage chunk size.
    pub chunk_size: u32,
    /// Borrowed filename for verification events.
    pub filename: *const TChar,
}
/// Split-part event payload; callbacks may redirect `part_name` on begin.
#[repr(C)]
#[derive(Clone, Copy, Debug)]
pub struct SplitProgress {
    /// Stored input metadata/content resource bytes.
    pub total_bytes: u64,
    /// Stored resource bytes copied so far.
    pub completed_bytes: u64,
    /// One-based current part.
    pub cur_part_number: u32,
    /// Total part count.
    pub total_parts: u32,
    /// Borrowed mutable filename; replacement storage must outlive part writing.
    pub part_name: *mut TChar,
}
/// Paths borrowed during the event emitted after an atomic overwrite rename.
#[repr(C)]
#[derive(Clone, Copy)]
pub struct RenameProgress {
    /// Temporary file name, no longer present after successful replacement.
    pub from: *const TChar,
    /// Original file name now containing the rebuilt archive.
    pub to: *const TChar,
}
/// Event 25 payload in the original public progress union.
/// This declaration describes the ABI; it does not enable the FUSE backend.
#[repr(C)]
#[derive(Clone, Copy)]
pub struct UnmountProgress {
    /// Directory being unmounted; borrowed for the callback.
    pub mountpoint: *const TChar,
    /// Mounted archive filename; borrowed for the callback.
    pub mounted_wim: *const TChar,
    /// One-based mounted image index.
    pub mounted_image: u32,
    /// Flags used when mounting the image.
    pub mount_flags: u32,
    /// Flags supplied to the unmount operation.
    pub unmount_flags: u32,
}
/// Event 26: the selected filesystem source is no longer needed by this write.
#[repr(C)]
#[derive(Clone, Copy)]
pub struct DoneWithFileProgress {
    /// Borrowed terminated source pathname, valid for this callback only.
    pub path_to_file: *const TChar,
}
/// Original public progress union on the supported 64-bit host ABI.
///
/// The storage preserves the complete 112-byte union, including members whose
/// operations are not yet implemented. Reading a field requires that its event
/// identifies that member. Current layout evidence covers Linux x86-64 only.
#[repr(C)]
#[derive(Clone, Copy)]
pub union ProgressInfo {
    /// Event 25; declaration only while the native FUSE backend remains unavailable.
    pub unmount: UnmountProgress,
    /// Event 26.
    pub done_with_file: DoneWithFileProgress,
    /// Filesystem capture scan error event 31.
    #[cfg(any(unix, windows))]
    pub handle_error: crate::engine::capture::HandleErrorProgress,
    /// Capture scan events 9, 10, and 11.
    #[cfg(any(unix, windows))]
    pub scan: crate::engine::capture::ScanProgress,
    /// Capture replacement event 23.
    #[cfg(any(unix, windows))]
    pub replace: crate::engine::capture::ReplaceProgress,
    /// Capture exclusion decision event 30.
    #[cfg(any(unix, windows))]
    pub file_exclusion: crate::engine::capture::FileExclusionProgress,
    /// Events 21 and 22.
    pub update: crate::engine::update::UpdateProgress,
    /// Events 0, 1, 3, 4, 5, 6, 7, and 8.
    pub extract: ExtractProgress,
    /// Event 27 or 28.
    pub verify_image: VerifyImageProgress,
    /// Event 29.
    pub verify_streams: VerifyStreamsProgress,
    /// Event 12.
    pub write_streams: WriteStreamsProgress,
    /// Event 16 or 17.
    pub integrity: IntegrityProgress,
    /// Event 19 or 20.
    pub split: SplitProgress,
    /// Event 15.
    pub rename: RenameProgress,
    /// Complete zeroable union extent, aligned for every supported member.
    pub storage: [u64; 14],
}
impl ProgressInfo {
    /// Initialize the complete union extent before populating one member.
    pub const fn zeroed() -> Self {
        Self { storage: [0; 14] }
    }
}
/// Original callback signature. Info and any contained strings are borrowed
/// only for the callback, except a caller-supplied replacement split filename.
/// The caller owns context storage and must not free/mutate the active WIM's
/// resource state from its callback. Replacing/unregistering progress is allowed.
pub type ProgressCallback = unsafe extern "C" fn(c_int, *mut ProgressInfo, *mut c_void) -> c_int;
/// A copied registration, with no ownership of callback code or caller context.
#[derive(Clone, Copy, Debug)]
pub struct ProgressRegistration {
    /// Callback function, or none to suppress progress.
    pub callback: Option<ProgressCallback>,
    /// Caller-owned context; retained without dereferencing or freeing it.
    pub context: *mut c_void,
    cancellation: Option<unsafe fn(*mut c_void) -> bool>,
}
impl Default for ProgressRegistration {
    fn default() -> Self {
        Self {
            callback: None,
            context: std::ptr::null_mut(),
            cancellation: None,
        }
    }
}
impl ProgressRegistration {
    /// Register a C callback and caller-owned context.
    pub fn new(callback: Option<ProgressCallback>, context: *mut c_void) -> Self {
        Self {
            callback,
            context,
            cancellation: None,
        }
    }

    /// Whether either native cancellation or a C progress callback is registered.
    pub(crate) fn is_registered(self) -> bool {
        self.callback.is_some() || self.cancellation.is_some()
    }

    /// Borrow a synchronous native cancellation closure.
    ///
    /// # Safety
    /// The registration must be removed before the closure goes out of scope
    /// or is accessed again. It may only be invoked on the calling thread.
    pub(crate) unsafe fn cancellation<F: FnMut() -> bool>(callback: &mut F) -> Self {
        unsafe fn invoke<F: FnMut() -> bool>(context: *mut c_void) -> bool {
            // SAFETY: The scoped owner keeps the exclusively borrowed closure live.
            let callback = unsafe { &mut *context.cast::<F>() };
            std::panic::catch_unwind(std::panic::AssertUnwindSafe(callback)).unwrap_or(true)
        }
        Self {
            callback: None,
            context: std::ptr::from_mut(callback).cast(),
            cancellation: Some(invoke::<F>),
        }
    }

    /// Dispatch one real lifecycle event and validate the caller's status.
    ///
    /// # Safety
    /// Registered code/context must remain valid and satisfy the callback's own
    /// contract. The selected event payload and strings must remain live until
    /// the call completes. No lock is held around the callback.
    pub unsafe fn call(self, event: c_int, info: *mut ProgressInfo) -> Result<(), ParseError> {
        if let Some(cancelled) = self.cancellation {
            // SAFETY: The registration's owner guarantees exclusive scoped access.
            return if unsafe { cancelled(self.context) } {
                Err(ParseError::AbortedByProgress)
            } else {
                Ok(())
            };
        }
        let Some(callback) = self.callback else {
            return Ok(());
        };
        // SAFETY: Callback lifetime, context and active payload are caller requirements.
        match unsafe { callback(event, info, self.context) } {
            0 => Ok(()),
            1 => Err(ParseError::AbortedByProgress),
            _ => Err(ParseError::UnknownProgressStatus),
        }
    }
}
/// Run an operation with a borrowed native cancellation closure, restoring the
/// prior registration before returning or propagating an operation panic.
pub(crate) fn with_cancellation<T>(
    handle: &mut crate::engine::handles::WimHandle,
    cancelled: &mut impl FnMut() -> bool,
    operation: impl FnOnce(&mut crate::engine::handles::WimHandle) -> T,
) -> T {
    // SAFETY: The operation is synchronous and the borrowed context cannot
    // escape: its registration is restored before return or unwind.
    let registration = unsafe { ProgressRegistration::cancellation(cancelled) };
    let old = handle.progress.replace(registration);
    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| operation(handle)));
    handle.progress.set(old);
    match result {
        Ok(result) => result,
        Err(panic) => std::panic::resume_unwind(panic),
    }
}

/// Store a borrowed callback/context until replaced or the handle is freed.
/// This setter touches only interior-mutable registration storage, so replacing
/// or unregistering progress during a callback does not borrow resource state.
///
/// # Safety
/// A nonnull handle must be live. Callback code and its caller-owned context
/// must remain valid whenever an operation may invoke the registration. A stream
/// verification phase snapshots its registration: replacing or unregistering it
/// does not release the old code/context until that phase returns. Callers
/// must not concurrently change the registration from another thread.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn wimlib_register_progress_function(
    handle: *mut crate::engine::handles::WimHandle,
    callback: Option<ProgressCallback>,
    context: *mut c_void,
) {
    // SAFETY: Caller guarantees the handle is live when nonnull.
    if let Some(handle) = unsafe { handle.as_ref() } {
        handle
            .progress
            .set(ProgressRegistration::new(callback, context));
    }
}

pub(crate) fn filename_buffer(
    path: Option<&std::path::Path>,
) -> Result<Option<Vec<TChar>>, ParseError> {
    let Some(path) = path else {
        return Ok(None);
    };
    let mut output = Vec::new();
    #[cfg(not(windows))]
    {
        use std::os::unix::ffi::OsStrExt;
        let bytes = path.as_os_str().as_bytes();
        output
            .try_reserve_exact(bytes.len() + 1)
            .map_err(|_| ParseError::Nomem)?;
        output.extend(bytes.iter().map(|&b| b as TChar));
    }
    #[cfg(windows)]
    {
        use std::os::windows::ffi::OsStrExt;
        let units = path.as_os_str().encode_wide();
        output
            .try_reserve_exact(units.clone().count() + 1)
            .map_err(|_| ParseError::Nomem)?;
        output.extend(units);
    }
    output.push(0);
    Ok(Some(output))
}

/// Rate limit used by upstream byte-based progress. The caller starts `next` at
/// zero; after each emitted event the next threshold is advanced. A terminal
/// threshold becomes u64::MAX, so no duplicate final event is emitted.
pub fn next_progress(completed: u64, total: u64, next: u64) -> u64 {
    if next < total {
        completed
            .saturating_add(total / 100)
            .min(completed.saturating_add(1 << 30))
            .min(total)
    } else {
        u64::MAX
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn selected_structures_match_original_64_bit_layout() {
        assert_eq!(std::mem::size_of::<ProgressInfo>(), 112);
        assert_eq!(std::mem::size_of::<ExtractProgress>(), 112);
        assert_eq!(std::mem::offset_of!(ExtractProgress, total_bytes), 40);
        assert_eq!(
            std::mem::offset_of!(ExtractProgress, current_file_count),
            96
        );
        assert_eq!(std::mem::align_of::<ProgressInfo>(), 8);
        assert_eq!(std::mem::size_of::<VerifyImageProgress>(), 16);
        assert_eq!(std::mem::size_of::<VerifyStreamsProgress>(), 40);
        assert_eq!(std::mem::size_of::<WriteStreamsProgress>(), 56);
        assert_eq!(std::mem::size_of::<IntegrityProgress>(), 40);
        assert_eq!(std::mem::size_of::<SplitProgress>(), 32);
        assert_eq!(std::mem::offset_of!(VerifyImageProgress, total_images), 8);
        assert_eq!(
            std::mem::offset_of!(VerifyStreamsProgress, completed_bytes),
            32
        );
    }
    unsafe extern "C" fn return_context_status(
        _event: c_int,
        _info: *mut ProgressInfo,
        context: *mut c_void,
    ) -> c_int {
        // SAFETY: The test passes a live aligned status integer.
        unsafe { *context.cast::<c_int>() }
    }

    #[test]
    fn native_cancellation_restores_previous_registration_after_operation_panics() {
        let mut handle =
            crate::engine::handles::create_handle(wim_format::Compression::None).unwrap();
        let mut status = 2;
        let old = ProgressRegistration::new(
            Some(return_context_status),
            std::ptr::from_mut(&mut status).cast(),
        );
        handle.progress.set(old);
        let mut calls = 0;
        let mut cancelled = || {
            calls += 1;
            false
        };
        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            with_cancellation(&mut handle, &mut cancelled, |handle| {
                let mut info = ProgressInfo::zeroed();
                // SAFETY: Scoped native registration and payload remain live.
                assert_eq!(unsafe { handle.progress.get().call(12, &mut info) }, Ok(()));
                panic!("operation panic after a native cancellation checkpoint");
            });
        }));
        assert!(result.is_err());
        assert_eq!(calls, 1);
        let mut info = ProgressInfo::zeroed();
        // SAFETY: The restored callback borrows the still-live status integer.
        assert_eq!(
            unsafe { handle.progress.get().call(12, &mut info) },
            Err(ParseError::UnknownProgressStatus)
        );
        handle.progress.set(Default::default());
    }
    #[test]
    fn dispatcher_preserves_context_and_maps_every_callback_status() {
        let mut info = ProgressInfo::zeroed();
        for (mut status, expected) in [
            (0, Ok(())),
            (1, Err(ParseError::AbortedByProgress)),
            (-1, Err(ParseError::UnknownProgressStatus)),
            (2, Err(ParseError::UnknownProgressStatus)),
        ] {
            let registration = ProgressRegistration::new(
                Some(return_context_status),
                (&mut status as *mut c_int).cast(),
            );
            // SAFETY: Context and complete union remain live during this callback.
            assert_eq!(unsafe { registration.call(29, &mut info) }, expected);
        }
    }
    #[test]
    fn byte_progress_threshold_tracks_original_terminal_and_gib_rules() {
        assert_eq!(next_progress(15, 63, 0), 15);
        assert_eq!(next_progress(63, 63, 15), 63);
        assert_eq!(next_progress(63, 63, 63), u64::MAX);
        assert_eq!(next_progress(0, 0, 0), u64::MAX);
        assert_eq!(next_progress(0, 1 << 40, 0), 1 << 30);
    }
}
