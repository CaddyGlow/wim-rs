// SPDX-License-Identifier: LGPL-2.1-or-later
//! Native public directory-tree callbacks, translated from iterate_dir.c.
use crate::engine::{
    TChar,
    handles::{WimHandle, image_metadata_bytes},
    lookup::{WimResourceEntry, resource_entry},
};
#[cfg(not(windows))]
use std::ffi::c_long;
use std::{
    collections::HashMap,
    ffi::{c_int, c_void},
    sync::Arc,
};
use wim_format::{
    ParseError,
    metadata::{Metadata, Stream, StreamType},
};

/// Public timestamp layout; Unix uses the native C `timespec` fields.
#[repr(C)]
#[derive(Debug, Default)]
pub struct WimTimespec {
    /// Seconds since the Unix epoch.
    #[cfg(not(windows))]
    pub tv_sec: c_long,
    /// Seconds since the Unix epoch on 64-bit Windows.
    #[cfg(all(windows, target_pointer_width = "64"))]
    pub tv_sec: i64,
    /// Seconds since the Unix epoch on 32-bit Windows.
    #[cfg(all(windows, target_pointer_width = "32"))]
    pub tv_sec: i32,
    /// Nanoseconds on Unix.
    #[cfg(not(windows))]
    pub tv_nsec: c_long,
    /// Nanoseconds on Windows.
    #[cfg(windows)]
    pub tv_nsec: i32,
}
/// Four identifiers in the original public NTFS object-ID structure.
#[repr(C)]
#[derive(Debug, Default)]
pub struct WimObjectId {
    /// Current object identifier.
    pub object_id: [u8; 16],
    /// Original volume identifier.
    pub birth_volume_id: [u8; 16],
    /// Original object identifier.
    pub birth_object_id: [u8; 16],
    /// Domain identifier.
    pub domain_id: [u8; 16],
}
/// Public callback stream information.
#[repr(C)]
#[derive(Debug, Default)]
pub struct WimStreamEntry {
    /// Named data stream name; NULL for the default stream.
    pub stream_name: *const TChar,
    /// Original resource information or a missing-resource digest.
    pub resource: WimResourceEntry,
    /// Reserved zero fields.
    pub reserved: [u64; 4],
}
/// Public directory-entry prefix, followed immediately by its stream array.
#[repr(C)]
#[derive(Debug, Default)]
pub struct WimDirEntry {
    /// Filename, NULL for the root directory.
    pub filename: *const TChar,
    /// DOS filename, NULL when absent.
    pub dos_name: *const TChar,
    /// Absolute image path with the platform WIM path separator.
    pub full_path: *const TChar,
    /// Depth measured from the image root.
    pub depth: usize,
    /// Borrowed self-relative Windows security descriptor.
    pub security_descriptor: *const std::ffi::c_char,
    /// Byte count of the descriptor.
    pub security_descriptor_size: usize,
    /// Windows file attributes.
    pub attributes: u32,
    /// Reparse-point tag, zero for other files.
    pub reparse_tag: u32,
    /// Number of names sharing this inode.
    pub num_links: u32,
    /// Named data stream count, excluding the default entry.
    pub num_named_streams: u32,
    /// Shared inode identifier.
    pub hard_link_group_id: u64,
    /// Creation timestamp.
    pub creation_time: WimTimespec,
    /// Last-write timestamp.
    pub last_write_time: WimTimespec,
    /// Last-access timestamp.
    pub last_access_time: WimTimespec,
    /// Unix user ID, valid when mode is nonzero.
    pub unix_uid: u32,
    /// Unix group ID, valid when mode is nonzero.
    pub unix_gid: u32,
    /// Unix mode, zero when the extension is absent.
    pub unix_mode: u32,
    /// Unix device ID.
    pub unix_rdev: u32,
    /// NTFS object identifiers.
    pub object_id: WimObjectId,
    /// High timestamp seconds when the seconds field is 32-bit.
    pub creation_time_high: i32,
    /// High last-write seconds when the seconds field is 32-bit.
    pub last_write_time_high: i32,
    /// High last-access seconds when the seconds field is 32-bit.
    pub last_access_time_high: i32,
    /// Reserved zero field.
    pub reserved2: i32,
    /// Reserved zero fields.
    pub reserved: [u64; 4],
    /// Flexible array: one default stream followed by named data streams.
    pub streams: [WimStreamEntry; 0],
}
/// Callback with entry storage and all nested pointers valid until it returns.
pub type DirTreeCallback = unsafe extern "C" fn(*const WimDirEntry, *mut c_void) -> c_int;

