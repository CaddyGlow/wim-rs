//! Borrowed command ABI and transactional image update orchestration.

use crate::engine::collections::FallibleSet as _;
use crate::engine::{HandleImage, ProgressInfo, TChar, WimHandle};
use std::ffi::c_int;
use wim_format::ParseError;
/// Filesystem capture command.
#[repr(C)]
#[derive(Clone, Copy)]
pub struct AddCommand {
    /// Filesystem source.
    pub fs_source_path: *mut TChar,
    /// Image target.
    pub wim_target_path: *mut TChar,
    /// Optional capture configuration.
    pub config_file: *mut TChar,
    /// Capture flags.
    pub add_flags: c_int,
}
/// Image path deletion command.
#[repr(C)]
#[derive(Clone, Copy)]
pub struct DeleteCommand {
    /// Image path.
    pub wim_path: *mut TChar,
    /// Deletion flags.
    pub delete_flags: c_int,
}
/// Image path rename command.
#[repr(C)]
#[derive(Clone, Copy)]
pub struct RenameCommand {
    /// Source image path.
    pub wim_source_path: *mut TChar,
    /// Target image path.
    pub wim_target_path: *mut TChar,
    /// Reserved zero flags.
    pub rename_flags: c_int,
}
/// Original operation-specific command union.
#[repr(C)]
#[derive(Clone, Copy)]
pub union UpdateCommandData {
    /// Operation zero.
    pub add: AddCommand,
    /// Operation one.
    pub delete: DeleteCommand,
    /// Operation two.
    pub rename: RenameCommand,
}
/// Original tagged update command.
#[repr(C)]
#[derive(Clone, Copy)]
pub struct UpdateCommand {
    /// Operation zero, one or two.
    pub op: c_int,
    /// Payload selected by operation.
    pub data: UpdateCommandData,
}
/// Borrowed update command progress payload.
#[repr(C)]
#[derive(Clone, Copy)]
pub struct UpdateProgress {
    /// Canonicalized command, valid only during this callback.
    pub command: *const UpdateCommand,
    /// Number already completed.
    pub completed_commands: usize,
    /// Total commands.
    pub total_commands: usize,
}
unsafe fn canonical(pointer: *const TChar) -> Result<Vec<TChar>, ParseError> {
    let mut length = 0usize;
    if !pointer.is_null() {
        // SAFETY: Caller supplies readable terminated platform text.
        while unsafe { *pointer.add(length) } != 0 {
            length = length.checked_add(1).ok_or(ParseError::Nomem)?;
        }
    }
    let capacity = length.checked_add(2).ok_or(ParseError::Nomem)?;
    let mut output = Vec::new();
    output
        .try_reserve_exact(capacity)
        .map_err(|_| ParseError::Nomem)?;
    #[cfg(windows)]
    const SEPARATOR: TChar = 92;
    #[cfg(not(windows))]
    const SEPARATOR: TChar = 47;
    output.push(SEPARATOR);
    let mut separator = false;
    for index in 0..length {
        // SAFETY: The preceding terminated-text scan established this range.
        let unit = unsafe { *pointer.add(index) };
        if unit == 47 as TChar || unit == 92 as TChar {
            separator = output.len() > 1;
        } else {
            if separator {
                output.push(SEPARATOR);
                separator = false;
            }
            output.push(unit);
        }
    }
    output.push(0);
    Ok(output)
}

