// SPDX-License-Identifier: LGPL-2.1-or-later
//! Native filesystem extraction from current image metadata and resource ownership.
use crate::engine::{
    TChar,
    handles::{WimHandle, image_metadata_bytes, path_from_pointer},
};
use std::ffi::c_int;
use std::path::Path;
use wim_format::{ParseError, metadata::Metadata};

#[cfg(any(unix, windows))]
mod blob;
mod paths;
#[cfg(any(target_os = "linux", windows))]
mod pipe;
#[cfg(any(target_os = "linux", windows))]
pub use pipe::*;
#[cfg(unix)]
pub(crate) mod unix;
#[cfg(windows)]
mod windows;
pub(crate) use paths::extract_paths;
pub use paths::{wimlib_extract_pathlist, wimlib_extract_paths};

pub(crate) const PUBLIC_FLAGS: u32 = 0x0f7c_ffe3;

fn checked_flags(handle: &WimHandle, flags: u32, image_mode: bool) -> Result<u32, ParseError> {
    if flags & !PUBLIC_FLAGS != 0
        || flags & (0x40 | 0x80) == (0x40 | 0x80)
        || flags & (0x100 | 0x200) == (0x100 | 0x200)
    {
        return Err(ParseError::InvalidParam);
    }
    if flags & (1 | 0x0040_0000 | 0x0f00_0000) != 0 {
        return Err(ParseError::Unsupported);
    }
    Ok(
        if image_mode && flags & 0x300 == 0 && handle.header.flags & 0x80 != 0 {
            flags | 0x100
        } else {
            flags
        },
    )
}

fn extract_one(
    handle: &WimHandle,
    image: i32,
    target: &Path,
    flags: u32,
) -> Result<(), ParseError> {
    let flags = checked_flags(handle, flags, true)?;
    if image <= 0 || image as u32 > handle.header.image_count {
        return Err(ParseError::InvalidImage);
    }
    // Resolve captured streams before any extraction output or callbacks.
    let selected_bytes = image_metadata_bytes(handle, image as usize - 1)?;
    Metadata::parse(&selected_bytes)?;
    crate::engine::capture::checksum_pending(handle)?;
    let bytes = image_metadata_bytes(handle, image as usize - 1)?;
    let metadata = Metadata::parse(&bytes)?;
    if metadata.nodes.is_empty() {
        #[cfg(target_os = "linux")]
        // SAFETY: Original root lookup reports ENOENT for pending rootless images.
        unsafe {
            *libc::__errno_location() = libc::ENOENT;
        }
        return Err(ParseError::PathDoesNotExist);
    }
    #[cfg(unix)]
    return unix::extract(handle, image, target, flags, &metadata, None);
    #[cfg(windows)]
    return windows::extract(handle, image, target, flags, &metadata, None);
    #[cfg(not(any(unix, windows)))]
    {
        let _ = (target, flags, metadata);
        Err(ParseError::Unsupported)
    }
}

/// Extract an image into a filesystem directory, or all images into named subdirectories.
///
/// # Safety
/// The handle must be live and the target a readable, terminated platform string.
/// Callback storage must remain valid throughout extraction.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn wimlib_extract_image(
    handle: *mut WimHandle,
    image: c_int,
    target: *const TChar,
    flags: c_int,
) -> c_int {
    let result = (|| {
        let handle = unsafe { handle.as_ref() }.ok_or(ParseError::InvalidParam)?;
        let target = unsafe { path_from_pointer(target) }
            .map_err(|code| ParseError::from_i32(code).unwrap_or(ParseError::InvalidParam))?;
        extract_image(handle, image, &target, flags as u32)
    })();
    result.err().map_or(0, |error| error as c_int)
}

/// Extract an image through the Rust engine, retaining C-compatible progress behavior.
pub(crate) fn extract_image(
    handle: &WimHandle,
    image: i32,
    target: &Path,
    flags: u32,
) -> Result<(), ParseError> {
    (|| {
        if flags & !PUBLIC_FLAGS != 0 || flags & (0x0020_0000 | 0x400 | 0x0004_0000) != 0 {
            return Err(ParseError::InvalidParam);
        }
        if target.as_os_str().is_empty() {
            return Err(ParseError::InvalidParam);
        }
        if image == -1 {
            if flags & 1 != 0 {
                return Err(ParseError::InvalidParam);
            }
            if let Err(error) = std::fs::create_dir(target)
                && error.kind() != std::io::ErrorKind::AlreadyExists
            {
                return Err(ParseError::Mkdir);
            }
            for index in 1..=handle.header.image_count {
                #[cfg(unix)]
                let name = handle.xml.name_bytes(index as i32).unwrap_or_default();
                #[cfg(unix)]
                let name = {
                    use std::os::unix::ffi::OsStrExt;
                    if !name.is_empty()
                        && name.len() <= 128
                        && !name.contains(&b'/')
                        && name != b"."
                        && name != b".."
                    {
                        std::ffi::OsStr::from_bytes(name).to_owned()
                    } else {
                        index.to_string().into()
                    }
                };
                #[cfg(windows)]
                let name = {
                    use std::os::windows::ffi::OsStringExt;
                    let units = wim_format::platform_text::wtf8_to_utf16z(
                        handle.xml.name_bytes(index as i32).unwrap_or_default(),
                    )?;
                    let units = &units[..units.len() - 1];
                    if !units.is_empty()
                        && units.len() <= 128
                        && units != [46]
                        && units != [46, 46]
                        && !units
                            .iter()
                            .any(|&u| matches!(u, 60 | 62 | 58 | 34 | 47 | 92 | 124 | 63 | 42))
                    {
                        std::ffi::OsString::from_wide(units)
                    } else {
                        index.to_string().into()
                    }
                };
                #[cfg(not(any(unix, windows)))]
                let name = index.to_string();
                extract_one(handle, index as i32, &target.join(name), flags)?;
            }
            Ok(())
        } else {
            extract_one(handle, image, target, flags)
        }
    })()
}