fn timestamp(ticks: u64) -> (WimTimespec, i32) {
    let seconds = (ticks / 10_000_000) as i64 - 11_644_473_600;
    let time = WimTimespec {
        tv_sec: seconds as _,
        tv_nsec: ((ticks % 10_000_000) * 100) as _,
    };
    let high = if std::mem::size_of_val(&time.tv_sec) == 4 {
        (seconds >> 32) as i32
    } else {
        0
    };
    (time, high)
}
fn name(bytes: &[u8]) -> Result<Option<Vec<TChar>>, ParseError> {
    if bytes.is_empty() {
        #[cfg(not(windows))]
        return Ok(Some(vec![0]));
        #[cfg(windows)]
        return Ok(None);
    }
    let units: Vec<u16> = bytes
        .chunks_exact(2)
        .map(|b| u16::from_le_bytes([b[0], b[1]]))
        .collect();
    #[cfg(not(windows))]
    let mut output = wim_format::platform_text::utf16_to_wtf8(&units)?
        .into_iter()
        .map(|b| b as TChar)
        .collect::<Vec<_>>();
    #[cfg(windows)]
    let mut output = units;
    output.push(0);
    Ok(Some(output))
}
fn text_pointer(text: &Option<Vec<TChar>>) -> *const TChar {
    text.as_ref().map_or(std::ptr::null(), |s| s.as_ptr())
}
unsafe fn path_units(path: *const TChar) -> Option<Vec<u16>> {
    if path.is_null() {
        return Some(Vec::new());
    }
    #[cfg(not(windows))]
    {
        // SAFETY: The caller supplies a readable NUL-terminated path.
        wim_format::platform_text::wtf8_to_utf16(
            unsafe { std::ffi::CStr::from_ptr(path) }.to_bytes(),
        )
        .ok()
    }
    #[cfg(windows)]
    {
        let mut length = 0;
        // SAFETY: The caller supplies a readable NUL-terminated path.
        while unsafe { *path.add(length) } != 0 {
            length += 1;
        }
        // SAFETY: The terminating scan bounds the readable slice.
        Some(unsafe { std::slice::from_raw_parts(path, length) }.to_vec())
    }
}

