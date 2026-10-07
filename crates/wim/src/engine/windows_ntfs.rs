//! Native NTFS metadata and bounded EFS raw import/export.
use std::{
    ffi::c_void,
    fs::{File, OpenOptions},
    io::{Read, Seek, SeekFrom, Write},
    os::windows::{ffi::OsStrExt, fs::OpenOptionsExt},
    path::Path,
    ptr,
    sync::atomic::{AtomicU64, Ordering},
};
use wim_format::ParseError;
type Handle = *mut c_void;
#[repr(C)]
#[derive(Default)]
struct IoStatus {
    status: usize,
    information: usize,
}
#[link(name = "ntdll")]
unsafe extern "system" {
    fn NtQueryEaFile(
        file: Handle,
        status: *mut IoStatus,
        buffer: *mut c_void,
        length: u32,
        single: u8,
        list: *const c_void,
        list_length: u32,
        index: *const u32,
        restart: u8,
    ) -> i32;
    fn NtSetEaFile(file: Handle, status: *mut IoStatus, buffer: *const c_void, length: u32) -> i32;
}
#[link(name = "kernel32")]
unsafe extern "system" {
    fn DeviceIoControl(
        file: Handle,
        code: u32,
        input: *const c_void,
        input_length: u32,
        output: *mut c_void,
        output_length: u32,
        returned: *mut u32,
        overlapped: *mut c_void,
    ) -> i32;
    fn GetLastError() -> u32;
}
#[link(name = "advapi32")]
unsafe extern "system" {
    fn OpenEncryptedFileRawW(path: *const u16, flags: u32, context: *mut Handle) -> u32;
    fn ReadEncryptedFileRaw(
        callback: unsafe extern "system" fn(*const u8, Handle, u32) -> u32,
        callback_context: Handle,
        context: Handle,
    ) -> u32;
    fn WriteEncryptedFileRaw(
        callback: unsafe extern "system" fn(*mut u8, Handle, *mut u32) -> u32,
        callback_context: Handle,
        context: Handle,
    ) -> u32;
    fn CloseEncryptedFileRaw(context: Handle);
}
struct RawContext(Handle);
impl Drop for RawContext {
    fn drop(&mut self) {
        // SAFETY: This guard owns a successfully opened raw EFS context.
        unsafe { CloseEncryptedFileRaw(self.0) };
    }
}
pub(crate) fn spool() -> Result<File, ParseError> {
    static COUNTER: AtomicU64 = AtomicU64::new(0);
    for _ in 0..128 {
        let path = std::env::temp_dir().join(format!(
            "wim-efs-{}-{}.tmp",
            std::process::id(),
            COUNTER.fetch_add(1, Ordering::Relaxed)
        ));
        match OpenOptions::new()
            .read(true)
            .write(true)
            .create_new(true)
            .custom_flags(0x04000000)
            .open(path)
        {
            Ok(file) => return Ok(file),
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => continue,
            Err(_) => return Err(ParseError::Open),
        }
    }
    Err(ParseError::Open)
}
fn raw_context(path: &Path, flags: u32) -> Result<RawContext, ParseError> {
    let path: Vec<u16> = path.as_os_str().encode_wide().chain(Some(0)).collect();
    let mut context = ptr::null_mut();
    // SAFETY: Terminated pathname, initialized context output and documented flags.
    if unsafe { OpenEncryptedFileRawW(path.as_ptr(), flags, &mut context) } != 0 {
        return Err(ParseError::Open);
    }
    Ok(RawContext(context))
}
unsafe extern "system" fn export_block(data: *const u8, context: Handle, length: u32) -> u32 {
    if length == 0 {
        return 0;
    }
    // SAFETY: EFS provides a valid length-byte block and our synchronous File context.
    let file = unsafe { &mut *context.cast::<File>() };
    let bytes = unsafe { std::slice::from_raw_parts(data, length as usize) };
    if file.write_all(bytes).is_err() {
        1117
    } else {
        0
    }
}
unsafe extern "system" fn import_block(data: *mut u8, context: Handle, length: *mut u32) -> u32 {
    // SAFETY: EFS supplies a valid writable length pointer.
    if unsafe { *length } == 0 {
        return 0;
    }
    // SAFETY: EFS supplies a writable buffer with the capacity in length, and our live File context.
    let file = unsafe { &mut *context.cast::<File>() };
    let bytes = unsafe { std::slice::from_raw_parts_mut(data, *length as usize) };
    match file.read(bytes) {
        Ok(count) => {
            unsafe { *length = count as u32 };
            0
        }
        Err(_) => 1117,
    }
}
pub(crate) fn export_encrypted(path: &Path) -> Result<File, ParseError> {
    let context = raw_context(path, 0)?;
    let mut file = spool()?;
    // SAFETY: Export callback borrows file only for this synchronous EFS operation.
    if unsafe { ReadEncryptedFileRaw(export_block, (&mut file as *mut File).cast(), context.0) }
        != 0
    {
        return Err(ParseError::Read);
    }
    file.seek(SeekFrom::Start(0))
        .map_err(|_| ParseError::Read)?;
    Ok(file)
}
pub(crate) fn import_encrypted(
    path: &Path,
    file: &mut File,
    directory: bool,
) -> Result<(), ParseError> {
    file.seek(SeekFrom::Start(0))
        .map_err(|_| ParseError::Read)?;
    let context = raw_context(path, 1 | 4 | if directory { 2 } else { 0 })?;
    // SAFETY: Import callback borrows the staged raw stream for this synchronous operation.
    if unsafe { WriteEncryptedFileRaw(import_block, (file as *mut File).cast(), context.0) } != 0 {
        return Err(ParseError::Write);
    }
    Ok(())
}
pub(crate) fn capture_tags(file: Handle, object_ids: bool) -> Result<Vec<u8>, ParseError> {
    let mut tagged = Vec::new();
    if object_ids {
        let mut object = [0u8; 64];
        let mut returned = 0;
        // SAFETY: Live file handle and correctly sized FSCTL_GET_OBJECT_ID output.
        if unsafe {
            DeviceIoControl(
                file,
                0x9009c,
                ptr::null(),
                0,
                object.as_mut_ptr().cast(),
                64,
                &mut returned,
                ptr::null_mut(),
            )
        } != 0
        {
            if returned != 64 {
                return Err(ParseError::Stat);
            }
            append_tag(&mut tagged, 1, &object)?;
        } else if !matches!(unsafe { GetLastError() }, 1 | 2 | 50 | 4312) {
            return Err(ParseError::Stat);
        }
    }
    let mut bytes = vec![0u8; 65536];
    let mut status = IoStatus::default();
    // SAFETY: Live handle, initialized bounded EA output and IO status; no optional EA filter.
    let result = unsafe {
        NtQueryEaFile(
            file,
            &mut status,
            bytes.as_mut_ptr().cast(),
            bytes.len() as u32,
            0,
            ptr::null(),
            0,
            ptr::null(),
            1,
        )
    };
    if result == 0 {
        let bytes = bytes
            .get(..status.information)
            .ok_or(ParseError::InvalidXattr)?;
        let packed = crate::engine::ntfs_metadata::pack_eas(bytes)?;
        if !packed.is_empty() {
            append_tag(&mut tagged, 2, &packed)?;
        }
    } else if !matches!(
        result as u32,
        0x80000015 | 0xc0000052 | 0xc000004f | 0xc0000010 | 0xc00000bb
    ) {
        return Err(ParseError::Stat);
    }
    Ok(tagged)
}
fn append_tag(out: &mut Vec<u8>, tag: u32, bytes: &[u8]) -> Result<(), ParseError> {
    out.try_reserve(8 + bytes.len() + 7)
        .map_err(|_| ParseError::Nomem)?;
    out.extend_from_slice(&tag.to_le_bytes());
    out.extend_from_slice(&(bytes.len() as u32).to_le_bytes());
    out.extend_from_slice(bytes);
    out.resize((out.len() + 7) & !7, 0);
    Ok(())
}
pub(crate) fn apply_tags(
    file: Handle,
    object: Option<&[u8]>,
    eas: Option<&[u8]>,
) -> Result<(), ParseError> {
    if let Some(object) = object {
        if !matches!(object.len(), 16 | 64) {
            return Err(ParseError::InvalidMetadataResource);
        }
        let mut returned = 0;
        // SAFETY: Validated object-ID bytes and live metadata-write handle.
        if unsafe {
            DeviceIoControl(
                file,
                0x90098,
                object.as_ptr().cast(),
                object.len() as u32,
                ptr::null_mut(),
                0,
                &mut returned,
                ptr::null_mut(),
            )
        } == 0
        {
            // NTFS object IDs must be unique. Match upstream's collision policy.
            if !matches!(unsafe { GetLastError() }, 52 | 183 | 698) {
                return Err(ParseError::SetAttributes);
            }
        }
    }
    if let Some(eas) = eas {
        let bytes = crate::engine::ntfs_metadata::unpack_eas(eas)?;
        if !bytes.is_empty() {
            let length = u32::try_from(bytes.len()).map_err(|_| ParseError::InvalidXattr)?;
            let mut status = IoStatus::default();
            // SAFETY: Validated native EA list and live EA-write handle.
            if unsafe { NtSetEaFile(file, &mut status, bytes.as_ptr().cast(), length) } < 0 {
                return Err(ParseError::SetXattr);
            }
        }
    }
    Ok(())
}