struct Prepared {
    command: UpdateCommand,
    _source: Vec<TChar>,
    _target: Vec<TChar>,
}
unsafe fn prepare(
    command: UpdateCommand,
    _header: &wim_format::Header,
) -> Result<Prepared, ParseError> {
    // SAFETY: The operation determines the initialized caller union member.
    unsafe {
        if command.op == 0 {
            #[cfg(not(any(unix, windows)))]
            return Err(ParseError::Unsupported);
            #[cfg(any(unix, windows))]
            {
                let add = command.data.add;
                let mut target = canonical(add.wim_target_path)?;
                let flags = crate::engine::capture::normalize_flags(
                    _header,
                    target.len() == 2,
                    add.add_flags,
                )?;
                return Ok(Prepared {
                    command: UpdateCommand {
                        op: 0,
                        data: UpdateCommandData {
                            add: AddCommand {
                                fs_source_path: add.fs_source_path,
                                wim_target_path: target.as_mut_ptr(),
                                config_file: add.config_file,
                                add_flags: flags,
                            },
                        },
                    },
                    _source: Vec::new(),
                    _target: target,
                });
            }
        }
        let (mut source, mut target, flags) = match command.op {
            1 => (
                canonical(command.data.delete.wim_path)?,
                Vec::new(),
                command.data.delete.delete_flags,
            ),
            2 => (
                canonical(command.data.rename.wim_source_path)?,
                canonical(command.data.rename.wim_target_path)?,
                // Original copy_update_commands leaves this calloc-zeroed field
                // uncopied, including the progress-visible canonical command.
                0,
            ),
            _ => return Err(ParseError::InvalidParam),
        };
        if (command.op == 1 && flags & !3 != 0) || (command.op == 2 && flags != 0) {
            return Err(ParseError::InvalidParam);
        }
        let data = if command.op == 1 {
            UpdateCommandData {
                delete: DeleteCommand {
                    wim_path: source.as_mut_ptr(),
                    delete_flags: flags,
                },
            }
        } else {
            UpdateCommandData {
                rename: RenameCommand {
                    wim_source_path: source.as_mut_ptr(),
                    wim_target_path: target.as_mut_ptr(),
                    rename_flags: flags,
                },
            }
        };
        Ok(Prepared {
            command: UpdateCommand {
                op: command.op,
                data,
            },
            _source: source,
            _target: target,
        })
    }
}
struct Checkpoint {
    blob_index: crate::engine::blob_index::BlobIndex,
    image: HandleImage,
    dirty: hashbrown::HashSet<(u64, u32)>,
    references: Vec<u32>,
    owned: Vec<([u8; 20], u32)>,
}
impl Checkpoint {
    fn capture(handle: &WimHandle, index: usize) -> Result<Self, ParseError> {
        let image = match &handle.images[index] {
            HandleImage::Owned(image) => {
                let mut metadata = Vec::new();
                metadata
                    .try_reserve_exact(image.metadata.len())
                    .map_err(|_| ParseError::Nomem)?;
                metadata.extend_from_slice(&image.metadata);
                HandleImage::Owned(crate::engine::OwnedImage {
                    metadata,
                    hash: image.hash,
                    pending: image.pending.clone(),
                    descriptor: image.descriptor.clone(),
                    identity: image.identity,
                })
            }
            other => other.clone(),
        };
        let mut dirty = hashbrown::HashSet::new();
        dirty
            .try_reserve(handle.dirty_images.len())
            .map_err(|_| ParseError::Nomem)?;
        dirty.extend(handle.dirty_images.iter().copied());
        let mut references = Vec::new();
        if let Some(lookup) = &handle.lookup {
            references
                .try_reserve_exact(lookup.blobs.len())
                .map_err(|_| ParseError::Nomem)?;
            references.extend(lookup.blobs.iter().map(|b| b.reference_count));
        }
        let mut owned = Vec::new();
        owned
            .try_reserve_exact(handle.owned_blobs.len())
            .map_err(|_| ParseError::Nomem)?;
        owned.extend(
            handle
                .owned_blobs
                .iter()
                .map(|(h, b)| (*h, b.descriptor.blob.reference_count)),
        );
        Ok(Self {
            blob_index: handle.blob_index.try_clone()?,
            image,
            dirty,
            references,
            owned,
        })
    }
    fn restore(self, handle: &mut WimHandle, index: usize) {
        handle.blob_index = self.blob_index;
        handle.images[index] = self.image;
        handle.dirty_images = self.dirty;
        if let Some(lookup) = &mut handle.lookup {
            for (blob, count) in lookup.blobs.iter_mut().zip(self.references) {
                blob.reference_count = count;
            }
        }
        for (hash, count) in self.owned {
            if let Some(blob) = handle.owned_blobs.get_mut(&hash) {
                blob.descriptor.blob.reference_count = count;
            }
        }
    }
}
unsafe fn event(
    handle: *mut WimHandle,
    command: &UpdateCommand,
    completed: usize,
    total: usize,
    message: c_int,
) -> Result<(), ParseError> {
    let mut info = ProgressInfo::zeroed();
    info.update = UpdateProgress {
        command,
        completed_commands: completed,
        total_commands: total,
    };
    // SAFETY: Live shared handle and callback-only borrowed command/payload.
    unsafe { (&*handle).progress.get().call(message, &mut info) }
}
/// Execute image updates transactionally, retaining captured payloads for deferred reads.
/// # Safety
/// Handle and command array must be live and exclusively used; command strings
/// must be readable terminated text. Callbacks may replace progress registration.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn wimlib_update_image(
    handle: *mut WimHandle,
    image: c_int,
    commands: *const UpdateCommand,
    count: usize,
    flags: c_int,
) -> c_int {
    // SAFETY: The C caller guarantees command storage and its string lifetimes.
    unsafe { update_commands(handle, image, commands, count, flags) }
        .err()
        .map_or(0, |error| error as c_int)
}

