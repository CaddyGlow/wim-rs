//! Original capture entry points delegate real commands to transactional update.
use crate::engine::{AddCommand, TChar, UpdateCommand, UpdateCommandData, WimHandle};
use std::ffi::{c_int, c_long};
/// Borrowed original multisource capture record.
#[repr(C)]
#[derive(Clone, Copy)]
pub struct CaptureSource {
    /// Filesystem source path.
    pub fs_source_path: *mut TChar,
    /// Canonicalizable image destination path.
    pub wim_target_path: *mut TChar,
    /// Reserved; must be zero.
    pub reserved: c_long,
}
/// Capture filesystem sources as a new image, rolling back a failed scan/update.
/// # Safety
/// Handle and source array must be live; all nonnull strings are terminated text.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn wimlib_add_image_multisource(
    handle: *mut WimHandle,
    sources: *const CaptureSource,
    count: usize,
    name: *const TChar,
    config: *const TChar,
    flags: c_int,
) -> c_int {
    #[cfg(not(feature = "test-support"))]
    if flags & 0x08000000 != 0 {
        // The generator source is opaque, so reject it before decoding pointers.
        return 24;
    }
    if handle.is_null()
        || (count != 0 && sources.is_null())
        || count > isize::MAX as usize / std::mem::size_of::<CaptureSource>()
    {
        return 24;
    }
    for index in 0..count {
        // SAFETY: Caller guarantees the array spans count initialized sources.
        let source = unsafe { sources.add(index).read() };
        if source.reserved != 0 {
            return 24;
        }
    }
    let result = (|| {
        // SAFETY: The caller guarantees readable platform strings and a live handle.
        let mut decoded = Vec::new();
        decoded
            .try_reserve_exact(count)
            .map_err(|_| wim_format::ParseError::Nomem)?;
        for index in 0..count {
            // SAFETY: Array bounds and string lifetimes are the C caller's contract.
            let source = unsafe { sources.add(index).read() };
            let path = unsafe { crate::engine::handles::path_from_pointer(source.fs_source_path) }
                .map_err(|code| {
                    wim_format::ParseError::from_i32(code)
                        .unwrap_or(wim_format::ParseError::InvalidParam)
                })?;
            let target = unsafe { super::abi::text(source.wim_target_path) }?;
            decoded.push((path, target));
        }
        let name = if name.is_null() {
            None
        } else {
            Some(unsafe { super::abi::text(name) }?)
        };
        let config = if config.is_null() {
            None
        } else {
            Some(
                unsafe { crate::engine::handles::path_from_pointer(config) }.map_err(|code| {
                    wim_format::ParseError::from_i32(code)
                        .unwrap_or(wim_format::ParseError::InvalidParam)
                })?,
            )
        };
        // SAFETY: The C caller owns the handle through all callbacks.
        unsafe { capture_sources(handle, &decoded, name.as_deref(), config.as_deref(), flags) }
            .map(|_| ())
    })();
    result.err().map_or(0, |error| error as c_int)
}

pub(crate) fn capture_image(
    handle: &mut WimHandle,
    source: &std::path::Path,
    name: Option<&std::ffi::OsStr>,
    flags: i32,
) -> Result<u32, wim_format::ParseError> {
    let name = name
        .map(|name| {
            #[cfg(unix)]
            {
                use std::os::unix::ffi::OsStrExt;
                Ok(name.as_bytes().to_vec())
            }
            #[cfg(windows)]
            {
                use std::os::windows::ffi::OsStrExt;
                wim_format::platform_text::utf16_to_wtf8(&name.encode_wide().collect::<Vec<_>>())
            }
            #[cfg(not(any(unix, windows)))]
            {
                Ok(name
                    .to_str()
                    .ok_or(wim_format::ParseError::InvalidUtf8String)?
                    .as_bytes()
                    .to_vec())
            }
        })
        .transpose()?;
    if name.as_ref().is_some_and(|name| name.contains(&0)) {
        return Err(wim_format::ParseError::InvalidParam);
    }
    // SAFETY: The exclusive Rust handle and owned command text remain live.
    unsafe {
        capture_sources(
            handle,
            &[(source.to_owned(), b"/".to_vec())],
            name.as_deref(),
            None,
            flags,
        )
    }
}

