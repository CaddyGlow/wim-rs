//! Windows handle-based capture of ordinary files and directories.
//! Self-relative security descriptors use the original query/retry policy.
//! EFS raw ciphertext, object IDs and binary extended attributes are preserved.
//! Ordinary named data streams are retained on files and directories.
use super::common::{CaptureConfig, ScanCallback, ScanEvent};
use super::{CaptureBinding, CaptureIdentity, CapturePlan, CapturedSource, CapturedStream};
use sha1::{Digest, Sha1};
use std::{
    collections::{HashMap, HashSet},
    ffi::c_void,
    os::windows::ffi::{OsStrExt, OsStringExt},
    path::{Path, PathBuf},
    sync::Arc,
};
use wim_format::{
    ParseError,
    metadata_write::{OwnedDentry, OwnedStream},
    platform_text::utf16_to_wtf8,
};

type Handle = *mut c_void;
#[repr(C)]
#[derive(Clone, Copy, Default)]
struct FileTime {
    low: u32,
    high: u32,
}
impl FileTime {
    fn value(self) -> u64 {
        u64::from(self.low) | (u64::from(self.high) << 32)
    }
}
#[repr(C)]
#[derive(Default)]
struct FileInformation {
    attributes: u32,
    creation: FileTime,
    access: FileTime,
    write: FileTime,
    volume: u32,
    size_high: u32,
    size_low: u32,
    links: u32,
    index_high: u32,
    index_low: u32,
}
#[repr(C)]
#[derive(Default)]
struct IoStatus {
    status: usize,
    information: usize,
}
#[link(name = "kernel32")]
unsafe extern "system" {
    fn CreateFileW(
        path: *const u16,
        access: u32,
        share: u32,
        security: *const c_void,
        creation: u32,
        flags: u32,
        template: Handle,
    ) -> Handle;
    fn GetFileInformationByHandle(handle: Handle, information: *mut FileInformation) -> i32;
    fn GetVolumeInformationByHandleW(
        handle: Handle,
        name: *mut u16,
        name_length: u32,
        serial: *mut u32,
        maximum_component: *mut u32,
        flags: *mut u32,
        filesystem: *mut u16,
        filesystem_length: u32,
    ) -> i32;
    fn CloseHandle(handle: Handle) -> i32;
    fn DeviceIoControl(
        handle: Handle,
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
#[repr(C)]
struct UnicodeString {
    length: u16,
    maximum: u16,
    buffer: *mut u16,
}
#[repr(C)]
struct ObjectAttributes {
    length: u32,
    root: Handle,
    name: *mut UnicodeString,
    attributes: u32,
    security: *mut c_void,
    quality: *mut c_void,
}
#[link(name = "ntdll")]
unsafe extern "system" {
    fn NtOpenFile(
        output: *mut Handle,
        access: u32,
        attributes: *mut ObjectAttributes,
        status: *mut IoStatus,
        share: u32,
        options: u32,
    ) -> i32;
    fn NtQuerySecurityObject(
        handle: Handle,
        information: u32,
        descriptor: *mut c_void,
        length: u32,
        needed: *mut u32,
    ) -> i32;
    fn NtQueryInformationFile(
        handle: Handle,
        status: *mut IoStatus,
        output: *mut c_void,
        length: u32,
        class: u32,
    ) -> i32;
}
struct FileHandle(Handle);
impl Drop for FileHandle {
    fn drop(&mut self) {
        // SAFETY: This object exclusively owns a valid open handle.
        unsafe {
            CloseHandle(self.0);
        }
    }
}
fn wide(path: &Path) -> Result<Vec<u16>, ParseError> {
    let absolute = std::path::absolute(path).map_err(|_| ParseError::Open)?;
    let mut units: Vec<_> = absolute.as_os_str().encode_wide().collect();
    if units.contains(&0) {
        return Err(ParseError::InvalidParam);
    }
    // Extended-length paths retain every UTF-16 unit, including unpaired surrogates.
    if !units.starts_with(&[92, 92, 63, 92]) {
        let mut prefix = if units.starts_with(&[92, 92]) {
            vec![92, 92, 63, 92, 85, 78, 67, 92]
        } else {
            vec![92, 92, 63, 92]
        };
        prefix.extend_from_slice(if units.starts_with(&[92, 92]) {
            &units[2..]
        } else {
            &units
        });
        units = prefix;
    }
    units.push(0);
    Ok(units)
}
fn open(path: &[u16]) -> Result<FileHandle, ParseError> {
    let mut access = 0x80 | 8 | 0x20000 | 0x01000000;
    loop {
        // SAFETY: Terminated path is live; sharing and backup/reparse semantics
        // match the source scanner. Retry removes only denied security rights.
        let handle = unsafe {
            CreateFileW(
                path.as_ptr(),
                access,
                7,
                std::ptr::null(),
                3,
                0x02000000 | 0x00200000,
                std::ptr::null_mut(),
            )
        };
        if handle as isize != -1 {
            return Ok(FileHandle(handle));
        }
        // SAFETY: Read the immediately preceding failed open's error.
        if matches!(unsafe { GetLastError() }, 5 | 1314) {
            if access & 0x01000000 != 0 {
                access &= !0x01000000;
                continue;
            }
            if access & 0x20000 != 0 {
                access &= !0x20000;
                continue;
            }
        }
        return Err(ParseError::Open);
    }
}
fn security_descriptor(
    handle: &FileHandle,
    flags: i32,
    volume: u32,
) -> Result<Option<Vec<u8>>, ParseError> {
    if flags & 0x20 != 0 || volume & 8 == 0 {
        return Ok(None);
    }
    let mut information = 1 | 2 | 4 | 8 | 0x10 | 0x10000;
    let mut stack = [0u64; 512];
    let mut expanded: Vec<u64> = Vec::new();
    loop {
        let (pointer, length) = if expanded.is_empty() {
            (stack.as_mut_ptr(), 4096)
        } else {
            (expanded.as_mut_ptr(), expanded.len() * 8)
        };
        let mut needed = 0;
        // SAFETY: Handle and aligned initialized descriptor buffer remain live
        // for this synchronous kernel query; needed is a writable ULONG.
        let status = unsafe {
            NtQuerySecurityObject(
                handle.0,
                information,
                pointer.cast(),
                u32::try_from(length).map_err(|_| ParseError::Stat)?,
                &mut needed,
            )
        };
        if status >= 0 {
            if needed == 0 {
                return Ok(None);
            }
            let needed = needed as usize;
            if needed > length {
                return Err(ParseError::Stat);
            }
            let mut descriptor = Vec::new();
            descriptor
                .try_reserve_exact(needed)
                .map_err(|_| ParseError::Stat)?;
            // SAFETY: The successful query initialized needed bytes within the buffer.
            descriptor.extend_from_slice(unsafe {
                std::slice::from_raw_parts(pointer.cast::<u8>(), needed)
            });
            return Ok(Some(descriptor));
        }
        match status as u32 {
            0xc0000023 if expanded.is_empty() => {
                let units = (needed as usize).div_ceil(8);
                if units == 0 {
                    return Err(ParseError::Stat);
                }
                expanded
                    .try_reserve_exact(units)
                    .map_err(|_| ParseError::Stat)?;
                expanded.resize(units, 0);
            }
            0xc0000022 | 0xc0000061 if flags & 0x40 == 0 => {
                if information & 8 == 0 {
                    return Ok(None);
                }
                information &= !(8 | 0x10 | 0x10000);
            }
            _ => return Err(ParseError::Stat),
        }
    }
}
fn copy_security(bytes: &[u8]) -> Result<Vec<u8>, ParseError> {
    let mut copy = Vec::new();
    copy.try_reserve_exact(bytes.len())
        .map_err(|_| ParseError::Nomem)?;
    copy.extend_from_slice(bytes);
    Ok(copy)
}
fn retain_security(
    tree: &mut wim_format::metadata_write::OwnedMetadata,
    descriptor: Option<Vec<u8>>,
) -> Result<u32, ParseError> {
    let Some(descriptor) = descriptor else {
        return Ok(u32::MAX);
    };
    if let Some(index) = tree
        .security_descriptors
        .iter()
        .position(|sd| *sd == descriptor)
    {
        return u32::try_from(index).map_err(|_| ParseError::Stat);
    }
    let index = u32::try_from(tree.security_descriptors.len()).map_err(|_| ParseError::Stat)?;
    tree.security_descriptors
        .try_reserve(1)
        .map_err(|_| ParseError::Stat)?;
    tree.security_descriptors.push(descriptor);
    Ok(index)
}
fn information(handle: &FileHandle) -> Result<FileInformation, ParseError> {
    let mut info = FileInformation::default();
    // SAFETY: Open handle and writable exact-layout output remain live.
    if unsafe { GetFileInformationByHandle(handle.0, &mut info) } == 0 {
        return Err(ParseError::Stat);
    }
    Ok(info)
}
fn check_directory_case_policy(handle: &FileHandle) -> Result<(), ParseError> {
    let mut flags = 0u32;
    let mut status = IoStatus::default();
    // SAFETY: FileCaseSensitiveInformation writes one DWORD through the live
    // no-follow handle opened with FILE_READ_ATTRIBUTES.
    let result = unsafe {
        NtQueryInformationFile(
            handle.0,
            &mut status,
            std::ptr::from_mut(&mut flags).cast(),
            4,
            71,
        )
    };
    if result < 0 {
        return match result as u32 {
            // Older Windows/filesystems do not implement this directory flag.
            0xc0000002 | 0xc0000003 | 0xc000000d | 0xc00000bb => Ok(()),
            _ => Err(ParseError::Stat),
        };
    }
    if status.information != 4 {
        return Err(ParseError::Stat);
    }
    if flags != 0 {
        // This engine cannot restore per-directory case sensitivity. Fail
        // rather than silently changing name lookup or merging case aliases.
        return Err(ParseError::Unsupported);
    }
    Ok(())
}
fn volume_flags(handle: &FileHandle) -> Result<u32, ParseError> {
    let mut flags = 0;
    // SAFETY: Optional output buffers are NULL; flags is live writable DWORD storage.
    if unsafe {
        GetVolumeInformationByHandleW(
            handle.0,
            std::ptr::null_mut(),
            0,
            std::ptr::null_mut(),
            std::ptr::null_mut(),
            &mut flags,
            std::ptr::null_mut(),
            0,
        )
    } == 0
    {
        return Err(ParseError::Stat);
    }
    Ok(flags)
}
fn short_name(handle: &FileHandle) -> Result<Vec<u8>, ParseError> {
    // FILE_NAME_INFORMATION contains a byte length followed by UTF-16 units.
    // Match the original's aligned 128-byte buffer and class 21 query; unlike
    // WIN32_FIND_DATA this also returns the alternate name of an 8.3 filename.
    let mut buffer = [0u64; 16];
    let mut status = IoStatus::default();
    // SAFETY: The live file handle, aligned writable buffer and status record
    // remain valid throughout the synchronous query.
    let result = unsafe {
        NtQueryInformationFile(handle.0, &mut status, buffer.as_mut_ptr().cast(), 128, 21)
    };
    if result < 0 {
        return if result as u32 == 0xc0000017 {
            Err(ParseError::Nomem)
        } else {
            Ok(Vec::new())
        };
    }
    // SAFETY: The initialized buffer has exactly 128 accessible bytes.
    let bytes = unsafe { std::slice::from_raw_parts(buffer.as_ptr().cast::<u8>(), 128) };
    let length = u32::from_le_bytes(bytes[..4].try_into().expect("four-byte length")) as usize;
    if length > 124 || !length.is_multiple_of(2) {
        return Err(ParseError::Stat);
    }
    let mut name = Vec::new();
    name.try_reserve_exact(length)
        .map_err(|_| ParseError::Nomem)?;
    name.extend_from_slice(&bytes[4..4 + length]);
    Ok(name)
}
fn data_streams(
    handle: &FileHandle,
    attributes: u32,
    volume: u32,
) -> Result<Vec<super::ntfs_streams::DataStream>, ParseError> {
    if attributes & 0x4000 != 0 || volume & 0x40000 == 0 {
        return Ok(Vec::new());
    }
    // Query the existing no-follow backup handle. A path-based enumeration
    // would follow links and can reject protected files despite backup privileges.
    let mut buffer = vec![0u64; 512];
    loop {
        let mut status = IoStatus::default();
        let length = buffer.len() * 8;
        // SAFETY: Live handle, aligned initialized output, and writable status
        // remain valid through the synchronous FileStreamInformation query.
        let result = unsafe {
            NtQueryInformationFile(
                handle.0,
                &mut status,
                buffer.as_mut_ptr().cast(),
                length as u32,
                22,
            )
        };
        if matches!(result as u32, 0x80000005 | 0xc0000023) {
            // Bound stream enumeration independently of kernel-reported lengths.
            if length >= 16 * 1024 * 1024 {
                return Err(ParseError::Unsupported);
            }
            buffer
                .try_reserve_exact(buffer.len())
                .map_err(|_| ParseError::Nomem)?;
            buffer.resize(buffer.len() * 2, 0);
            continue;
        }
        if result < 0 || status.information > length {
            return Err(ParseError::Read);
        }
        // SAFETY: A successful query initialized exactly information bytes.
        let bytes =
            unsafe { std::slice::from_raw_parts(buffer.as_ptr().cast::<u8>(), status.information) };
        return super::ntfs_streams::parse(bytes);
    }
}
/// Source NT-prefix walk resolves aliases by file identity rather than spelling.
fn relative_link_target(target: &[u16], root: CaptureIdentity) -> Option<usize> {
    if target.first() != Some(&92) || target.get(1) == Some(&92) {
        return None;
    }
    let mut directory: Option<FileHandle> = None;
    let mut begin = 0;
    let mut position = 0;
    loop {
        while position < target.len() && target[position] != 92 {
            position += 1;
        }
        while position < target.len() && target[position] == 92 {
            position += 1;
        }
        let bytes = u16::try_from((position - begin) * 2).ok()?;
        let mut name = UnicodeString {
            length: bytes,
            maximum: bytes,
            buffer: target[begin..].as_ptr().cast_mut(),
        };
        let mut attributes = ObjectAttributes {
            length: std::mem::size_of::<ObjectAttributes>() as u32,
            root: directory.as_ref().map_or(std::ptr::null_mut(), |h| h.0),
            name: &mut name,
            attributes: 0,
            security: std::ptr::null_mut(),
            quality: std::ptr::null_mut(),
        };
        let mut status = IoStatus::default();
        let mut output = std::ptr::null_mut();
        // SAFETY: Bounded UnicodeString borrows live target units. No-follow is deliberately absent,
        // matching the source alias-resolving prefix walk. NtOpenFile returns an owned handle only on success.
        let result = unsafe {
            NtOpenFile(
                &mut output,
                0x80 | 0x20,
                &mut attributes,
                &mut status,
                7,
                0x4000,
            )
        };
        if result >= 0 {
            let opened = FileHandle(output);
            let matched = information(&opened).is_ok_and(|info| {
                u64::from(info.volume) == root.device
                    && (u64::from(info.index_low) | (u64::from(info.index_high) << 32))
                        == root.inode
            });
            directory = Some(opened);
            begin = position;
            if matched {
                while position > 0 && target[position - 1] == 92 {
                    position -= 1;
                }
                return Some(position);
            }
        }
        if position == target.len() {
            break;
        }
    }
    None
}
struct Prior {
    node: OwnedDentry,
    security: Option<Vec<u8>>,
    streams: Vec<(usize, Arc<CapturedStream>)>,
}
struct Scanner<'a> {
    source: &'a Path,
    flags: i32,
    config: &'a CaptureConfig,
    callback: &'a mut ScanCallback<'a>,
    session: u64,
    plan: CapturePlan,
    inodes: HashMap<(u64, u64), usize>,
    prior: HashMap<(u64, u64), Prior>,
    canonical: HashSet<usize>,
    counts: (u64, u64, u64),
    last_path: Option<PathBuf>,
    last_status: i32,
    root_identity: Option<CaptureIdentity>,
}
impl Scanner<'_> {
    fn event(
        &mut self,
        message: i32,
        path: Option<&Path>,
        status: i32,
        error: Option<ParseError>,
    ) -> Result<bool, ParseError> {
        let mut event = ScanEvent {
            message,
            source: self.source,
            current_path: path,
            status,
            symlink_target: None,
            directories: self.counts.0,
            nondirectories: self.counts.1,
            bytes: self.counts.2,
            exclude: false,
            error,
        };
        (self.callback)(&mut event)?;
        Ok(event.exclude)
    }
    fn report(&mut self, path: &Path, status: i32, index: Option<usize>) -> Result<(), ParseError> {
        if (status == 0 && self.flags & 4 == 0) || (status != 0 && self.flags & 0x80 == 0) {
            return Ok(());
        }
        if let Some(index) = index {
            if self.plan.tree.nodes[index].attributes & (0x10 | 0x400) == 0x10 {
                self.counts.0 += 1;
            } else {
                self.counts.1 += 1;
            }
            if self.canonical.contains(&index) {
                self.counts.2 += self
                    .plan
                    .bindings
                    .iter()
                    .filter(|binding| binding.node == index)
                    .map(|binding| binding.stream.size)
                    .sum::<u64>();
            }
        }
        self.last_path = Some(path.to_owned());
        self.last_status = status;
        self.event(10, Some(path), status, None).map(|_| ())
    }
    fn load_reparse(
        &mut self,
        handle: &FileHandle,
        path: &Path,
        index: usize,
        identity: CaptureIdentity,
    ) -> Result<(), ParseError> {
        let mut buffer = [0u64; 2048];
        let mut returned = 0u32;
        // SAFETY: FSCTL_GET_REPARSE_POINT reads this no-follow handle into aligned16384-byte storage.
        let result = unsafe {
            DeviceIoControl(
                handle.0,
                0x900a8,
                std::ptr::null(),
                0,
                buffer.as_mut_ptr().cast(),
                16384,
                &mut returned,
                std::ptr::null_mut(),
            )
        };
        if result == 0 {
            return Err(ParseError::Readlink);
        }
        if !(8..=16384).contains(&returned) {
            return Err(ParseError::InvalidReparseData);
        }
        // SAFETY: Success initialized returned bytes within the stack buffer.
        let raw =
            unsafe { std::slice::from_raw_parts(buffer.as_ptr().cast::<u8>(), returned as usize) };
        let tag = u32::from_le_bytes(raw[..4].try_into().unwrap());
        if tag == 0x80000013 {
            self.plan.tree.nodes[index].attributes &= !0x400;
            return Ok(());
        }
        let mut adjusted = None;
        let mut fixed = false;
        if self.flags & 0x100 != 0
            && let Some(link) = super::reparse::parse(raw)
            && !(link.tag == super::reparse::SYMLINK && link.flags & 1 != 0)
        {
            let target: Vec<u16> = link
                .substitute
                .chunks_exact(2)
                .map(|pair| u16::from_le_bytes([pair[0], pair[1]]))
                .collect();
            if let Some(suffix) = self
                .root_identity
                .and_then(|root| relative_link_target(&target, root))
            {
                let mut substitute = Vec::new();
                substitute
                    .try_reserve_exact(12 + (target.len() - suffix) * 2)
                    .map_err(|_| ParseError::Nomem)?;
                substitute.extend("\\??\\X:".encode_utf16().flat_map(u16::to_le_bytes));
                substitute.extend(target[suffix..].iter().flat_map(|unit| unit.to_le_bytes()));
                match super::reparse::make(&link, &substitute, &substitute[8..]) {
                    Ok(bytes) => {
                        adjusted = Some(bytes);
                        fixed = true;
                    }
                    Err(ParseError::InvalidReparseData) => {}
                    Err(error) => return Err(error),
                }
            }
            if self.flags & 0x80 != 0 {
                let print = adjusted
                    .as_ref()
                    .and_then(|b| super::reparse::parse(b))
                    .map_or(link.print, |l| l.print);
                let units: Vec<_> = print
                    .chunks_exact(2)
                    .map(|p| u16::from_le_bytes([p[0], p[1]]))
                    .collect();
                let text = utf16_to_wtf8(&units)?;
                self.last_path = Some(path.to_owned());
                self.last_status = if fixed { 3 } else { 4 };
                let mut event = ScanEvent {
                    message: 10,
                    source: self.source,
                    current_path: Some(path),
                    status: self.last_status,
                    symlink_target: Some(&text),
                    directories: self.counts.0,
                    nondirectories: self.counts.1,
                    bytes: self.counts.2,
                    exclude: false,
                    error: None,
                };
                (self.callback)(&mut event)?;
            }
        }
        let bytes = adjusted.as_deref().unwrap_or(raw);
        let node = &mut self.plan.tree.nodes[index];
        node.inode_union = u64::from(tag)
            | (u64::from(u16::from_le_bytes(bytes[6..8].try_into().unwrap())) << 32)
            | (u64::from(!fixed) << 48);
        let mut data = Vec::new();
        data.try_reserve_exact(bytes.len() - 8)
            .map_err(|_| ParseError::Nomem)?;
        data.extend_from_slice(&bytes[8..]);
        if !data.is_empty() {
            node.main_hash = Sha1::digest(&data).into();
            self.plan
                .bindings
                .try_reserve(1)
                .map_err(|_| ParseError::Nomem)?;
            self.plan.bindings.push(CaptureBinding {
                node: index,
                slot: 0,
                stream: Arc::new(CapturedStream {
                    size: data.len() as u64,
                    identity,
                    source: CapturedSource::Inline(data),
                }),
            });
        }
        Ok(())
    }
    fn visit(
        &mut self,
        path: &Path,
        root: bool,
        image_root: bool,
    ) -> Result<Option<usize>, ParseError> {
        let before = self.plan.tree.nodes.len();
        match self.visit_inner(path, root, image_root) {
            Err(error)
                if !matches!(
                    error,
                    ParseError::AbortedByProgress | ParseError::UnknownProgressStatus
                ) =>
            {
                self.plan.tree.nodes.truncate(before);
                self.plan.identities.truncate(before);
                self.plan.bindings.retain(|binding| binding.node < before);
                self.inodes.retain(|_, index| *index < before);
                self.canonical.retain(|index| *index < before);
                if self.event(31, Some(path), self.last_status, Some(error))? {
                    Ok(None)
                } else {
                    Err(error)
                }
            }
            result => result,
        }
    }
    fn visit_inner(
        &mut self,
        path: &Path,
        root: bool,
        image_root: bool,
    ) -> Result<Option<usize>, ParseError> {
        let relative = path.strip_prefix(self.source).unwrap_or(path);
        let units: Vec<_> = relative.as_os_str().encode_wide().collect();
        let mut bytes = utf16_to_wtf8(&units)?;
        for byte in &mut bytes {
            if *byte == b'\\' {
                *byte = b'/';
            }
        }
        if self.config.excluded(&bytes)
            || (self.flags & 0x4000 != 0 && self.event(30, Some(path), 0, None)?)
        {
            self.report(path, 1, None)?;
            return Ok(None);
        }
        let pathname = wide(path)?;
        let handle = open(&pathname)?;
        let info = information(&handle)?;
        if info.attributes & (0x10 | 0x400) == 0x10 {
            check_directory_case_policy(&handle)?;
        }
        let volume = volume_flags(&handle)?;
        let streams = data_streams(&handle, info.attributes, volume)?;
        let directory = info.attributes & 0x10 != 0;
        let identity = CaptureIdentity {
            session: self.session,
            device: u64::from(info.volume),
            inode: u64::from(info.index_low) | (u64::from(info.index_high) << 32),
        };
        if root {
            self.root_identity = Some(identity);
        }
        let name = if root {
            Vec::new()
        } else {
            path.file_name()
                .ok_or(ParseError::InvalidParam)?
                .encode_wide()
                .flat_map(u16::to_le_bytes)
                .collect()
        };
        let index = self.plan.tree.nodes.len();
        let key = (identity.device, identity.inode);
        let shared = if !directory && info.links > 1 {
            self.inodes.get(&key).copied()
        } else {
            None
        };
        if let Some(first) = shared {
            let mut node = self.plan.tree.nodes[first].clone();
            node.name = name;
            node.short_name = short_name(&handle)?;
            node.children.clear();
            let group = first as u64 + 1;
            if node.attributes & 0x400 == 0 {
                node.inode_union = group;
                self.plan.tree.nodes[first].inode_union = group;
            }
            self.plan.tree.nodes.push(node);
            self.plan.identities.push(Some(identity));
            let bindings: Vec<_> = self
                .plan
                .bindings
                .iter()
                .filter(|binding| binding.node == first)
                .map(|binding| CaptureBinding {
                    node: index,
                    slot: binding.slot,
                    stream: binding.stream.clone(),
                })
                .collect();
            self.plan.bindings.extend(bindings);
            self.report(path, 0, Some(index))?;
            return Ok(Some(index));
        }
        if !directory
            && info.links > 1
            && let Some(prior) = self.prior.get(&key)
        {
            let mut node = prior.node.clone();
            node.name = name;
            node.short_name = short_name(&handle)?;
            node.children.clear();
            // The source inode table spans all ADDs in one update. Only the
            // first alias reads inode metadata; later aliases retain that SD
            // even if flags or the source DACL changed between commands.
            node.security_id = retain_security(
                &mut self.plan.tree,
                prior.security.as_deref().map(copy_security).transpose()?,
            )?;
            self.plan.tree.nodes.push(node);
            self.plan.identities.push(Some(identity));
            self.plan
                .bindings
                .extend(prior.streams.iter().map(|(slot, stream)| CaptureBinding {
                    node: index,
                    slot: *slot,
                    stream: stream.clone(),
                }));
            self.inodes.insert(key, index);
            self.report(path, 0, Some(index))?;
            return Ok(Some(index));
        }
        let mut node = OwnedDentry::new(name, info.attributes);
        node.creation_time = info.creation.value();
        node.last_access_time = info.access.value();
        node.last_write_time = info.write.value();
        node.short_name = short_name(&handle)?;
        node.tagged_items =
            crate::engine::windows_ntfs::capture_tags(handle.0, volume & 0x10000 != 0)?;
        node.security_id = retain_security(
            &mut self.plan.tree,
            security_descriptor(&handle, self.flags, volume)?,
        )?;
        self.plan
            .tree
            .nodes
            .try_reserve(1)
            .map_err(|_| ParseError::Nomem)?;
        self.plan
            .identities
            .try_reserve(1)
            .map_err(|_| ParseError::Nomem)?;
        self.plan.tree.nodes.push(node);
        self.plan.identities.push(Some(identity));
        self.inodes.insert(key, index);
        self.canonical.insert(index);
        if info.attributes & 0x400 != 0 {
            self.load_reparse(&handle, path, index, identity)?;
        }
        let size = u64::from(info.size_low) | (u64::from(info.size_high) << 32);
        if info.attributes & 0x4000 != 0 {
            // EFS raw export requires metadata handles to be closed first.
            drop(handle);
            let raw = crate::engine::windows_ntfs::export_encrypted(path)?;
            let size = raw.metadata().map_err(|_| ParseError::Stat)?.len();
            self.plan.bindings.push(CaptureBinding {
                node: index,
                slot: 0,
                stream: Arc::new(CapturedStream {
                    size,
                    identity,
                    source: CapturedSource::Temporary(raw),
                }),
            });
        } else if !directory && self.plan.tree.nodes[index].attributes & 0x400 == 0 && size != 0 {
            self.plan
                .bindings
                .try_reserve(1)
                .map_err(|_| ParseError::Nomem)?;
            self.plan.bindings.push(CaptureBinding {
                node: index,
                slot: 0,
                stream: Arc::new(CapturedStream {
                    size,
                    identity,
                    source: CapturedSource::File(path.to_owned()),
                }),
            });
        }
        for stream in streams {
            let name = stream.name;
            let size = stream.size;
            if name.is_empty() && (self.plan.tree.nodes[index].attributes & 0x400 == 0 || size == 0)
            {
                continue;
            }
            let node = &mut self.plan.tree.nodes[index];
            node.extra_streams
                .try_reserve(1)
                .map_err(|_| ParseError::Nomem)?;
            node.extra_streams.push(OwnedStream {
                name: name.iter().flat_map(|unit| unit.to_le_bytes()).collect(),
                ..OwnedStream::default()
            });
            let slot = node.extra_streams.len();
            if size != 0 {
                let mut stream_path = path.as_os_str().to_os_string();
                stream_path.push(":");
                stream_path.push(std::ffi::OsString::from_wide(&name));
                if name.is_empty() {
                    stream_path.push(":$DATA");
                }
                self.plan
                    .bindings
                    .try_reserve(1)
                    .map_err(|_| ParseError::Nomem)?;
                self.plan.bindings.push(CaptureBinding {
                    node: index,
                    slot,
                    stream: Arc::new(CapturedStream {
                        size,
                        identity,
                        source: CapturedSource::File(PathBuf::from(stream_path)),
                    }),
                });
            }
        }
        if directory && self.plan.tree.nodes[index].attributes & 0x400 == 0 {
            for entry in std::fs::read_dir(path).map_err(|_| ParseError::Open)? {
                let entry = entry.map_err(|_| ParseError::Read)?;
                if let Some(child) = self.visit(&entry.path(), false, image_root)? {
                    self.plan.tree.nodes[index]
                        .children
                        .try_reserve(1)
                        .map_err(|_| ParseError::Nomem)?;
                    self.plan.tree.nodes[index].children.push(child);
                }
            }
        }
        self.report(path, 0, Some(index))?;
        Ok(Some(index))
    }
}
/// Scan ordinary Windows files with preserved raw metadata and deferred payloads.
/// Security descriptors are read through the source-compatible NT query policy.
pub fn scan_source<'a>(
    source: &'a Path,
    image_root: bool,
    flags: i32,
    config: &'a CaptureConfig,
    callback: &'a mut ScanCallback<'a>,
) -> Result<CapturePlan, ParseError> {
    scan_source_seeded(
        source,
        image_root,
        flags,
        config,
        &CapturePlan::default(),
        crate::engine::handles::new_identity(),
        callback,
    )
}
/// Scan another source while preserving transaction-scoped hard-link ownership.
pub fn scan_source_seeded<'a>(
    source: &'a Path,
    image_root: bool,
    flags: i32,
    config: &'a CaptureConfig,
    existing: &CapturePlan,
    session: u64,
    callback: &'a mut ScanCallback<'a>,
) -> Result<CapturePlan, ParseError> {
    let mut prior = HashMap::new();
    for (index, identity) in existing.identities.iter().enumerate() {
        if let Some(identity) = identity
            && identity.session == session
        {
            let node = existing
                .tree
                .nodes
                .get(index)
                .ok_or(ParseError::InvalidMetadataResource)?;
            if node.attributes & 0x10 != 0 {
                continue;
            }
            let key = (identity.device, identity.inode);
            if prior.contains_key(&key) {
                continue;
            }
            let security = if node.security_id == u32::MAX {
                None
            } else {
                Some(copy_security(
                    existing
                        .tree
                        .security_descriptors
                        .get(node.security_id as usize)
                        .ok_or(ParseError::InvalidMetadataResource)?,
                )?)
            };
            prior.try_reserve(1).map_err(|_| ParseError::Nomem)?;
            prior.insert(
                key,
                Prior {
                    node: node.clone(),
                    security,
                    streams: existing
                        .bindings
                        .iter()
                        .filter(|binding| binding.node == index)
                        .map(|binding| (binding.slot, binding.stream.clone()))
                        .collect(),
                },
            );
        }
    }
    let mut source_units = wide(source)?;
    source_units.pop();
    let normalized_source = PathBuf::from(std::ffi::OsString::from_wide(&source_units));
    let mut scanner = Scanner {
        source: &normalized_source,
        flags,
        config,
        callback,
        session,
        plan: CapturePlan::default(),
        inodes: HashMap::new(),
        prior,
        canonical: HashSet::new(),
        counts: (0, 0, 0),
        last_path: None,
        last_status: 0,
        root_identity: None,
    };
    scanner.event(9, None, 0, None)?;
    scanner.visit(&normalized_source, true, image_root)?;
    let last = scanner.last_path.clone();
    scanner.event(11, last.as_deref(), scanner.last_status, None)?;
    if image_root
        && scanner
            .plan
            .tree
            .nodes
            .first()
            .is_some_and(|node| node.attributes & (0x10 | 0x400) != 0x10)
    {
        return Err(ParseError::Notdir);
    }
    Ok(scanner.plan)
}