/// Execute validated command storage without crossing the C adapter.
/// # Safety
/// Handle and command strings must remain live through callback completion.
pub(crate) unsafe fn update_commands(
    handle: *mut WimHandle,
    image: c_int,
    commands: *const UpdateCommand,
    count: usize,
    flags: c_int,
) -> Result<(), ParseError> {
    (|| {
        if flags & !1 != 0 {
            return Err(ParseError::InvalidParam);
        }
        // SAFETY: Caller supplies a live handle.
        let h = unsafe { handle.as_ref() }.ok_or(ParseError::InvalidParam)?;
        crate::engine::path_mutation::select_for_update(h, image)?;
        if count > isize::MAX as usize / std::mem::size_of::<UpdateCommand>()
            || (count != 0 && commands.is_null())
        {
            return Err(ParseError::InvalidParam);
        }
        let index = image as usize - 1;
        let mut prepared = Vec::new();
        prepared
            .try_reserve_exact(count)
            .map_err(|_| ParseError::Nomem)?;
        for i in 0..count {
            // SAFETY: Caller array spans count initialized command records.
            prepared.push(unsafe { prepare(commands.add(i).read(), &h.header) }?);
        }
        let checkpoint = Checkpoint::capture(h, index)?;
        // Reserve the eventual dirty marker before any command changes state.
        // SAFETY: The preceding shared borrow ends before this exclusive access.
        unsafe { &mut *handle }
            .dirty_images
            .try_reserve(1)
            .map_err(|_| ParseError::Nomem)?;
        let execution = (|| {
            #[cfg(any(unix, windows))]
            let capture_session = crate::engine::handles::new_identity();
            for (i, prepared) in prepared.iter().enumerate() {
                let command = &prepared.command;
                if flags & 1 != 0 {
                    // SAFETY: Command and borrowed registration remain live.
                    unsafe { event(handle, command, i, count, 21) }?;
                }
                // SAFETY: No whole-handle borrow spans callbacks; operation selects the union.
                unsafe {
                    match command.op {
                        0 => {
                            #[cfg(not(any(unix, windows)))]
                            {
                                Err(ParseError::Unsupported)
                            }
                            #[cfg(any(unix, windows))]
                            {
                                crate::engine::capture::execute_add(
                                    handle,
                                    image,
                                    command.data.add.fs_source_path,
                                    command.data.add.wim_target_path,
                                    command.data.add.config_file,
                                    command.data.add.add_flags,
                                    capture_session,
                                )
                            }
                        }
                        1 => crate::engine::path_mutation::delete_path(
                            handle,
                            image,
                            command.data.delete.wim_path,
                            command.data.delete.delete_flags,
                        ),
                        _ => crate::engine::path_mutation::rename_path(
                            handle,
                            image,
                            command.data.rename.wim_source_path,
                            command.data.rename.wim_target_path,
                        ),
                    }
                }?;
                if flags & 1 != 0 {
                    // SAFETY: As above; registration is reread after each command.
                    unsafe { event(handle, command, i + 1, count, 22) }?;
                }
            }
            Ok(())
        })();
        if execution.is_err() {
            // SAFETY: Fresh exclusive borrow after callback completion.
            checkpoint.restore(unsafe { &mut *handle }, index);
        } else {
            // SAFETY: Fresh exclusive borrow after callback completion.
            let h = unsafe { &mut *handle };
            h.dirty_images
                .try_insert(crate::engine::handles::image_identity(h, &h.images[index]))
                .map_err(|_| ParseError::Nomem)?;
            if prepared.iter().any(|prepared| {
                // SAFETY: Operation zero selects the initialized ADD payload.
                prepared.command.op == 0
                    && unsafe { prepared.command.data.add.add_flags } & 0x100 != 0
            }) {
                h.header.flags |= 0x80;
            }
        }
        execution
    })()
}

/// Add a filesystem branch using owned platform text and transactional updates.
pub(crate) fn add_file(
    handle: &mut WimHandle,
    image: i32,
    source: &std::path::Path,
    target: &std::ffi::OsStr,
    flags: i32,
) -> Result<(), ParseError> {
    let mut source =
        crate::engine::progress::filename_buffer(Some(source))?.ok_or(ParseError::InvalidParam)?;
    let mut target = crate::engine::progress::filename_buffer(Some(std::path::Path::new(target)))?
        .ok_or(ParseError::InvalidParam)?;
    if source[..source.len() - 1].contains(&0) || target[..target.len() - 1].contains(&0) {
        return Err(ParseError::InvalidParam);
    }
    let command = UpdateCommand {
        op: 0,
        data: UpdateCommandData {
            add: AddCommand {
                fs_source_path: source.as_mut_ptr(),
                wim_target_path: target.as_mut_ptr(),
                config_file: std::ptr::null_mut(),
                add_flags: flags,
            },
        },
    };
    // SAFETY: Exclusive handle and owned terminated strings outlive every callback.
    unsafe { update_commands(handle, image, &command, 1, 0) }
}