fn find_node(metadata: &Metadata<'_>, path: &[u16]) -> Result<usize, c_int> {
    if metadata.nodes.is_empty() {
        return Err(libc::ENOENT);
    }
    let mut current = 0;
    for component in path
        .split(|&c| c == b'/' as u16 || c == b'\\' as u16)
        .filter(|c| !c.is_empty())
    {
        if !metadata
            .inode_entry(current)
            .ok_or(libc::ENOENT)?
            .is_directory()
        {
            return Err(libc::ENOTDIR);
        }
        let children = &metadata.nodes[current].children;
        let matches = |child: usize, folded: bool| {
            let fold = |unit| {
                if folded {
                    wim_format::ntfs_upcase::uppercase(unit)
                } else {
                    unit
                }
            };
            metadata.nodes[child]
                .entry
                .name
                .chunks_exact(2)
                .map(|b| fold(u16::from_le_bytes([b[0], b[1]])))
                .eq(component.iter().copied().map(fold))
        };
        current = *children
            .iter()
            .find(|&&child| matches(child, false))
            .or_else(|| {
                crate::engine::runtime::ignore_case()
                    .then(|| children.iter().find(|&&child| matches(child, true)))
                    .flatten()
            })
            .ok_or(libc::ENOENT)?;
    }
    Ok(current)
}
fn full_path(metadata: &Metadata<'_>, node: usize) -> Result<(Vec<TChar>, usize), ParseError> {
    let mut ancestors = Vec::new();
    let mut current = node;
    while let Some(parent) = metadata.nodes[current].parent {
        ancestors.push(current);
        current = parent;
    }
    let depth = ancestors.len();
    #[cfg(not(windows))]
    let separator = b'/' as TChar;
    #[cfg(windows)]
    let separator = b'\\' as TChar;
    let mut path = vec![separator];
    for (i, &ancestor) in ancestors.iter().rev().enumerate() {
        if i != 0 {
            path.push(separator);
        }
        if let Some(text) = name(metadata.nodes[ancestor].entry.name)? {
            path.extend_from_slice(&text[..text.len() - 1]);
        }
    }
    path.push(0);
    Ok((path, depth))
}
fn stream_resource(
    handle: &WimHandle,
    stream: Option<&Stream<'_>>,
    flags: c_int,
) -> Result<WimResourceEntry, ParseError> {
    let Some(stream) = stream else {
        return Ok(WimResourceEntry::default());
    };
    if stream.hash == [0; 20] {
        return Ok(WimResourceEntry::default());
    }
    if !handle.removed_blobs.contains(&stream.hash) {
        if let Some(owned) = handle.owned_blobs.get(&stream.hash) {
            let d = &owned.descriptor;
            return Ok(resource_entry(&d.blob, &d.resource, d.part));
        }
        if let Some(table) = &handle.lookup
            && let Some(blob) = table.find(&stream.hash)
        {
            return Ok(resource_entry(
                blob,
                &table.resources[blob.resource_index],
                handle.header.part_number,
            ));
        }
    }
    if let Some(resource) = crate::engine::lookup::captured_resources(handle)?
        .iter()
        .find(|resource| resource.hash == stream.hash)
    {
        return Ok(resource.entry());
    }
    if flags & 4 != 0 {
        return Err(ParseError::ResourceNotFound);
    }
    Ok(WimResourceEntry {
        sha1_hash: stream.hash,
        flags: 1 << 4,
        ..WimResourceEntry::default()
    })
}
struct CaptureView<'a> {
    plan: &'a crate::engine::capture::CapturePlan,
    order: Vec<usize>,
    bindings: HashMap<(usize, usize), usize>,
    references: HashMap<usize, u32>,
}
impl<'a> CaptureView<'a> {
    fn new(plan: &'a crate::engine::capture::CapturePlan) -> Result<Self, ParseError> {
        let mut bindings = HashMap::new();
        let mut references = HashMap::new();
        bindings
            .try_reserve(plan.bindings.len())
            .map_err(|_| ParseError::Nomem)?;
        references
            .try_reserve(plan.bindings.len())
            .map_err(|_| ParseError::Nomem)?;
        for (index, binding) in plan.bindings.iter().enumerate() {
            bindings
                .entry((binding.node, binding.slot))
                .or_insert(index);
            let count = references
                .entry(Arc::as_ptr(&binding.stream) as usize)
                .or_insert(0u32);
            *count = count
                .checked_add(1)
                .ok_or(ParseError::InvalidMetadataResource)?;
        }
        Ok(Self {
            plan,
            order: plan.metadata_order()?,
            bindings,
            references,
        })
    }
    fn resource(&self, node: usize, slot: usize) -> Option<WimResourceEntry> {
        let &graph_node = self.order.get(node)?;
        let binding = &self.plan.bindings[*self.bindings.get(&(graph_node, slot))?];
        Some(WimResourceEntry {
            uncompressed_size: binding.stream.size,
            reference_count: *self
                .references
                .get(&(Arc::as_ptr(&binding.stream) as usize))?,
            ..WimResourceEntry::default()
        })
    }
    fn named_resource(&self, node: usize, name: &[u8]) -> Option<WimResourceEntry> {
        let &graph_node = self.order.get(node)?;
        let slot = self.plan.tree.nodes[graph_node]
            .extra_streams
            .iter()
            .position(|s| s.name == name)?
            + 1;
        self.resource(node, slot)
    }
}
struct ImageView<'a> {
    links: &'a [u32],
    capture: Option<&'a CaptureView<'a>>,
}
fn callback_entry(
    handle: &WimHandle,
    metadata: &Metadata<'_>,
    view: &ImageView<'_>,
    node: usize,
    flags: c_int,
    callback: Option<DirTreeCallback>,
    context: *mut c_void,
) -> Result<c_int, ParseError> {
    let ImageView { links, capture } = *view;
    let entry = &metadata.nodes[node].entry;
    let inode = metadata
        .inode_entry(node)
        .ok_or(ParseError::InvalidMetadataResource)?;
    let filename = if node == 0 {
        name(&[])?
    } else {
        name(entry.name)?
    };
    let dos_name = name(entry.short_name)?;
    let (full_path, depth) = full_path(metadata, node)?;
    let security = metadata.security_descriptor(node);
    let wanted = if inode.attributes & 0x4000 != 0 {
        StreamType::EncryptedRaw
    } else if inode.attributes & 0x400 != 0 {
        StreamType::ReparsePoint
    } else {
        StreamType::Data
    };
    let default = inode
        .streams
        .iter()
        .find(|s| s.name.is_empty() && s.kind == wanted);
    let named: Vec<_> = inode
        .streams
        .iter()
        .filter(|s| !s.name.is_empty() && s.kind == StreamType::Data)
        .collect();
    let names: Vec<_> = named
        .iter()
        .map(|s| name(s.name))
        .collect::<Result<_, _>>()?;
    let stream_count = named.len() + 1;
    let bytes = std::mem::size_of::<WimDirEntry>()
        .checked_add(
            stream_count
                .checked_mul(std::mem::size_of::<WimStreamEntry>())
                .ok_or(ParseError::Nomem)?,
        )
        .ok_or(ParseError::Nomem)?;
    let words = bytes.checked_add(7).ok_or(ParseError::Nomem)? / 8;
    let mut storage = Vec::<u64>::new();
    storage
        .try_reserve_exact(words)
        .map_err(|_| ParseError::Nomem)?;
    storage.resize(words, 0);
    let pointer = storage.as_mut_ptr().cast::<WimDirEntry>();
    let (creation_time, creation_time_high) = timestamp(inode.creation_time);
    let (last_write_time, last_write_time_high) = timestamp(inode.last_write_time);
    let (last_access_time, last_access_time_high) = timestamp(inode.last_access_time);
    let unix = inode.tagged_item(0x337d_d873, 16);
    let unix_field = |offset| {
        unix.map_or(0, |b| {
            u32::from_le_bytes(b[offset..offset + 4].try_into().unwrap_or([0; 4]))
        })
    };
    let mut object_id = [0u8; 64];
    if let Some(object) = inode.tagged_item(1, 16) {
        let size = object.len().min(64);
        object_id[..size].copy_from_slice(&object[..size]);
    }
    let canonical = metadata.nodes[node].inode;
    let num_links = links[canonical];
    let public = WimDirEntry {
        filename: text_pointer(&filename),
        dos_name: text_pointer(&dos_name),
        full_path: full_path.as_ptr(),
        depth,
        security_descriptor: security.map_or(std::ptr::null(), |s| s.as_ptr().cast()),
        security_descriptor_size: security.map_or(0, <[u8]>::len),
        attributes: inode.attributes,
        reparse_tag: inode.reparse_fields().map_or(0, |r| r.0),
        num_links,
        num_named_streams: named.len() as u32,
        hard_link_group_id: inode.hard_link_group_id().unwrap_or(0),
        creation_time,
        last_write_time,
        last_access_time,
        unix_uid: unix_field(0),
        unix_gid: unix_field(4),
        unix_mode: unix_field(8),
        unix_rdev: unix_field(12),
        object_id: WimObjectId {
            object_id: object_id[..16].try_into().unwrap_or([0; 16]),
            birth_volume_id: object_id[16..32].try_into().unwrap_or([0; 16]),
            birth_object_id: object_id[32..48].try_into().unwrap_or([0; 16]),
            domain_id: object_id[48..].try_into().unwrap_or([0; 16]),
        },
        creation_time_high,
        last_write_time_high,
        last_access_time_high,
        ..WimDirEntry::default()
    };
    // SAFETY: Vec<u64> storage is aligned and sized for this prefix and stream array.
    unsafe {
        pointer.write(public);
    }
    // SAFETY: The flexible array begins at the aligned end of the repr(C) prefix.
    let streams = unsafe {
        pointer
            .byte_add(std::mem::size_of::<WimDirEntry>())
            .cast::<WimStreamEntry>()
    };
    let first = if default.is_none_or(|stream| stream.hash == [0; 20]) {
        match capture.and_then(|view| view.resource(node, 0)) {
            Some(resource) => resource,
            None => stream_resource(handle, default, flags)?,
        }
    } else {
        stream_resource(handle, default, flags)?
    };
    // SAFETY: Stream storage was allocated above for exactly stream_count entries.
    unsafe {
        streams.write(WimStreamEntry {
            resource: first,
            ..WimStreamEntry::default()
        });
    }
    for (i, stream) in named.iter().enumerate() {
        let pending = (stream.hash == [0; 20])
            .then(|| capture.and_then(|view| view.named_resource(node, stream.name)))
            .flatten();
        let resource = match pending {
            Some(resource) => resource,
            None => stream_resource(handle, Some(stream), flags)?,
        };
        // SAFETY: i+1 is within the allocated flexible array extent.
        unsafe {
            streams.add(i + 1).write(WimStreamEntry {
                stream_name: text_pointer(&names[i]),
                resource,
                ..WimStreamEntry::default()
            });
        }
    }
    if flags & 2 != 0 {
        return Ok(0);
    }
    let callback = callback.ok_or(ParseError::InvalidParam)?;
    // SAFETY: Storage and all nested pointer owners live through this callback.
    Ok(unsafe { callback(pointer, context) })
}
/// Iterate a selected image subtree, preserving original callback stop results.
/// The entry, its flexible stream array, names and security bytes are borrowed
/// only until the callback returns. Current POSIX conversion assumes UTF-8.
///
/// # Safety
/// The handle must be live and exclusively accessed; path must be NULL or a
/// readable terminated platform string. The callback must not retain pointers
/// or mutate/free the handle, and context must satisfy the callback requirements.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn wimlib_iterate_dir_tree(
    handle: *mut WimHandle,
    image: c_int,
    path: *const TChar,
    flags: c_int,
    callback: Option<DirTreeCallback>,
    context: *mut c_void,
) -> c_int {
    if flags & !7 != 0 {
        return 24;
    }
    // SAFETY: Caller provides a live handle.
    let Some(handle) = (unsafe { handle.as_ref() }) else {
        return 24;
    };
    let images = if image == -1 {
        0..handle.header.image_count as usize
    } else if image >= 1 && image as u32 <= handle.header.image_count {
        image as usize - 1..image as usize
    } else {
        return 18;
    };
    // SAFETY: Caller provides the terminated path, or NULL.
    let path = unsafe { path_units(path) };
    for image in images {
        if handle.images.get(image).is_none() {
            return ParseError::MetadataNotFound as c_int;
        }
        let bytes = match image_metadata_bytes(handle, image) {
            Ok(bytes) => bytes,
            Err(e) => return e as c_int,
        };
        let metadata = match Metadata::parse(&bytes) {
            Ok(m) => m,
            Err(e) => return e as c_int,
        };
        let selected = match path
            .as_ref()
            .ok_or(libc::EILSEQ)
            .and_then(|path| find_node(&metadata, path))
        {
            Ok(selected) => selected,
            Err(error) => {
                #[cfg(target_os = "linux")]
                // SAFETY: libc returns writable errno storage for the calling thread.
                unsafe {
                    *libc::__errno_location() = error;
                }
                #[cfg(not(target_os = "linux"))]
                let _ = error;
                return 49;
            }
        };
        let capture = match crate::engine::handles::image_capture_plan(&handle.images[image]) {
            Ok(capture) => capture,
            Err(error) => return error as c_int,
        };
        let captured = match capture.as_ref().map(CaptureView::new).transpose() {
            Ok(view) => view,
            Err(error) => return error as c_int,
        };
        let mut links = vec![0u32; metadata.nodes.len()];
        for node in &metadata.nodes {
            links[node.inode] = links[node.inode].wrapping_add(1);
        }
        let view = ImageView {
            links: &links,
            capture: captured.as_ref(),
        };
        let mut pending = vec![(selected, flags)];
        while let Some((node, flags)) = pending.pop() {
            match callback_entry(handle, &metadata, &view, node, flags, callback, context) {
                Ok(0) => {}
                Ok(stop) => return stop,
                Err(e) => return e as c_int,
            }
            if flags & 3 != 0 {
                pending.extend(
                    metadata.nodes[node]
                        .children
                        .iter()
                        .rev()
                        .map(|&child| (child, flags & !2)),
                );
            }
        }
    }
    0
}

