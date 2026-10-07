// SPDX-License-Identifier: LGPL-2.1-or-later
//! Handle-relative NTFS extraction, including named streams and reparse points.

use super::blob::{BlobSource, blob_source, chunk_end, read_chunk};
use super::paths::Selection;
use crate::engine::collections::FallibleCollections as _;
use crate::engine::collections::FallibleMap as _;
use crate::engine::{
    handles::WimHandle,
    progress::{ExtractProgress, ProgressInfo, next_progress},
};
use sha1::{Digest, Sha1};
use std::{
    ffi::c_void,
    fs::File,
    io::{Seek, SeekFrom, Write},
    os::windows::{
        ffi::{OsStrExt, OsStringExt},
        io::{AsRawHandle, FromRawHandle},
    },
    path::Path,
};
use wim_format::{
    ParseError,
    metadata::{Dentry, Metadata, StreamType},
    platform_text::wtf8_to_utf16z,
};

use hashbrown::HashMap;
use std::vec::Vec;

type Handle = *mut c_void;
#[repr(C)]
struct UnicodeString {
    length: u16,
    maximum: u16,
    buffer: *const u16,
}
#[repr(C)]
struct ObjectAttributes {
    length: u32,
    root: Handle,
    name: *const UnicodeString,
    attributes: u32,
    security: *const c_void,
    quality: *const c_void,
}
#[repr(C)]
#[derive(Default)]
struct IoStatus {
    status: usize,
    information: usize,
}
#[repr(C)]
struct BasicInformation {
    creation: i64,
    access: i64,
    write: i64,
    change: i64,
    attributes: u32,
}
#[link(name = "ntdll")]
unsafe extern "system" {
    fn NtCreateFile(
        out: *mut Handle,
        access: u32,
        attr: *const ObjectAttributes,
        io: *mut IoStatus,
        allocation: *const i64,
        attributes: u32,
        share: u32,
        disposition: u32,
        options: u32,
        ea: *const c_void,
        ea_length: u32,
    ) -> i32;
    fn NtSetInformationFile(
        handle: Handle,
        io: *mut IoStatus,
        data: *const c_void,
        length: u32,
        class: u32,
    ) -> i32;
    fn NtQueryInformationFile(
        handle: Handle,
        io: *mut IoStatus,
        data: *mut c_void,
        length: u32,
        class: u32,
    ) -> i32;
    fn NtFsControlFile(
        handle: Handle,
        event: Handle,
        apc: *const c_void,
        context: *const c_void,
        io: *mut IoStatus,
        code: u32,
        input: *const c_void,
        input_length: u32,
        output: *mut c_void,
        output_length: u32,
    ) -> i32;
    fn NtSetSecurityObject(handle: Handle, information: u32, descriptor: *const c_void) -> i32;
    fn RtlDosPathNameToNtPathName_U_WithStatus(
        path: *const u16,
        output: *mut UnicodeString,
        file_part: *mut *mut u16,
        relative: *mut c_void,
    ) -> i32;
    fn RtlFreeUnicodeString(path: *mut UnicodeString);
}
struct NtPath(UnicodeString);
impl Drop for NtPath {
    fn drop(&mut self) {
        // SAFETY: The RTL allocated this complete UnicodeString on successful conversion.
        unsafe { RtlFreeUnicodeString(&mut self.0) };
    }
}
fn open_target(target: &[u16]) -> Result<File, ParseError> {
    let mut path = UnicodeString {
        length: 0,
        maximum: 0,
        buffer: std::ptr::null(),
    };
    // SAFETY: Backend owns the terminated target; writable output uses the NT UnicodeString ABI.
    let status = unsafe {
        RtlDosPathNameToNtPathName_U_WithStatus(
            target.as_ptr(),
            &mut path,
            std::ptr::null_mut(),
            std::ptr::null_mut(),
        )
    };
    if status < 0 {
        return Err(if status as u32 == 0xc000_0017 {
            ParseError::Nomem
        } else {
            ParseError::InvalidParam
        });
    }
    let path = NtPath(path);
    let attr = ObjectAttributes {
        length: std::mem::size_of::<ObjectAttributes>() as u32,
        root: std::ptr::null_mut(),
        name: &path.0,
        attributes: 0,
        security: std::ptr::null(),
        quality: std::ptr::null(),
    };
    let mut io = IoStatus::default();
    let mut handle = std::ptr::null_mut();
    // SAFETY: Converted absolute NT target and output records remain live. Match source
    // FILE_TRAVERSE/OPEN_IF/DIRECTORY/BACKUP; caller-selected root reparse points are followed.
    let status = unsafe {
        NtCreateFile(
            &mut handle,
            0x20,
            &attr,
            &mut io,
            std::ptr::null(),
            0,
            7,
            3,
            1 | 0x4000,
            std::ptr::null(),
            0,
        )
    };
    if status < 0 {
        return Err(ParseError::Opendir);
    }
    // SAFETY: The successful NT directory handle transfers unique ownership.
    Ok(unsafe { File::from_raw_handle(handle) })
}