unsafe fn capture_sources(
    handle: *mut WimHandle,
    sources: &[(std::path::PathBuf, Vec<u8>)],
    name: Option<&[u8]>,
    config: Option<&std::path::Path>,
    flags: i32,
) -> Result<u32, wim_format::ParseError> {
    use wim_format::ParseError;
    let mut commands = Vec::new();
    let mut source_text = Vec::new();
    let mut target_text = Vec::new();
    commands
        .try_reserve_exact(sources.len())
        .map_err(|_| ParseError::Nomem)?;
    source_text
        .try_reserve_exact(sources.len())
        .map_err(|_| ParseError::Nomem)?;
    target_text
        .try_reserve_exact(sources.len())
        .map_err(|_| ParseError::Nomem)?;
    let config_text = crate::engine::progress::filename_buffer(config)?;
    for (source, target) in sources {
        source_text.push(
            crate::engine::progress::filename_buffer(Some(source))?
                .ok_or(ParseError::InvalidParam)?,
        );
        target_text.push(super::abi::PlatformText::image_path(target)?);
    }
    for source in &source_text {
        if source[..source.len() - 1].contains(&0) {
            return Err(ParseError::InvalidParam);
        }
    }
    for (source, target) in source_text.iter_mut().zip(&target_text) {
        commands.push(UpdateCommand {
            op: 0,
            data: UpdateCommandData {
                add: AddCommand {
                    fs_source_path: source.as_mut_ptr(),
                    wim_target_path: target.as_ptr().cast_mut(),
                    config_file: config_text
                        .as_ref()
                        .map_or(std::ptr::null_mut(), |text| text.as_ptr().cast_mut()),
                    add_flags: flags & !8,
                },
            },
        });
    }
    // SAFETY: Fresh exclusive borrow before any capture callback begins.
    let image = crate::engine::image_mutation::add_empty(unsafe { &mut *handle }, name)?;
    // SAFETY: Owned command strings and exclusive handle remain live through callbacks.
    let result = unsafe {
        crate::engine::update::update_commands(
            handle,
            image as i32,
            commands.as_ptr(),
            commands.len(),
            0,
        )
    }
    .and_then(|()| {
        // SAFETY: The transactional command has completed every callback.
        let handle = unsafe { &mut *handle };
        if flags & 0x1000 != 0 {
            handle
                .xml
                .set_property_bytes(image as i32, b"WIMBOOT", Some(b"1"))?;
        }
        if flags & 8 != 0 {
            handle.header.boot_index = image;
        }
        Ok(image)
    });
    if result.is_err() {
        let _ = crate::engine::image_mutation::delete_images(unsafe { &mut *handle }, image as i32);
    }
    result
}
/// Capture one filesystem directory as a new image with deferred file contents.
/// # Safety
/// Handle and all nonnull strings must satisfy the capture API lifetime contract.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn wimlib_add_image(
    handle: *mut WimHandle,
    source: *const TChar,
    name: *const TChar,
    config: *const TChar,
    flags: c_int,
) -> c_int {
    #[cfg(feature = "test-support")]
    if flags & 0x08000000 != 0 {
        // SAFETY: Optional source is deliberately opaque, matching original generator.
        return unsafe {
            crate::engine::test_support::generate::add_image(handle, source, name, config, flags)
        };
    }
    let target = [47 as TChar, 0];
    let source = CaptureSource {
        fs_source_path: source.cast_mut(),
        wim_target_path: target.as_ptr().cast_mut(),
        reserved: 0,
    };
    // SAFETY: Temporary source/target records remain valid for the delegated call.
    unsafe { wimlib_add_image_multisource(handle, &source, 1, name, config, flags) }
}
/// Add one filesystem branch to an existing image through a real ADD command.
/// # Safety
/// Handle and source/target strings must remain live through update callbacks.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn wimlib_add_tree(
    handle: *mut WimHandle,
    image: c_int,
    source: *const TChar,
    target: *const TChar,
    flags: c_int,
) -> c_int {
    let command = UpdateCommand {
        op: 0,
        data: UpdateCommandData {
            add: AddCommand {
                fs_source_path: source.cast_mut(),
                wim_target_path: target.cast_mut(),
                config_file: std::ptr::null_mut(),
                add_flags: flags,
            },
        },
    };
    // SAFETY: Command storage and caller strings remain valid for update.
    unsafe {
        crate::engine::update::update_commands(handle, image, &command, 1, 0)
            .err()
            .map_or(0, |error| error as c_int)
    }
}