#[cfg(test)]
mod capture_view_tests {
    use super::*;
    use crate::engine::capture::{
        CaptureBinding, CaptureIdentity, CapturePlan, CapturedSource, CapturedStream,
    };
    use wim_format::metadata_write::{OwnedDentry, OwnedStream};

    #[test]
    fn pending_stream_resources_preserve_alias_reference_counts_and_named_sizes() {
        let mut plan = CapturePlan::default();
        let mut root = OwnedDentry::new(Vec::new(), 16);
        root.children = vec![1, 2];
        let mut file = OwnedDentry::new(vec![b'a', 0], 32);
        file.extra_streams.push(OwnedStream {
            name: vec![b'x', 0],
            ..OwnedStream::default()
        });
        let mut alias = file.clone();
        alias.name = vec![b'b', 0];
        plan.tree.nodes = vec![root, file, alias];
        let main = Arc::new(CapturedStream {
            size: 19,
            identity: CaptureIdentity {
                session: 1,
                device: 2,
                inode: 3,
            },
            source: CapturedSource::Inline(vec![0; 19]),
        });
        let named = Arc::new(CapturedStream {
            size: 7,
            identity: main.identity,
            source: CapturedSource::Inline(vec![0; 7]),
        });
        for node in [1, 2] {
            plan.bindings.push(CaptureBinding {
                node,
                slot: 0,
                stream: main.clone(),
            });
            plan.bindings.push(CaptureBinding {
                node,
                slot: 1,
                stream: named.clone(),
            });
        }
        let view = CaptureView::new(&plan).unwrap();
        assert!(view.resource(0, 0).is_none());
        for node in [1, 2] {
            let resource = view.resource(node, 0).unwrap();
            assert_eq!(
                (resource.uncompressed_size, resource.reference_count),
                (19, 2)
            );
            let resource = view.named_resource(node, &[b'x', 0]).unwrap();
            assert_eq!(
                (resource.uncompressed_size, resource.reference_count),
                (7, 2)
            );
            assert!(view.named_resource(node, &[b'z', 0]).is_none());
        }
    }
}