fn units(bytes: &[u8]) -> Result<Vec<u16>, ParseError> {
    let mut text = Vec::new();
    text.try_reserve(bytes.len() / 2)
        .map_err(|_| ParseError::Nomem)?;
    text.try_extend(
        bytes
            .chunks_exact(2)
            .map(|b| u16::from_le_bytes([b[0], b[1]])),
    )
    .map_err(|_| ParseError::Nomem)?;
    Ok(text)
}
fn leaf(bytes: &[u8]) -> Result<Vec<u16>, ParseError> {
    let text = units(bytes)?;
    if text.is_empty()
        || text.as_slice() == [46]
        || text.as_slice() == [46, 46]
        || text.iter().any(|&u| matches!(u, 0 | 47 | 92 | 58))
    {
        return Err(ParseError::InvalidMetadataResource);
    }
    Ok(text)
}
fn open(
    parent: &File,
    name: &[u16],
    access: u32,
    attributes: u32,
    disposition: u32,
    options: u32,
) -> Result<File, i32> {
    open_with_information(parent, name, access, attributes, disposition, options).map(|v| v.0)
}
fn open_with_information(
    parent: &File,
    name: &[u16],
    access: u32,
    attributes: u32,
    disposition: u32,
    options: u32,
) -> Result<(File, usize), i32> {
    let length = u16::try_from(name.len().saturating_mul(2)).map_err(|_| 0xc000_0106u32 as i32)?;
    let name = UnicodeString {
        length,
        maximum: length,
        buffer: name.as_ptr(),
    };
    let attr = ObjectAttributes {
        length: std::mem::size_of::<ObjectAttributes>() as u32,
        root: parent.as_raw_handle(),
        name: &name,
        attributes: 0,
        security: std::ptr::null(),
        quality: std::ptr::null(),
    };
    let mut io = IoStatus::default();
    let mut handle = std::ptr::null_mut();
    // SAFETY: All NT records and UTF-16 units remain live; success transfers a new handle.
    let status = unsafe {
        NtCreateFile(
            &mut handle,
            access | 0x0010_0000,
            &attr,
            &mut io,
            std::ptr::null(),
            attributes,
            7,
            disposition,
            options | 0x20 | 0x4000 | 0x0020_0000,
            std::ptr::null(),
            0,
        )
    };
    if status < 0 {
        return Err(status);
    }
    // SAFETY: Successful NtCreateFile returns a uniquely owned synchronous file handle.
    Ok((unsafe { File::from_raw_handle(handle) }, io.information))
}
fn stream_name(base: &[u16], stream: &[u8]) -> Result<Vec<u16>, ParseError> {
    let mut name = Vec::new();
    name.try_extend_from_slice(base)
        .map_err(|_| ParseError::Nomem)?;
    if !stream.is_empty() {
        name.try_push(58).map_err(|_| ParseError::Nomem)?;
        name.try_extend_from_slice(&leaf(stream)?)
            .map_err(|_| ParseError::Nomem)?;
        name.try_extend(":$DATA".encode_utf16())
            .map_err(|_| ParseError::Nomem)?;
    }
    Ok(name)
}
fn fix_reparse(raw: &[u8], target: &[u16]) -> Result<Option<Vec<u8>>, ParseError> {
    use crate::engine::capture::reparse;
    let Some(link) = reparse::parse(raw) else {
        return Ok(None);
    };
    if link.tag == reparse::SYMLINK && link.flags & 1 != 0 {
        return Ok(None);
    }
    let mut path = UnicodeString {
        length: 0,
        maximum: 0,
        buffer: std::ptr::null(),
    };
    // SAFETY: Retained target is terminated and the output NT string is writable.
    if unsafe {
        RtlDosPathNameToNtPathName_U_WithStatus(
            target.as_ptr(),
            &mut path,
            std::ptr::null_mut(),
            std::ptr::null_mut(),
        )
    } < 0
    {
        return Err(ParseError::InvalidParam);
    }
    let path = NtPath(path);
    // SAFETY: Successful RTL conversion initialized length bytes owned by path.
    let target = unsafe { std::slice::from_raw_parts(path.0.buffer, path.0.length as usize / 2) };
    let substitute = units(link.substitute)?;
    // Upstream recognizes these NT namespace prefixes before skipping the device.
    let prefixes: &[&[u16]] = &[
        &[92, 63, 63, 92],
        &[92, 68, 111, 115, 68, 101, 118, 105, 99, 101, 115, 92],
        &[92, 68, 101, 118, 105, 99, 101, 92],
    ];
    let mut relative = substitute.as_slice();
    if let Some(prefix) = prefixes.iter().find(|p| relative.starts_with(p)) {
        relative = &relative[prefix.len()..];
        while relative.first() == Some(&92) {
            relative = &relative[1..];
        }
        relative = &relative[relative
            .iter()
            .position(|&u| u == 92)
            .unwrap_or(relative.len())..];
    }
    while relative.starts_with(&[92, 92]) {
        relative = &relative[1..];
    }
    let target = if target.last() == Some(&92) && relative.first() == Some(&92) {
        &target[..target.len() - 1]
    } else {
        target
    };
    let mut bytes = Vec::new();
    bytes
        .try_extend(target.iter().chain(relative).flat_map(|u| u.to_le_bytes()))
        .map_err(|_| ParseError::Nomem)?;
    let print = if bytes.starts_with(&[92, 0, 63, 0, 63, 0, 92, 0]) {
        &bytes[8..]
    } else {
        bytes.as_slice()
    };
    reparse::make(&link, &bytes, print).map(Some)
}
fn fsctl(file: &File, code: u32, input: &[u8]) -> i32 {
    let mut io = IoStatus::default();
    // SAFETY: Synchronous handle, initialized IO status and readable control input
    // remain live; no event, APC, or output buffer is requested.
    unsafe {
        NtFsControlFile(
            file.as_raw_handle(),
            std::ptr::null_mut(),
            std::ptr::null(),
            std::ptr::null(),
            &mut io,
            code,
            input.as_ptr().cast(),
            input.len() as u32,
            std::ptr::null_mut(),
            0,
        )
    }
}
fn storage_attributes(file: &File, attributes: u32) -> Result<(), ParseError> {
    if attributes & 0x800 != 0 && fsctl(file, 0x9c040, &1u16.to_ne_bytes()) < 0 {
        return Err(ParseError::SetAttributes);
    }
    if attributes & 0x200 != 0 && fsctl(file, 0x900c4, &[]) < 0 {
        return Err(ParseError::SetAttributes);
    }
    Ok(())
}
fn set_info(file: &File, bytes: &[u8], class: u32) -> i32 {
    let mut io = IoStatus::default();
    // SAFETY: File is live and the class-specific input remains readable during the call.
    unsafe {
        NtSetInformationFile(
            file.as_raw_handle(),
            &mut io,
            bytes.as_ptr().cast(),
            bytes.len() as u32,
            class,
        )
    }
}
fn check_directory(file: &File) -> Result<(), ParseError> {
    let mut info = BasicInformation {
        creation: 0,
        access: 0,
        write: 0,
        change: 0,
        attributes: 0,
    };
    let mut io = IoStatus::default();
    // SAFETY: Correctly aligned writable class-4 record and a live directory handle.
    let status = unsafe {
        NtQueryInformationFile(
            file.as_raw_handle(),
            &mut io,
            (&mut info as *mut BasicInformation).cast(),
            std::mem::size_of::<BasicInformation>() as u32,
            4,
        )
    };
    if status < 0 || info.attributes & 0x410 != 0x10 {
        return Err(ParseError::Mkdir);
    }
    Ok(())
}
fn clear_attributes(file: &File) {
    let info = BasicInformation {
        creation: 0,
        access: 0,
        write: 0,
        change: 0,
        attributes: 0x80,
    };
    let mut io = IoStatus::default();
    // SAFETY: Live directory and aligned NT record; clearing existing directory
    // attributes is deliberately best effort, as in create_directory().
    unsafe {
        NtSetInformationFile(
            file.as_raw_handle(),
            &mut io,
            (&info as *const BasicInformation).cast(),
            std::mem::size_of::<BasicInformation>() as u32,
            4,
        );
    }
}
fn delete_existing(parent: &File, name: &[u16]) -> Result<(), ParseError> {
    let file = open(parent, name, 0x0001_0100, 0, 1, 0).map_err(|_| ParseError::Open)?;
    let info = BasicInformation {
        creation: 0,
        access: 0,
        write: 0,
        change: 0,
        attributes: 0x80,
    };
    let mut io = IoStatus::default();
    // SAFETY: Correctly aligned basic information is readable and file is live.
    unsafe {
        NtSetInformationFile(
            file.as_raw_handle(),
            &mut io,
            (&info as *const BasicInformation).cast(),
            std::mem::size_of::<BasicInformation>() as u32,
            4,
        );
    }
    if set_info(&file, &[1], 13) < 0 {
        return Err(ParseError::Open);
    }
    Ok(())
}
fn create_file(parent: &File, name: &[u16]) -> Result<File, ParseError> {
    match open(parent, name, 0x0012_019f | 0x0001_0000, 4, 2, 0x40) {
        Ok(file) => Ok(file),
        Err(status) if status as u32 == 0xc000_0035 => {
            delete_existing(parent, name)?;
            open(parent, name, 0x0012_019f | 0x0001_0000, 4, 2, 0x40).map_err(|_| ParseError::Open)
        }
        Err(_) => Err(ParseError::Open),
    }
}
fn short_name(file: &File, name: &[u8], flags: u32) -> Result<(), ParseError> {
    // NT requires room for at least two UTF-16 units even for an empty short name.
    let size = 4 + name.len().max(2) + 2;
    let mut buffer = Vec::new();
    buffer
        .try_extend(std::iter::repeat_n(0u64, size.div_ceil(8)))
        .map_err(|_| ParseError::Nomem)?;
    // SAFETY: A u64 collection provides aligned writable storage for the NT record.
    let bytes = unsafe { std::slice::from_raw_parts_mut(buffer.as_mut_ptr().cast::<u8>(), size) };
    bytes[..4].copy_from_slice(&(name.len() as u32).to_le_bytes());
    bytes[4..4 + name.len()].copy_from_slice(name);
    if set_info(file, bytes, 40) < 0 && flags & 0x4000 != 0 {
        return Err(ParseError::SetShortName);
    }
    Ok(())
}
fn hardlink(source: &File, parent: &File, name: &[u16]) -> Result<(), ParseError> {
    let size = 20 + name.len() * 2 + 2;
    let mut buffer = Vec::new();
    buffer
        .try_extend(std::iter::repeat_n(0u64, size.div_ceil(8)))
        .map_err(|_| ParseError::Nomem)?;
    // SAFETY: Aligned storage fits the x64 FILE_LINK_INFORMATION header and UTF-16 name.
    let bytes = unsafe { std::slice::from_raw_parts_mut(buffer.as_mut_ptr().cast::<u8>(), size) };
    bytes[0] = 1;
    bytes[8..16].copy_from_slice(&(parent.as_raw_handle() as usize as u64).to_ne_bytes());
    bytes[16..20].copy_from_slice(&((name.len() * 2) as u32).to_le_bytes());
    for (dst, unit) in bytes[20..].chunks_exact_mut(2).zip(name) {
        dst.copy_from_slice(&unit.to_le_bytes());
    }
    for _ in 0..32 {
        if set_info(source, bytes, 11) >= 0 {
            return Ok(());
        }
    }
    Err(ParseError::Link)
}
fn security(file: &File, descriptor: &[u8], flags: u32) -> Result<(), ParseError> {
    let mut aligned = Vec::new();
    aligned
        .try_extend(std::iter::repeat_n(0u64, descriptor.len().div_ceil(8)))
        .map_err(|_| ParseError::Nomem)?;
    // SAFETY: Aligned storage contains the complete self-relative descriptor.
    let bytes = unsafe {
        std::slice::from_raw_parts_mut(aligned.as_mut_ptr().cast::<u8>(), descriptor.len())
    };
    bytes.copy_from_slice(descriptor);
    if bytes.len() >= 4 {
        let mut control = u16::from_le_bytes([bytes[2], bytes[3]]);
        if control & 0x400 != 0 {
            control |= 0x100;
        }
        if control & 0x800 != 0 {
            control |= 0x200;
        }
        bytes[2..4].copy_from_slice(&control.to_le_bytes());
    }
    let mut information = 1 | 2 | 4 | 8 | 16 | 0x10000;
    loop {
        // SAFETY: Handle and copied self-relative descriptor remain live through the call.
        let status = unsafe {
            NtSetSecurityObject(file.as_raw_handle(), information, bytes.as_ptr().cast())
        };
        if status >= 0 {
            return Ok(());
        }
        if flags & 0x80 != 0 {
            return Err(ParseError::SetSecurity);
        }
        if !matches!(status as u32, 0xc000_0022 | 0xc000_0061) {
            return Ok(());
        }
        if information & 8 != 0 {
            information &= !(8 | 16 | 0x10000);
        } else if information & 4 != 0 {
            information &= !4;
        } else if information & 1 != 0 {
            information &= !1;
        } else {
            return Ok(());
        }
    }
}
fn metadata(
    parent: &File,
    name: &[u16],
    inode: &Dentry<'_>,
    descriptor: Option<&[u8]>,
    flags: u32,
) -> Result<(), ParseError> {
    let mut access = 0x100 | 0x10 | 0x2 | 0x40000 | 0x80000 | 0x0100_0000;
    let file = loop {
        match open(parent, name, access, 0, 1, 0) {
            Ok(file) => break file,
            Err(status) if matches!(status as u32, 0xc000_0022 | 0xc000_0061) => {
                if access & 0x0100_0000 != 0 {
                    access &= !0x0100_0000;
                } else if access & 0x40000 != 0 {
                    access &= !0x40000;
                } else if access & 0x80000 != 0 {
                    access &= !0x80000;
                } else {
                    return Err(ParseError::Open);
                }
            }
            Err(_) => return Err(ParseError::Open),
        }
    };
    crate::engine::windows_ntfs::apply_tags(
        file.as_raw_handle(),
        inode.tagged_item(1, 16),
        inode.tagged_item(2, 0),
    )?;
    if flags & 0x40 == 0
        && let Some(descriptor) = descriptor
    {
        security(&file, descriptor, flags)?;
    }
    let attributes = if flags & 0x0010_0000 != 0 {
        0x80
    } else {
        let attributes = inode.attributes & !(0x400 | 0x10 | 0x4000 | 0x200 | 0x800);
        if attributes == 0 { 0x80 } else { attributes }
    };
    let info = BasicInformation {
        creation: inode.creation_time as i64,
        access: inode.last_access_time as i64,
        write: inode.last_write_time as i64,
        change: 0,
        attributes,
    };
    let mut io = IoStatus::default();
    // SAFETY: Aligned NT basic information and live handle match class 4's ABI.
    let status = unsafe {
        NtSetInformationFile(
            file.as_raw_handle(),
            &mut io,
            (&info as *const BasicInformation).cast(),
            std::mem::size_of::<BasicInformation>() as u32,
            4,
        )
    };
    if status < 0 {
        return Err(ParseError::SetAttributes);
    }
    Ok(())
}

