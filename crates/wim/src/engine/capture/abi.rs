//! Capture command validation and original callback ABI.

use super::{CaptureConfig, CapturePlan, scan_source};
use crate::engine::collections::FallibleSet as _;
use crate::engine::{ProgressInfo, TChar, WimHandle};
use std::{ffi::c_int, path::Path};
use wim_format::{Header, ParseError, metadata::Metadata, metadata_write::OwnedMetadata};

/// Original capture scan progress union member.
#[repr(C)]
#[derive(Clone, Copy)]
pub struct ScanProgress {
    /// Top-level source path.
    pub source: *const TChar,
    /// Current scanned path.
    pub current_path: *const TChar,
    /// Original status enum value.
    pub status: c_int,
    /// Image target on begin/end, symlink target on fixup notifications.
    pub target: *const TChar,
    /// Scanned directories.
    pub directories: u64,
    /// Scanned non-directories.
    pub nondirectories: u64,
    /// Unique stream bytes.
    pub bytes: u64,
}
/// Bidirectional exclusion progress payload (message 30).
#[repr(C)]
#[derive(Clone, Copy)]
pub struct FileExclusionProgress {
    /// Current filesystem path.
    pub path: *const TChar,
    /// Callback may set true to exclude this node and its subtree.
    pub will_exclude: bool,
}
/// Replacement notification payload (message 23).
#[repr(C)]
#[derive(Clone, Copy)]
pub struct ReplaceProgress {
    /// Canonical image path to the replaced non-directory entry.
    pub path_in_wim: *const TChar,
}
/// Bidirectional filesystem scan error notification (message 31).
#[repr(C)]
#[derive(Clone, Copy)]
pub struct HandleErrorProgress {
    /// Filesystem path on which this scan level failed.
    pub path: *const TChar,
    /// Original library error code.
    pub error_code: c_int,
    /// Callback may ignore this error and omit the failed subtree.
    pub will_ignore: bool,
}
/// Validate capture flags and apply original verbosity/fixup defaults.
pub(crate) fn normalize_flags(
    header: &Header,
    image_root: bool,
    mut flags: c_int,
) -> Result<c_int, ParseError> {
    if flags & !0x1fff7 != 0 {
        return Err(ParseError::InvalidParam);
    }
    if flags & (1 | 0x8000) != 0 {
        return Err(ParseError::Unsupported);
    }
    #[cfg(windows)]
    if flags & (2 | 0x10) != 0 {
        return Err(ParseError::Unsupported);
    }
    if flags & 4 != 0 {
        flags |= 0x80;
    }
    if flags & 0x300 == 0x300 {
        return Err(ParseError::InvalidParam);
    }
    if flags & 0x300 == 0 && image_root && (header.flags & 0x80 != 0 || header.image_count == 1) {
        flags |= 0x100;
    }
    if flags & 0x100 != 0 && !image_root {
        return Err(ParseError::InvalidParam);
    }
    Ok(flags)
}
pub(super) struct PlatformText(Vec<TChar>);
impl PlatformText {
    fn bytes(bytes: &[u8]) -> Result<Self, ParseError> {
        #[cfg(unix)]
        let mut text: Vec<TChar> = bytes.iter().map(|&byte| byte as TChar).collect();
        #[cfg(windows)]
        let mut text = wim_format::platform_text::wtf8_to_utf16(bytes)?;
        if text.contains(&0) {
            return Err(ParseError::InvalidParam);
        }
        text.push(0);
        Ok(Self(text))
    }
    pub(super) fn image_path(bytes: &[u8]) -> Result<Self, ParseError> {
        let mut text = Self::bytes(bytes)?;
        #[cfg(windows)]
        for unit in &mut text.0 {
            if *unit == 47 {
                *unit = 92;
            }
        }
        #[cfg(unix)]
        let _ = &mut text;
        Ok(text)
    }
    fn path(path: &Path) -> Result<Self, ParseError> {
        crate::engine::progress::filename_buffer(Some(path))?
            .map(Self)
            .ok_or(ParseError::InvalidParam)
    }
    pub(super) fn as_ptr(&self) -> *const TChar {
        self.0.as_ptr()
    }
}
pub(super) unsafe fn text(pointer: *const TChar) -> Result<Vec<u8>, ParseError> {
    if pointer.is_null() {
        return Ok(Vec::new());
    }
    #[cfg(unix)]
    {
        // SAFETY: Caller owns readable terminated platform text.
        Ok(unsafe { std::ffi::CStr::from_ptr(pointer) }
            .to_bytes()
            .to_vec())
    }
    #[cfg(windows)]
    {
        let mut length = 0;
        // SAFETY: Caller owns readable terminated wide platform text.
        while unsafe { *pointer.add(length) } != 0 {
            length += 1;
        }
        // SAFETY: Terminated-string scan established readable UTF-16 units.
        wim_format::platform_text::utf16_to_wtf8(unsafe {
            std::slice::from_raw_parts(pointer, length)
        })
    }
}
/// Execute one prepared ADD command, committing a fresh pending metadata owner.
/// # Safety
/// Handle and strings obey the public update command lifetime contract.
pub(crate) unsafe fn execute_add(
    handle: *mut WimHandle,
    image: c_int,
    source: *const TChar,
    target: *const TChar,
    config: *const TChar,
    flags: c_int,
    session: u64,
) -> Result<(), ParseError> {
    // SAFETY: Caller supplies the live handle and terminated command strings.
    let h = unsafe { handle.as_ref() }.ok_or(ParseError::InvalidParam)?;
    crate::engine::path_mutation::select_for_update(h, image)?;
    let target_bytes = unsafe { text(target) }?;
    let units = wim_format::platform_text::wtf8_to_utf16(&target_bytes)?;
    let components: Vec<Vec<u16>> = units
        .split(|&u| u == 47 || u == 92)
        .filter(|p| !p.is_empty())
        .map(<[u16]>::to_vec)
        .collect();
    let mut flags = normalize_flags(&h.header, components.is_empty(), flags)?;
    let source_path = unsafe { crate::engine::handles::path_from_pointer(source) }
        .map_err(|c| ParseError::from_i32(c).unwrap_or(ParseError::InvalidParam))?;
    let default_config = if config.is_null() && flags & 0x1000 != 0 {
        let path = source_path.join("Windows/System32/WimBootCompress.ini");
        let pathname = PlatformText::path(&path)?;
        #[cfg(unix)]
        let exists = {
            let mut stat = std::mem::MaybeUninit::<libc::stat>::uninit();
            // SAFETY: Terminated path and writable stat storage are live.
            (unsafe { libc::stat(pathname.as_ptr(), stat.as_mut_ptr()) }) == 0
        };
        #[cfg(windows)]
        let exists = std::fs::metadata(&path).is_ok();
        if exists {
            flags &= !0x800;
            Some(pathname)
        } else {
            None
        }
    } else {
        None
    };
    let selected_config = default_config.as_ref().map_or(config, |p| p.as_ptr());
    if flags & 0x800 != 0 && !selected_config.is_null() {
        return Err(ParseError::InvalidParam);
    }
    let capture_config = if flags & 0x800 != 0 {
        CaptureConfig::windows_default()
    } else if !selected_config.is_null() {
        // SAFETY: Config string remains live during this command.
        let translated = unsafe { crate::engine::text_file::load_capture_text(selected_config) }
            .map_err(|c| match ParseError::from_i32(c) {
                Some(ParseError::InvalidUtf8String | ParseError::InvalidUtf16String) => {
                    ParseError::InvalidCaptureConfig
                }
                Some(
                    ParseError::Open | ParseError::Stat | ParseError::Nomem | ParseError::Read,
                ) => ParseError::UnableToReadCaptureConfig,
                Some(error) => error,
                None => ParseError::InvalidParam,
            })?;
        {
            #[cfg(unix)]
            let bytes: Vec<_> = translated.into_iter().map(|b| b as u8).collect();
            #[cfg(windows)]
            let bytes = wim_format::platform_text::utf16_to_wtf8(&translated)?;
            CaptureConfig::parse_text(&bytes)?
        }
    } else {
        CaptureConfig::default()
    };
    let index = image as usize - 1;
    let bytes = crate::engine::handles::image_metadata_bytes(h, index)?;
    let mut plan =
        crate::engine::handles::image_capture_plan(&h.images[index])?.unwrap_or(CapturePlan {
            tree: OwnedMetadata::from_metadata(&Metadata::parse(&bytes)?)?,
            bindings: Vec::new(),
            identities: Vec::new(),
        });
    let registration = h.progress.get();
    let target_string = PlatformText::image_path(&target_bytes)?;
    let mut callback = |event: &mut super::ScanEvent<'_>| {
        let current = event.current_path.map(PlatformText::path).transpose()?;
        let symlink = event.symlink_target.map(PlatformText::bytes).transpose()?;
        let mut info = ProgressInfo::zeroed();
        if event.message == 31 {
            info.handle_error = HandleErrorProgress {
                path: current.as_ref().map_or(std::ptr::null(), |p| p.as_ptr()),
                error_code: event.error.ok_or(ParseError::InvalidParam)? as c_int,
                will_ignore: false,
            };
        } else if event.message == 30 {
            info.file_exclusion = FileExclusionProgress {
                path: current.as_ref().map_or(std::ptr::null(), |p| p.as_ptr()),
                will_exclude: false,
            };
        } else {
            info.scan = ScanProgress {
                source,
                current_path: current.as_ref().map_or(std::ptr::null(), |p| p.as_ptr()),
                status: event.status,
                target: symlink
                    .as_ref()
                    .map_or(target_string.as_ptr(), |p| p.as_ptr()),
                directories: event.directories,
                nondirectories: event.nondirectories,
                bytes: event.bytes,
            };
        }
        // SAFETY: Payload strings and registration remain live; no lock is held.
        unsafe { registration.call(event.message, &mut info) }?;
        if event.message == 30 {
            // SAFETY: Event 30 initializes this union member.
            event.exclude = unsafe { info.file_exclusion.will_exclude };
        } else if event.message == 31 {
            // SAFETY: Event 31 initializes this union member.
            event.exclude = unsafe { info.handle_error.will_ignore };
        }
        Ok(())
    };
    let branch = super::scan_source_seeded(
        &source_path,
        components.is_empty(),
        flags,
        &capture_config,
        &plan,
        session,
        &mut callback,
    )?;
    plan.overlay(branch, &components, flags, &mut |path| {
        let path = PlatformText::image_path(path)?;
        let mut info = ProgressInfo::zeroed();
        info.replace = ReplaceProgress {
            path_in_wim: path.as_ptr(),
        };
        // SAFETY: The temporary path is valid through callback completion.
        unsafe { registration.call(23, &mut info) }
    })?;
    if !config.is_null() && flags & 0x1000 != 0 && components.is_empty() {
        // SAFETY: Explicit configuration path is caller-owned terminated text.
        let config_path = unsafe { crate::engine::handles::path_from_pointer(config) }
            .map_err(|c| ParseError::from_i32(c).unwrap_or(ParseError::InvalidParam))?;
        let empty_config = CaptureConfig::default();
        let branch = scan_source(&config_path, false, 0, &empty_config, &mut |_| Ok(()))?;
        let target: Vec<Vec<u16>> = ["Windows", "System32", "WimBootCompress.ini"]
            .into_iter()
            .map(|p| p.encode_utf16().collect())
            .collect();
        plan.overlay(branch, &target, 0, &mut |_| Ok(()))?;
    }
    let output = if plan.tree.nodes.is_empty() {
        bytes.into_owned()
    } else {
        plan.tree.encode()?
    };
    // Stage real inline hashes and retired prior descriptors before releasing
    // the replaced graph's final stream owners.
    let staged_index = crate::engine::lookup::prepare_capture_index(h, index, Some(&plan))?;
    let identity = crate::engine::handles::image_identity(h, &h.images[index]);
    let pending =
        crate::engine::handles::new_pending_metadata(crate::engine::handles::PendingMetadata {
            capture: Some(plan),
            metadata: output,
            hash: [0; 20],
        })?;
    // SAFETY: No callback-bearing shared/exclusive borrow is active at commit.
    let h = unsafe { &mut *handle };
    h.dirty_images
        .try_reserve(1)
        .map_err(|_| ParseError::Nomem)?;
    h.dirty_images
        .try_insert(identity)
        .map_err(|_| ParseError::Nomem)?;
    h.images[index] = crate::engine::HandleImage::Owned(crate::engine::handles::OwnedImage {
        metadata: Vec::new(),
        hash: [0; 20],
        pending: Some(pending),
        descriptor: None,
        identity,
    });
    h.blob_index = staged_index;
    Ok(())
}