struct Progress<'a> {
    handle: &'a WimHandle,
    info: ProgressInfo,
    next: u64,
}
#[derive(Clone, Copy)]
struct Representative {
    last: usize,
    path: usize,
}
impl Progress<'_> {
    fn call(&mut self, message: i32) -> Result<(), ParseError> {
        // SAFETY: Backend owns all borrowed progress strings and the initialized extract union.
        unsafe { self.handle.progress.get().call(message, &mut self.info) }
    }
    fn phase(&mut self, message: i32, current: u64, total: u64) -> Result<(), ParseError> {
        // SAFETY: This backend initializes and exclusively uses the extract union member.
        let info = unsafe { &mut self.info.extract };
        info.current_file_count = current;
        info.end_file_count = total;
        self.call(message)
    }
    fn data(&mut self, bytes: u64, streams: u64) -> Result<(), ParseError> {
        // SAFETY: The extract member is initialized throughout the operation.
        let info = unsafe { &mut self.info.extract };
        info.completed_bytes += bytes;
        info.completed_streams += streams;
        if info.completed_bytes >= self.next {
            self.call(4)?;
            // SAFETY: Callback registration does not change the active union member.
            let info = unsafe { self.info.extract };
            self.next = next_progress(info.completed_bytes, info.total_bytes, self.next);
        }
        Ok(())
    }
}

#[derive(Clone, Copy)]
struct StreamTarget {
    node: usize,
    stream: usize,
}
struct Layout {
    root_selected: bool,
    names: Vec<Vec<u16>>,
    order: Vec<usize>,
    parents: Vec<Option<usize>>,
    representatives: HashMap<usize, Representative>,
    groups: HashMap<[u8; 20], Vec<StreamTarget>>,
}
fn layout(tree: &Metadata<'_>, selection: Option<&Selection>) -> Result<Layout, ParseError> {
    let root_selected = selection.is_none_or(|selection| selection.nodes.contains(&0));
    let mut names = Vec::new();
    let mut representatives: HashMap<usize, Representative> = HashMap::new();
    let mut groups: HashMap<[u8; 20], Vec<StreamTarget>> = HashMap::new();
    let mut order = Vec::new();
    let mut stack = Vec::new();
    stack.try_push(0usize).map_err(|_| ParseError::Nomem)?;
    while let Some(index) = stack.pop() {
        let node = tree
            .nodes
            .get(index)
            .ok_or(ParseError::InvalidMetadataResource)?;
        order.try_push(index).map_err(|_| ParseError::Nomem)?;
        stack
            .try_extend(node.children.iter().rev().copied())
            .map_err(|_| ParseError::Nomem)?;
    }
    if let Some(selection) = selection {
        order.clear();
        // Root holds the target directory even when flattened selections omit it.
        order.try_push(0).map_err(|_| ParseError::Nomem)?;
        order
            .try_extend(selection.nodes.iter().copied().filter(|&i| i != 0))
            .map_err(|_| ParseError::Nomem)?;
    }
    let mut parents = Vec::new();
    parents
        .try_extend(tree.nodes.iter().enumerate().map(|(index, node)| {
            selection
                .and_then(|s| s.parents.get(&index).copied())
                .or(node.parent)
        }))
        .map_err(|_| ParseError::Nomem)?;
    names
        .try_extend((0..tree.nodes.len()).map(|_| Vec::new()))
        .map_err(|_| ParseError::Nomem)?;
    for &index in &order {
        if index == 0 && !root_selected {
            continue;
        }
        let node = &tree.nodes[index];
        let inode = tree
            .inode_entry(index)
            .ok_or(ParseError::InvalidMetadataResource)?;
        if inode.tagged_item(0x337d_d874, 0).is_some()
            || inode.streams.iter().any(|s| {
                !matches!(
                    s.kind,
                    StreamType::Data | StreamType::ReparsePoint | StreamType::EncryptedRaw
                ) && s.hash != [0; 20]
            })
        {
            return Err(ParseError::Unsupported);
        }
        if inode.attributes & 0x400 != 0 && !node.children.is_empty() {
            return Err(ParseError::InvalidMetadataResource);
        }
        names[index] = if index == 0 {
            units(&[])?
        } else {
            leaf(node.entry.name)?
        };
        for stream in &inode.streams {
            if !stream.name.is_empty() {
                leaf(stream.name)?;
            }
        }
    }
    for &index in &order {
        if index == 0 && !root_selected {
            continue;
        }
        let node = &tree.nodes[index];
        if !node.entry.is_directory() {
            if let Some(previous) = representatives.get_mut(&node.inode) {
                // Extraction alias lists are built by head insertion. The last
                // visited alias triggers creation; its first DOS-bearing alias
                // in that reversed list determines the physical creation path.
                previous.last = index;
                if !node.entry.short_name.is_empty()
                    || tree.nodes[previous.path].entry.short_name.is_empty()
                {
                    previous.path = index;
                }
            } else {
                representatives
                    .try_insert_reserved(
                        node.inode,
                        Representative {
                            last: index,
                            path: index,
                        },
                    )
                    .map_err(|_| ParseError::Nomem)?;
            }
        }
    }
    for &index in &order {
        if index == 0 && !root_selected {
            continue;
        }
        if let Some(representative) = representatives.get(&tree.nodes[index].inode)
            && representative.last != index
        {
            continue;
        }
        let inode = tree
            .inode_entry(index)
            .ok_or(ParseError::InvalidMetadataResource)?;
        for (stream_index, stream) in inode.streams.iter().enumerate() {
            if stream.hash == [0; 20] {
                continue;
            }
            if !groups.contains_key(&stream.hash) {
                groups
                    .try_insert_reserved(stream.hash, Vec::new())
                    .map_err(|_| ParseError::Nomem)?;
            }
            groups
                .get_mut(&stream.hash)
                .ok_or(ParseError::InvalidMetadataResource)?
                .try_push(StreamTarget {
                    node: index,
                    stream: stream_index,
                })
                .map_err(|_| ParseError::Nomem)?;
        }
    }
    Ok(Layout {
        root_selected,
        names,
        order,
        parents,
        representatives,
        groups,
    })
}
pub(crate) fn required_stream_count(tree: &Metadata<'_>, _flags: u32) -> Result<u64, ParseError> {
    let selected = layout(tree, None)?;
    Ok(selected
        .groups
        .values()
        .map(|nodes| nodes.len() as u64)
        .sum())
}
pub(crate) struct PreparedExtraction<'handle, 'tree, 'data> {
    tree: &'tree Metadata<'data>,
    flags: u32,
    root_selected: bool,
    names: Vec<Vec<u16>>,
    order: Vec<usize>,
    parents: Vec<Option<usize>>,
    groups: HashMap<[u8; 20], Vec<StreamTarget>>,
    files: Vec<Option<File>>,
    progress: Progress<'handle>,
    reparses: HashMap<usize, Vec<u8>>,
    encrypted: HashMap<usize, File>,
    _target: Vec<u16>,
    _filename: Vec<u16>,
    _image_name: Vec<u16>,
}
impl<'handle, 'tree, 'data> PreparedExtraction<'handle, 'tree, 'data> {
    pub(crate) fn prepare_image(
        handle: &'handle WimHandle,
        image: i32,
        target: &Path,
        flags: u32,
        tree: &'tree Metadata<'data>,
        totals: (u64, u64),
    ) -> Result<Self, ParseError> {
        Self::prepare(
            handle,
            image,
            target,
            flags,
            tree,
            layout(tree, None)?,
            totals,
        )
    }
    fn prepare(
        handle: &'handle WimHandle,
        image: i32,
        target: &Path,
        flags: u32,
        tree: &'tree Metadata<'data>,
        selected: Layout,
        totals: (u64, u64),
    ) -> Result<Self, ParseError> {
        let (total_bytes, total_streams) = totals;
        let Layout {
            root_selected,
            names,
            order,
            parents,
            representatives,
            groups,
        } = selected;
        let mut files = Vec::new();
        files
            .try_extend(std::iter::repeat_with(|| None::<File>).take(tree.nodes.len()))
            .map_err(|_| ParseError::Nomem)?;
        let mut target_text = Vec::new();
        target_text
            .try_extend(target.as_os_str().encode_wide())
            .map_err(|_| ParseError::Nomem)?;
        target_text.try_push(0).map_err(|_| ParseError::Nomem)?;
        let image_name = wtf8_to_utf16z(handle.xml.name_bytes(image).unwrap_or_default())?;
        let mut filename = Vec::new();
        if let Some(path) = handle.filename.as_deref() {
            filename
                .try_extend(path.as_os_str().encode_wide())
                .map_err(|_| ParseError::Nomem)?;
            filename.try_push(0).map_err(|_| ParseError::Nomem)?;
        }
        let mut info = ProgressInfo::zeroed();
        info.extract = ExtractProgress {
            image: image as u32,
            extract_flags: flags,
            wimfile_name: if filename.is_empty() {
                std::ptr::null()
            } else {
                filename.as_ptr()
            },
            image_name: image_name.as_ptr(),
            target: target_text.as_ptr(),
            reserved: std::ptr::null(),
            total_bytes,
            completed_bytes: 0,
            total_streams,
            completed_streams: 0,
            part_number: 0,
            total_parts: 0,
            guid: [0; 16],
            current_file_count: 0,
            end_file_count: 0,
        };
        let mut progress = Progress {
            handle,
            info,
            next: 0,
        };
        progress.call(0)?;
        files[0] = Some(open_target(&target_text)?);
        let count = order.len() as u64;
        progress.phase(3, 0, count)?;
        let root = open(
            files[0].as_ref().ok_or(ParseError::Opendir)?,
            &[],
            0x0012_019f,
            4,
            3,
            1,
        )
        .map_err(|_| ParseError::Mkdir)?;
        if root_selected && flags & 0x0010_0000 == 0 {
            clear_attributes(&root);
        }
        files[0] = Some(root);
        let mut completed = 1u64;
        for &index in order.iter().skip(1) {
            let node = &tree.nodes[index];
            if !node.entry.is_directory() {
                continue;
            }
            let parent = parents[index].ok_or(ParseError::InvalidMetadataResource)?;
            let (directory, disposition) = open_with_information(
                files[parent].as_ref().ok_or(ParseError::Mkdir)?,
                &names[index],
                0x0012_019f | 0x10000,
                4,
                3,
                1,
            )
            .map_err(|_| ParseError::Mkdir)?;
            check_directory(&directory)?;
            if disposition == 1 && flags & 0x0010_0000 == 0 {
                clear_attributes(&directory);
            }
            storage_attributes(&directory, node.entry.attributes & !0x200)?;
            short_name(&directory, node.entry.short_name, flags)?;
            files[index] = Some(directory);
            completed += 1;
            if completed.is_multiple_of(500) {
                progress.phase(3, completed, count)?;
            }
        }
        for &index in order.iter().skip(1) {
            let node = &tree.nodes[index];
            if node.entry.is_directory() {
                continue;
            }
            let representative = *representatives
                .get(&node.inode)
                .ok_or(ParseError::InvalidMetadataResource)?;
            if representative.last == index {
                let chosen = representative.path;
                let parent = files[parents[chosen].ok_or(ParseError::InvalidMetadataResource)?]
                    .as_ref()
                    .ok_or(ParseError::Mkdir)?;
                let file = create_file(parent, &names[chosen])?;
                storage_attributes(
                    &file,
                    tree.inode_entry(index)
                        .ok_or(ParseError::InvalidMetadataResource)?
                        .attributes,
                )?;
                short_name(&file, tree.nodes[chosen].entry.short_name, flags)?;
                for &alias in order.iter().rev() {
                    let alias_node = &tree.nodes[alias];
                    if alias != chosen && alias_node.inode == node.inode {
                        let parent = files
                            [parents[alias].ok_or(ParseError::InvalidMetadataResource)?]
                        .as_ref()
                        .ok_or(ParseError::Mkdir)?;
                        hardlink(&file, parent, &names[alias])?;
                    }
                }
            }
            completed += 1;
            if completed.is_multiple_of(500) {
                progress.phase(3, completed, count)?;
            }
        }
        let mut reparses = HashMap::new();
        for &index in &order {
            if index == 0 && !root_selected {
                continue;
            }
            if let Some(representative) = representatives.get(&tree.nodes[index].inode)
                && representative.last != index
            {
                continue;
            }
            let inode = tree
                .inode_entry(index)
                .ok_or(ParseError::InvalidMetadataResource)?;
            for stream in &inode.streams {
                if stream.hash != [0; 20] {
                    continue;
                }
                if stream.kind == StreamType::Data && !stream.name.is_empty() {
                    let name = stream_name(&names[index], stream.name)?;
                    let parent = files[parents[index].unwrap_or(0)]
                        .as_ref()
                        .ok_or(ParseError::Mkdir)?;
                    open(parent, &name, 2, 0, 5, 4).map_err(|_| ParseError::Open)?;
                } else if stream.kind == StreamType::ReparsePoint {
                    reparses
                        .try_insert_reserved(index, Vec::new())
                        .map_err(|_| ParseError::Nomem)?;
                }
            }
        }
        progress.phase(3, count, count)?;
        Ok(Self {
            tree,
            flags,
            root_selected,
            names,
            order,
            parents,
            groups,
            files,
            progress,
            reparses,
            encrypted: HashMap::new(),
            _target: target_text,
            _filename: filename,
            _image_name: image_name,
        })
    }
    pub(crate) fn part_begin(
        &mut self,
        part: u32,
        total: u32,
        guid: [u8; 16],
    ) -> Result<(), ParseError> {
        // SAFETY: This backend retains the initialized extraction union.
        let info = unsafe { &mut self.progress.info.extract };
        info.part_number = part;
        info.total_parts = total;
        info.guid = guid;
        self.progress.call(5)
    }
    pub(crate) fn all_streams_complete(&self) -> bool {
        self.groups.is_empty()
    }
    pub(crate) fn needs_stream(&self, hash: &[u8; 20]) -> bool {
        self.groups.contains_key(hash)
    }
    pub(crate) fn begin_stream<'sink>(
        &'sink mut self,
        hash: [u8; 20],
        size: u64,
        mismatch: ParseError,
    ) -> Result<StreamSink<'sink, 'handle, 'tree, 'data>, ParseError> {
        let sinks = self.groups.get(&hash).ok_or(ParseError::ResourceNotFound)?;
        let mut active = Vec::new();
        let mut reparses = Vec::new();
        let mut encrypted = Vec::new();
        for sink in sinks {
            let index = sink.node;
            let parent = self.parents[index].unwrap_or(0);
            let stream = &self
                .tree
                .inode_entry(index)
                .ok_or(ParseError::InvalidMetadataResource)?
                .streams[sink.stream];
            if stream.kind == StreamType::EncryptedRaw {
                encrypted
                    .try_push((index, crate::engine::windows_ntfs::spool()?))
                    .map_err(|_| ParseError::Nomem)?;
                continue;
            }
            if stream.kind == StreamType::ReparsePoint {
                if size > 16376 {
                    return Err(ParseError::InvalidReparseData);
                }
                reparses.try_push(index).map_err(|_| ParseError::Nomem)?;
                continue;
            }
            let name = stream_name(&self.names[index], stream.name)?;
            let file = open(
                self.files[parent].as_ref().ok_or(ParseError::Mkdir)?,
                &name,
                2,
                0,
                if stream.name.is_empty() { 1 } else { 5 },
                4,
            )
            .map_err(|_| ParseError::Open)?;
            let sparse = self
                .tree
                .inode_entry(index)
                .ok_or(ParseError::InvalidMetadataResource)?
                .attributes
                & 0x200
                != 0;
            if sparse {
                storage_attributes(&file, 0x200)?;
            } else {
                // Original FileAllocationInformation is best effort.
                set_info(&file, &(size as i64).to_ne_bytes(), 19);
            }
            active
                .try_push((file, sparse))
                .map_err(|_| ParseError::Nomem)?;
        }
        Ok(StreamSink {
            backend: self,
            active,
            reparses,
            reparse_data: Vec::new(),
            encrypted,
            hash,
            size,
            offset: 0,
            digest: Sha1::new(),
            mismatch,
        })
    }
    pub(crate) fn finish(mut self) -> Result<(), ParseError> {
        if !self.all_streams_complete() {
            return Err(ParseError::ResourceOrder);
        }
        // Raw EFS import needs all directory handles closed to avoid sharing violations.
        if !self.encrypted.is_empty() {
            self.files.iter_mut().for_each(|file| *file = None);
            for &index in &self.order {
                let Some(mut raw) = self.encrypted.remove(&index) else {
                    continue;
                };
                let mut components = Vec::new();
                let mut current = index;
                while current != 0 {
                    components.push(current);
                    current = self.parents[current].ok_or(ParseError::InvalidMetadataResource)?;
                }
                let mut path = std::path::PathBuf::from(std::ffi::OsString::from_wide(
                    &self._target[..self._target.len() - 1],
                ));
                for &part in components.iter().rev() {
                    path.push(std::ffi::OsString::from_wide(&self.names[part]));
                }
                crate::engine::windows_ntfs::import_encrypted(
                    &path,
                    &mut raw,
                    self.tree.nodes[index].entry.is_directory(),
                )?;
            }
            self.files[0] = Some(open_target(&self._target)?);
            for &index in self.order.iter().skip(1) {
                if self.tree.nodes[index].entry.is_directory() {
                    let parent = self.parents[index].ok_or(ParseError::InvalidMetadataResource)?;
                    self.files[index] = Some(
                        open(
                            self.files[parent].as_ref().ok_or(ParseError::Mkdir)?,
                            &self.names[index],
                            0x0012_019f,
                            0,
                            1,
                            1,
                        )
                        .map_err(|_| ParseError::Open)?,
                    );
                }
            }
        }
        let tree = self.tree;
        let flags = self.flags;
        let count = self.order.len() as u64;
        let order = &self.order;
        let files = &self.files;
        let names = &self.names;
        let progress = &mut self.progress;
        for (&index, data) in &self.reparses {
            let inode = tree
                .inode_entry(index)
                .ok_or(ParseError::InvalidMetadataResource)?;
            let mut raw = Vec::new();
            raw.try_extend_from_slice(&(inode.inode_union as u32).to_le_bytes())
                .map_err(|_| ParseError::Nomem)?;
            raw.try_extend_from_slice(&(data.len() as u16).to_le_bytes())
                .map_err(|_| ParseError::Nomem)?;
            raw.try_extend_from_slice(&((inode.inode_union >> 32) as u16).to_le_bytes())
                .map_err(|_| ParseError::Nomem)?;
            raw.try_extend_from_slice(data)
                .map_err(|_| ParseError::Nomem)?;
            let fixed = if flags & 0x100 != 0 && inode.inode_union & (1 << 48) == 0 {
                fix_reparse(&raw, &self._target)?
            } else {
                None
            };
            let parent = files[self.parents[index].unwrap_or(0)]
                .as_ref()
                .ok_or(ParseError::Mkdir)?;
            let has_data = inode.streams.iter().any(|stream| {
                stream.kind == StreamType::Data && stream.name.is_empty() && stream.hash != [0; 20]
            });
            let mut file = open(
                parent,
                &names[index],
                if has_data { 0xc000_0000 } else { 0x4000_0000 },
                0,
                1,
                0,
            )
            .map_err(|_| ParseError::Open)?;
            // NTFS requires an empty unnamed stream when attaching a symlink
            // reparse point, but permits data written afterwards. Resource
            // ordering can put that data first, including in pipable archives.
            // Spool it on disk rather than buffering an unbounded stream.
            let mut saved = if has_data {
                let mut saved = crate::engine::windows_ntfs::spool()?;
                file.seek(SeekFrom::Start(0))
                    .map_err(|_| ParseError::Read)?;
                std::io::copy(&mut file, &mut saved).map_err(|_| ParseError::Write)?;
                file.set_len(0).map_err(|_| ParseError::Write)?;
                Some(saved)
            } else {
                None
            };
            if fsctl(&file, 0x900a4, fixed.as_deref().unwrap_or(&raw)) < 0 {
                return Err(ParseError::SetReparseData);
            }
            if let Some(saved) = &mut saved {
                saved
                    .seek(SeekFrom::Start(0))
                    .map_err(|_| ParseError::Read)?;
                file.seek(SeekFrom::Start(0))
                    .map_err(|_| ParseError::Write)?;
                std::io::copy(saved, &mut file).map_err(|_| ParseError::Write)?;
            }
        }
        progress.phase(6, 0, count)?;
        for (completed, &index) in order.iter().rev().enumerate() {
            if index == 0 && !self.root_selected {
                continue;
            }
            let parent = if index == 0 {
                0
            } else {
                self.parents[index].ok_or(ParseError::InvalidMetadataResource)?
            };
            metadata(
                files[parent].as_ref().ok_or(ParseError::Mkdir)?,
                &names[index],
                tree.inode_entry(index)
                    .ok_or(ParseError::InvalidMetadataResource)?,
                tree.security_descriptor(index),
                flags,
            )?;
            if (completed + 1).is_multiple_of(500) {
                progress.phase(6, (completed + 1) as u64, count)?;
            }
        }
        progress.phase(6, count, count)?;
        progress.call(7)
    }
}
pub(crate) struct StreamSink<'sink, 'handle, 'tree, 'data> {
    backend: &'sink mut PreparedExtraction<'handle, 'tree, 'data>,
    active: Vec<(File, bool)>,
    reparses: Vec<usize>,
    reparse_data: Vec<u8>,
    encrypted: Vec<(usize, File)>,
    hash: [u8; 20],
    size: u64,
    offset: u64,
    digest: Sha1,
    mismatch: ParseError,
}
impl StreamSink<'_, '_, '_, '_> {
    pub(crate) fn consume(&mut self, bytes: &[u8]) -> Result<(), ParseError> {
        let end = self
            .offset
            .checked_add(bytes.len() as u64)
            .filter(|&end| end <= self.size)
            .ok_or(ParseError::InvalidMetadataResource)?;
        self.digest.update(bytes);
        self.offset = end;
        // Source extract.c reports decoded progress before calling backend write.
        self.backend.progress.data(
            bytes.len() as u64
                * (self.active.len() + self.reparses.len() + self.encrypted.len()) as u64,
            if end == self.size {
                (self.active.len() + self.reparses.len() + self.encrypted.len()) as u64
            } else {
                0
            },
        )?;
        if !self.reparses.is_empty() {
            self.reparse_data
                .try_extend_from_slice(bytes)
                .map_err(|_| ParseError::Nomem)?;
        }
        for (_, file) in &mut self.encrypted {
            file.write_all(bytes).map_err(|_| ParseError::Write)?;
        }
        for (file, sparse) in &mut self.active {
            if *sparse {
                for chunk in bytes.chunks(4096) {
                    if chunk.iter().all(|&b| b == 0) {
                        file.seek(SeekFrom::Current(chunk.len() as i64))
                            .map_err(|_| ParseError::Write)?;
                    } else {
                        file.write_all(chunk).map_err(|_| ParseError::Write)?;
                    }
                }
            } else {
                file.write_all(bytes).map_err(|_| ParseError::Write)?;
            }
        }
        Ok(())
    }
    pub(crate) fn finish(mut self) -> Result<(), ParseError> {
        if self.offset != self.size {
            return Err(ParseError::UnexpectedEndOfFile);
        }
        let actual: [u8; 20] = self.digest.finalize().into();
        if actual != self.hash && self.backend.flags & 2 == 0 {
            return Err(self.mismatch);
        }
        for (file, sparse) in &self.active {
            if *sparse {
                file.set_len(self.size).map_err(|_| ParseError::Write)?;
            }
        }
        while let Some((index, file)) = self.encrypted.pop() {
            self.backend
                .encrypted
                .try_insert_reserved(index, file)
                .map_err(|_| ParseError::Nomem)?;
        }
        for &index in &self.reparses {
            let mut data = Vec::new();
            data.try_extend_from_slice(&self.reparse_data)
                .map_err(|_| ParseError::Nomem)?;
            self.backend
                .reparses
                .try_insert_reserved(index, data)
                .map_err(|_| ParseError::Nomem)?;
        }
        self.backend.groups.remove(&self.hash);
        Ok(())
    }
}
pub(super) fn extract(
    handle: &WimHandle,
    image: i32,
    target: &Path,
    flags: u32,
    tree: &Metadata<'_>,
    selection: Option<&Selection>,
) -> Result<(), ParseError> {
    let selected = layout(tree, selection)?;
    let mut blobs = Vec::new();
    let mut total_bytes = 0u64;
    let mut total_streams = 0u64;
    for (hash, sinks) in &selected.groups {
        let (source, size, chunk, order) = blob_source(handle, hash)?;
        total_bytes = total_bytes
            .checked_add(
                size.checked_mul(sinks.len() as u64)
                    .ok_or(ParseError::InvalidMetadataResource)?,
            )
            .ok_or(ParseError::InvalidMetadataResource)?;
        total_streams += sinks.len() as u64;
        blobs
            .try_push((order, *hash, source, size, chunk))
            .map_err(|_| ParseError::Nomem)?;
    }
    blobs.sort_by_key(|b| b.0);
    let mut backend = PreparedExtraction::prepare(
        handle,
        image,
        target,
        flags,
        tree,
        selected,
        (total_bytes, total_streams),
    )?;
    for (_, hash, source, size, chunk) in &blobs {
        let mismatch = if matches!(source, BlobSource::Captured {stream,..} if matches!(stream.source, crate::engine::capture::CapturedSource::File(_)))
        {
            ParseError::ConcurrentModificationDetected
        } else {
            ParseError::InvalidResourceHash
        };
        let mut sink = backend.begin_stream(*hash, *size, mismatch)?;
        let mut offset = 0;
        while offset < *size {
            let end = chunk_end(source, offset, *size, *chunk);
            let bytes = read_chunk(source, offset, end, flags)?;
            sink.consume(&bytes)?;
            offset = end;
        }
        sink.finish()?;
    }
    backend.finish()
}
