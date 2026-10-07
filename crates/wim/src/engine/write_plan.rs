// SPDX-License-Identifier: LGPL-2.1-or-later
//! Lazy resource selection and output owned separately from the live handle.
use super::{Settings, generate_guid, table_entry};
use crate::engine::{
    handles::WimHandle,
    progress::{
        DoneWithFileProgress, IntegrityProgress, ProgressInfo, ProgressRegistration,
        WriteStreamsProgress, filename_buffer, next_progress,
    },
};
use sha1::{Digest, Sha1};
use std::{
    cell::{Cell, RefCell},
    collections::{BTreeMap, BTreeSet},
    fs::File,
    io::{Seek, SeekFrom, Write},
    sync::Arc,
};
use wim_format::{
    Compression, HEADER_SIZE, Header, PIPABLE_MAGIC, ParseError, ResourceHeader, WIM_MAGIC,
    archive::Archive,
    integrity::{DEFAULT_CHUNK_SIZE, IntegrityTable},
    lookup::{LookupBlob, LookupResource},
    metadata::Metadata,
    resource::ResourceLayout,
    resource_write::{encode_resource, stream_resource_from_chunks},
    xml::XmlInfo,
};
// Reparses with multiple streams store RP first in extras, with a zero main
// hash. Files also store DATA; directories only store their named streams.
fn normalize_capture_reparse_streams(
    capture: &mut crate::engine::capture::CapturePlan,
) -> Result<(), c_int> {
    for (index, node) in capture.tree.nodes.iter_mut().enumerate() {
        if node.attributes & 0x400 == 0 {
            continue;
        }
        if node.attributes & 0x10 != 0 {
            let has_named = node
                .extra_streams
                .iter()
                .any(|stream| !stream.name.is_empty());
            if !has_named {
                // A lone directory RP belongs in main_hash, without DATA.
                if !node.extra_streams.is_empty() {
                    return Err(68);
                }
                continue;
            }
            let unnamed = node
                .extra_streams
                .iter()
                .filter(|stream| stream.name.is_empty())
                .count();
            let deferred_main = capture
                .bindings
                .iter()
                .any(|binding| binding.node == index && binding.slot == 0);
            if node.main_hash == [0; 20]
                && unnamed == 1
                && !deferred_main
                && node.extra_streams[0].name.is_empty()
            {
                // Already serialized: the only unnamed extra is the RP.
                continue;
            }
            if unnamed != 0 {
                return Err(68); // Directories cannot have unnamed DATA.
            }
            node.extra_streams.try_reserve(1).map_err(|_| 39)?;
            node.extra_streams.insert(
                0,
                wim_format::metadata_write::OwnedStream {
                    hash: std::mem::take(&mut node.main_hash),
                    ..Default::default()
                },
            );
            for binding in &mut capture.bindings {
                if binding.node == index {
                    binding.slot += 1;
                }
            }
            continue;
        }
        let data = node
            .extra_streams
            .iter()
            .position(|stream| stream.name.is_empty());
        // An already serialized reparse has its RP in the extra streams.
        if node.main_hash == [0; 20]
            && data.is_some()
            && !capture
                .bindings
                .iter()
                .any(|binding| binding.node == index && binding.slot == 0)
        {
            continue;
        }
        node.extra_streams.try_reserve(2).map_err(|_| 39)?;
        let reparse = std::mem::take(&mut node.main_hash);
        let unnamed = data.map_or_else(Default::default, |slot| node.extra_streams.remove(slot));
        node.extra_streams.insert(
            0,
            wim_format::metadata_write::OwnedStream {
                hash: reparse,
                ..Default::default()
            },
        );
        node.extra_streams.insert(1, unnamed);
        for binding in &mut capture.bindings {
            if binding.node != index {
                continue;
            }
            binding.slot = if binding.slot == 0 {
                1
            } else if data.is_some_and(|slot| binding.slot == slot + 1) {
                2
            } else if data.is_some_and(|slot| binding.slot > slot + 1) {
                binding.slot + 1
            } else {
                binding.slot + 2
            };
        }
    }
    Ok(())
}
struct CapturedState {
    stream: Arc<crate::engine::capture::CapturedStream>,
    reader: RefCell<Option<File>>,
    digest: RefCell<Sha1>,
    hash: Cell<[u8; 20]>,
    done_early: Cell<bool>,
    hash_ready: Cell<bool>,
    ready_order: Cell<Option<usize>>,
    extra_references: Cell<u32>,
}
struct Blob<'a> {
    hash: [u8; 20],
    captured: Option<CapturedState>,
    count: u32,
    bytes: Option<&'a [u8]>,
    backing: Option<&'a [u8]>,
    file_backing: Option<&'a crate::engine::backing::Backing>,
    descriptor: Option<(LookupBlob, LookupResource)>,
    origin: ([u8; 16], u16, usize),
}
/// Metadata selection follows the initial target header. Data remains unread.
pub(crate) struct Plan<'a> {
    wim: &'a WimHandle,
    selected: &'a Settings,
    header: Header,
    xml: XmlInfo,
    captures: Vec<Option<crate::engine::capture::CapturePlan>>,
    metadata: Vec<Vec<u8>>,
    metadata_copy: Vec<Option<(ResourceHeader, Vec<u8>)>>,
    blobs: Vec<Blob<'a>>,
    append: bool,
    compact: bool,
    reuse_table: bool,
    retained_table: Vec<u8>,
    ready_clock: Cell<usize>,
    compressor: RefCell<CompressorCache>,
}
pub(crate) fn initial_header(wim: &WimHandle, selected: &Settings) -> Result<Header, c_int> {
    let mut header = wim.header.clone();
    header.magic = if selected.pipable {
        PIPABLE_MAGIC
    } else {
        WIM_MAGIC
    };
    header.flags &= 0x84;
    header.chunk_size = wim.output_chunk_size;
    header.version = if selected.solid || wim.output_compression == Compression::Lzms {
        0xe00
    } else {
        0x10d00
    };
    header.flags |= match wim.output_compression {
        Compression::None => 0,
        Compression::Xpress => 2 | 0x20000,
        Compression::Lzx => 2 | 0x40000,
        Compression::Lzms => 2 | 0x80000,
    };
    if selected.flags & 0x800 == 0 {
        header.guid = generate_guid()?;
    }
    header.part_number = 1;
    header.total_parts = 1;
    header.image_count = selected.images.len() as u32;
    header.boot_index = selected
        .images
        .iter()
        .position(|&i| i as u32 + 1 == wim.header.boot_index)
        .map_or(0, |i| i as u32 + 1);
    header.boot_metadata = ResourceHeader::default();
    header.integrity_table = ResourceHeader::default();
    header.blob_table = ResourceHeader::default();
    header.xml_data = ResourceHeader::default();
    header.reserved = [0; 60];
    Ok(header)
}
/// Handle effects applied after all callback-bearing borrows have ended.
#[derive(Default)]
pub(crate) struct Effects {
    pub(crate) total_bytes: Option<u64>,
}
impl<'a> Plan<'a> {
    pub(crate) fn new(
        wim: &'a WimHandle,
        selected: &'a Settings,
        header: Header,
    ) -> Result<Self, c_int> {
        Self::new_internal(wim, selected, header, false)
    }
    pub(crate) fn new_inplace(
        wim: &'a WimHandle,
        selected: &'a Settings,
        header: Header,
    ) -> Result<Self, c_int> {
        Self::new_internal(wim, selected, header, true)
    }
    fn new_internal(
        wim: &'a WimHandle,
        selected: &'a Settings,
        header: Header,
        skip_source: bool,
    ) -> Result<Self, c_int> {
        let mut captures = Vec::new();
        let mut metadata = Vec::new();
        let mut metadata_copy = Vec::new();
        let mut refs = BTreeMap::new();
        for &index in &selected.images {
            if skip_source
                && matches!(
                    wim.images[index],
                    crate::engine::handles::HandleImage::Source(_)
                )
            {
                captures.push(None);
                metadata.push(Vec::new());
                metadata_copy.push(None);
                continue;
            }
            let mut capture =
                crate::engine::handles::image_capture_plan(&wim.images[index]).map_err(code)?;
            if let Some(capture) = &mut capture {
                normalize_capture_reparse_streams(capture)?;
            }
            let bytes = crate::engine::handles::image_metadata_bytes(wim, index)
                .map_err(code)?
                .into_owned();
            let parsed = Metadata::parse(&bytes).map_err(code)?;
            if let Some(capture) = &capture {
                let bound = capture
                    .bindings
                    .iter()
                    .map(|b| (b.node, b.slot))
                    .collect::<BTreeSet<_>>();
                for (node, entry) in capture.tree.nodes.iter().enumerate() {
                    for (slot, hash) in std::iter::once(entry.main_hash)
                        .chain(entry.extra_streams.iter().map(|s| s.hash))
                        .enumerate()
                    {
                        if !bound.contains(&(node, slot)) && hash != [0; 20] {
                            let n = refs.entry(hash).or_insert(0u32);
                            *n = n.checked_add(1).ok_or(24)?;
                        }
                    }
                }
            } else {
                for node in 0..parsed.nodes.len() {
                    for stream in &parsed.inode_entry(node).ok_or(21)?.streams {
                        if stream.hash != [0; 20] {
                            let n = refs.entry(stream.hash).or_insert(0u32);
                            *n = n.checked_add(1).ok_or(24)?;
                        }
                    }
                }
            }
            captures.push(capture);
            let copy = if let crate::engine::handles::HandleImage::Source(source_index) =
                wim.images[index]
            {
                let archive = Archive::open(
                    wim.backing
                        .as_deref()
                        .map(|b| b.bytes())
                        .transpose()
                        .map_err(code)?
                        .ok_or(21)?,
                )
                .map_err(code)?;
                let blob = archive
                    .lookup
                    .metadata
                    .get((source_index - 1) as usize)
                    .ok_or(21)?;
                let r = &archive.lookup.resources[blob.resource_index];
                if selected.flags & 16 == 0
                    && r.compression_code == wim.output_compression as u32
                    && r.chunk_size == wim.output_chunk_size
                    && (archive.header.magic == PIPABLE_MAGIC) == selected.pipable
                {
                    Some((
                        r.header,
                        range(
                            wim.backing
                                .as_deref()
                                .map(|b| b.bytes())
                                .transpose()
                                .map_err(code)?
                                .ok_or(21)?,
                            r.header.offset_in_wim,
                            r.header.size_in_wim,
                        )
                        .map_err(code)?
                        .to_vec(),
                    ))
                } else {
                    None
                }
            } else {
                None
            };
            metadata_copy.push(copy);
            drop(parsed);
            metadata.push(bytes);
        }
        if selected.flags & 0x400 != 0 {
            if let Some(lookup) = &wim.lookup {
                for blob in &lookup.blobs {
                    if !wim.removed_blobs.contains(&blob.hash) {
                        refs.insert(blob.hash, blob.reference_count);
                    }
                }
            }
            for (hash, blob) in wim.owned_blobs.iter() {
                if !wim.removed_blobs.contains(hash) {
                    refs.insert(*hash, blob.descriptor.blob.reference_count);
                }
            }
        }
        let mut blobs = Vec::new();
        for (hash, count) in refs {
            if wim.removed_blobs.contains(&hash) {
                return Err(55);
            }
            let file_backing = if let Some(owned) = wim.owned_blobs.get(&hash) {
                owned.backing.as_deref()
            } else {
                wim.backing.as_deref()
            };
            let (bytes, backing, descriptor) = if let Some(owned) = wim.owned_blobs.get(&hash) {
                (
                    (owned.backing.is_none() && owned.captured.is_none())
                        .then_some(owned.bytes.as_slice()),
                    owned
                        .backing
                        .as_deref()
                        .map(|backing| backing.bytes())
                        .transpose()
                        .map_err(code)?,
                    Some((
                        owned.descriptor.blob.clone(),
                        owned.descriptor.resource.clone(),
                    )),
                )
            } else {
                let lookup = wim.lookup.as_ref().ok_or(55)?;
                let blob = lookup.find(&hash).ok_or(55)?;
                (
                    None,
                    wim.backing
                        .as_deref()
                        .map(|b| b.bytes())
                        .transpose()
                        .map_err(code)?,
                    Some((blob.clone(), lookup.resources[blob.resource_index].clone())),
                )
            };
            let origin = if let Some(backing) = backing {
                let header = file_backing.ok_or(55)?.header().map_err(code)?;
                (header.guid, header.part_number, backing.as_ptr() as usize)
            } else {
                ([0; 16], 0, usize::MAX)
            };
            let captured = wim.owned_blobs.get(&hash).and_then(|owned| {
                owned.captured.as_ref().map(|stream| CapturedState {
                    stream: stream.clone(),
                    reader: RefCell::new(None),
                    digest: RefCell::new(Sha1::new()),
                    hash: Cell::new(hash),
                    done_early: Cell::new(false),
                    hash_ready: Cell::new(true),
                    ready_order: Cell::new(None),
                    extra_references: Cell::new(0),
                })
            });
            blobs.push(Blob {
                hash,
                captured,
                count,
                bytes,
                backing,
                file_backing,
                descriptor,
                origin,
            });
        }
        let mut captured_indices = BTreeMap::new();
        for capture in captures.iter().flatten() {
            for binding in &capture.bindings {
                let identity = Arc::as_ptr(&binding.stream) as usize;
                if let Some(&index) = captured_indices.get(&identity) {
                    let b: &mut Blob<'_> = &mut blobs[index];
                    b.count = b.count.checked_add(1).ok_or(24)?;
                    continue;
                }
                let entry = capture.tree.nodes.get(binding.node).ok_or(21)?;
                let hash = if binding.slot == 0 {
                    entry.main_hash
                } else {
                    entry.extra_streams.get(binding.slot - 1).ok_or(21)?.hash
                };
                captured_indices.insert(identity, blobs.len());
                blobs.push(Blob {
                    hash,
                    captured: Some(CapturedState {
                        stream: binding.stream.clone(),
                        reader: RefCell::new(None),
                        digest: RefCell::new(Sha1::new()),
                        hash: Cell::new(hash),
                        done_early: Cell::new(false),
                        hash_ready: Cell::new(hash != [0; 20]),
                        ready_order: Cell::new(None),
                        extra_references: Cell::new(0),
                    }),
                    count: 1,
                    bytes: None,
                    backing: None,
                    file_backing: None,
                    descriptor: None,
                    origin: ([0; 16], 0, usize::MAX),
                });
            }
        }
        blobs.sort_by(|a, b| {
            let location = |blob: &Blob<'_>| {
                if blob.backing.is_some() {
                    0
                } else if blob.captured.as_ref().is_some_and(|c| {
                    matches!(
                        c.stream.source,
                        crate::engine::capture::CapturedSource::File(_)
                    )
                }) {
                    1
                } else {
                    2
                }
            };
            location(a).cmp(&location(b)).then_with(|| {
                if let (Some(a), Some(b)) = (&a.captured, &b.captured)
                    && let (
                        crate::engine::capture::CapturedSource::File(a),
                        crate::engine::capture::CapturedSource::File(b),
                    ) = (&a.stream.source, &b.stream.source)
                {
                    return a.cmp(b);
                }
                (
                    a.origin,
                    a.descriptor
                        .as_ref()
                        .map_or((0, 0), |(b, r)| (r.header.offset_in_wim, b.offset)),
                )
                    .cmp(&(
                        b.origin,
                        b.descriptor
                            .as_ref()
                            .map_or((0, 0), |(b, r)| (r.header.offset_in_wim, b.offset)),
                    ))
            })
        });
        let xml = wim
            .xml
            .select_images(
                &selected
                    .images
                    .iter()
                    .map(|&i| i as u32 + 1)
                    .collect::<Vec<_>>(),
            )
            .map_err(code)?;
        Ok(Self {
            wim,
            selected,
            header,
            xml,
            captures,
            metadata,
            metadata_copy,
            blobs,
            append: false,
            compact: false,
            reuse_table: false,
            retained_table: Vec::new(),
            ready_clock: Cell::new(0),
            compressor: RefCell::new(CompressorCache::default()),
        })
    }
    // solid.c uses an extension and basename heuristic only when SOLID is
    // explicitly requested. Stable ties retain the preceding source order.
    fn sort_solid_blobs(&mut self) -> Result<(), c_int> {
        let mut names = Vec::new();
        names.try_reserve_exact(self.blobs.len()).map_err(|_| 39)?;
        names.resize_with(self.blobs.len(), || None::<Vec<u8>>);
        for capture in self.captures.iter().flatten() {
            for binding in &capture.bindings {
                if !matches!(
                    binding.stream.source,
                    crate::engine::capture::CapturedSource::File(_)
                ) {
                    continue;
                }
                let node = capture.tree.nodes.get(binding.node).ok_or(21)?;
                for (i, blob) in self.blobs.iter().enumerate() {
                    if blob
                        .captured
                        .as_ref()
                        .is_some_and(|c| Arc::ptr_eq(&c.stream, &binding.stream))
                    {
                        let mut name = &node.name;
                        if node.attributes & 0x400 == 0 && node.inode_union != 0 {
                            for alias in &capture.tree.nodes {
                                if alias.attributes & 0x400 == 0
                                    && alias.inode_union == node.inode_union
                                    && alias.name.len() < name.len()
                                {
                                    name = &alias.name;
                                }
                            }
                        }
                        if names[i].is_none() {
                            let mut copy = Vec::new();
                            copy.try_reserve_exact(name.len()).map_err(|_| 39)?;
                            copy.extend_from_slice(name);
                            names[i] = Some(copy);
                        }
                    }
                }
            }
        }
        let mut sources = BTreeSet::new();
        for blob in &self.blobs {
            let Some(backing) = blob.backing else {
                continue;
            };
            if !sources.insert(backing.as_ptr() as usize) {
                continue;
            }
            let archive = Archive::open(backing).map_err(code)?;
            for image in 1..=archive.header.image_count {
                let bytes = archive.read_metadata(image).map_err(code)?;
                let metadata = Metadata::parse(&bytes).map_err(code)?;
                for node in &metadata.nodes {
                    let inode = metadata.inode_entry(node.inode).ok_or(21)?;
                    let Some(stream) = inode.streams.iter().find(|s| {
                        s.name.is_empty() && s.kind == wim_format::metadata::StreamType::Data
                    }) else {
                        continue;
                    };
                    let mut name = node.entry.name;
                    for alias in &metadata.nodes {
                        if alias.inode == node.inode && alias.entry.name.len() < name.len() {
                            name = alias.entry.name;
                        }
                    }
                    for (i, candidate) in self.blobs.iter().enumerate() {
                        if names[i].is_none()
                            && candidate.hash == stream.hash
                            && candidate
                                .backing
                                .is_some_and(|b| b.as_ptr() == backing.as_ptr())
                            && candidate
                                .descriptor
                                .as_ref()
                                .is_some_and(|(b, r)| b.size == r.header.uncompressed_size)
                        {
                            let mut copy = Vec::new();
                            copy.try_reserve_exact(name.len()).map_err(|_| 39)?;
                            copy.extend_from_slice(name);
                            names[i] = Some(copy);
                        }
                    }
                }
            }
        }
        fn extension(name: &[u8]) -> Option<&[u8]> {
            for (i, unit) in name.chunks_exact(2).enumerate().rev() {
                let unit = u16::from_le_bytes([unit[0], unit[1]]);
                if unit == b'.' as u16 {
                    return Some(&name[(i + 1) * 2..]);
                }
                if unit == b'/' as u16 || unit == b'\\' as u16 {
                    break;
                }
            }
            None
        }
        fn folded(a: &[u8], b: &[u8]) -> std::cmp::Ordering {
            fn units(bytes: &[u8]) -> impl Iterator<Item = u16> + '_ {
                bytes
                    .chunks_exact(2)
                    .map(|u| wim_format::ntfs_upcase::uppercase(u16::from_le_bytes([u[0], u[1]])))
            }
            units(a).cmp(units(b))
        }
        if names.iter().all(Option::is_none) {
            return Ok(());
        }
        let mut order = Vec::new();
        order.try_reserve_exact(self.blobs.len()).map_err(|_| 39)?;
        order.extend(0..self.blobs.len());
        order.sort_by(|&a, &b| match (&names[a], &names[b]) {
            (None, None) => a.cmp(&b),
            (None, Some(_)) => std::cmp::Ordering::Less,
            (Some(_), None) => std::cmp::Ordering::Greater,
            (Some(a_name), Some(b_name)) => match (extension(a_name), extension(b_name)) {
                (None, None) => std::cmp::Ordering::Equal,
                (None, Some(_)) => std::cmp::Ordering::Less,
                (Some(_), None) => std::cmp::Ordering::Greater,
                (Some(a_ext), Some(b_ext)) => folded(a_ext, b_ext),
            }
            .then_with(|| folded(a_name, b_name))
            .then_with(|| a.cmp(&b)),
        });
        // Rotate in place after every fallible name lookup has completed.
        let mut labels = Vec::new();
        labels.try_reserve_exact(order.len()).map_err(|_| 39)?;
        labels.extend(0..order.len());
        let mut positions = Vec::new();
        positions.try_reserve_exact(order.len()).map_err(|_| 39)?;
        positions.extend(0..order.len());
        for (destination, &original) in order.iter().enumerate() {
            let current = positions[original];
            let displaced = labels[destination];
            self.blobs.swap(destination, current);
            labels.swap(destination, current);
            positions[original] = destination;
            positions[displaced] = current;
        }
        Ok(())
    }
    pub(crate) fn unchanged_append(
        wim: &'a WimHandle,
        selected: &'a Settings,
        header: Header,
    ) -> Result<Self, c_int> {
        let xml = wim
            .xml
            .select_images(
                &selected
                    .images
                    .iter()
                    .map(|i| *i as u32 + 1)
                    .collect::<Vec<_>>(),
            )
            .map_err(code)?;
        Ok(Self {
            wim,
            selected,
            header,
            xml,
            captures: vec![None; selected.images.len()],
            metadata: vec![Vec::new(); selected.images.len()],
            metadata_copy: vec![None; selected.images.len()],
            blobs: Vec::new(),
            append: false,
            compact: false,
            reuse_table: false,
            retained_table: Vec::new(),
            ready_clock: Cell::new(0),
            compressor: RefCell::new(CompressorCache::default()),
        })
    }
    pub(crate) fn retain_part(&mut self, hashes: &[[u8; 20]]) {
        self.blobs.retain(|blob| hashes.contains(&blob.hash()));
        if self.header.part_number != 1 {
            self.metadata.clear();
            self.metadata_copy.clear();
        }
    }
    pub(crate) fn run(
        self,
        file: &mut File,
        effects: &mut Effects,
    ) -> Result<WrittenOutput, c_int> {
        let output = self.header.encode_canonical().to_vec();
        self.run_output(file, effects, Output::new(output, false))
    }
    pub(crate) fn run_compact(
        mut self,
        file: &mut File,
        effects: &mut Effects,
    ) -> Result<WrittenOutput, c_int> {
        self.compact = true;
        let backing = self
            .wim
            .backing
            .as_deref()
            .map(|b| b.bytes())
            .transpose()
            .map_err(code)?
            .ok_or(21)?;
        let lookup = self.wim.lookup.as_ref().ok_or(21)?;
        for &index in &self.selected.images {
            if let crate::engine::handles::HandleImage::Source(source) = self.wim.images[index] {
                let descriptor = lookup
                    .metadata
                    .get((source - 1) as usize)
                    .ok_or(21)?
                    .clone();
                let resource = lookup.resources[descriptor.resource_index].clone();
                self.blobs.push(Blob {
                    hash: descriptor.hash,
                    captured: None,
                    count: 1,
                    bytes: None,
                    backing: Some(backing),
                    file_backing: self.wim.backing.as_deref(),
                    descriptor: Some((descriptor, resource)),
                    origin: (
                        self.wim.header.guid,
                        self.wim.header.part_number,
                        backing.as_ptr() as usize,
                    ),
                });
            }
        }
        self.blobs.sort_by_key(|b| {
            (
                b.origin,
                b.descriptor
                    .as_ref()
                    .map_or(0, |(_, r)| r.header.offset_in_wim),
            )
        });
        let output = self.header.encode_canonical().to_vec();
        self.run_output(file, effects, Output::new(output, false))
    }
    pub(crate) fn run_append(
        mut self,
        file: &mut File,
        effects: &mut Effects,
        end: usize,
        unchanged: bool,
    ) -> Result<WrittenOutput, c_int> {
        self.append = true;
        self.reuse_table = unchanged;
        let backing = self
            .wim
            .backing
            .as_deref()
            .map(|b| b.bytes())
            .transpose()
            .map_err(code)?
            .ok_or(21)?;
        let mut solid_resources = BTreeSet::new();
        // Keep each solid marker adjacent to all of its local blob offsets.
        // Serialization later relocates these groups into the combined range.
        let mut retained: Vec<_> = self
            .blobs
            .iter()
            .filter(|blob| blob.backing.is_some_and(|b| std::ptr::eq(b, backing)))
            .collect();
        retained.sort_by_key(|blob| {
            blob.descriptor
                .as_ref()
                .map(|(_, resource)| (resource.solid, resource.header.offset_in_wim))
        });
        for blob in retained {
            if blob.backing.is_some_and(|b| std::ptr::eq(b, backing)) {
                let (descriptor, resource) = blob.descriptor.as_ref().ok_or(55)?;
                if resource.solid {
                    if solid_resources.insert(resource.header.offset_in_wim) {
                        table_entry(
                            &mut self.retained_table,
                            ResourceHeader {
                                uncompressed_size: wim_format::lookup::SOLID_RESOURCE_MARKER,
                                flags: 16,
                                ..resource.header
                            },
                            [0; 20],
                            1,
                        )
                        .map_err(code)?;
                    }
                    table_entry(
                        &mut self.retained_table,
                        ResourceHeader {
                            size_in_wim: descriptor.size,
                            offset_in_wim: descriptor.offset,
                            uncompressed_size: 0,
                            flags: 16,
                        },
                        blob.hash(),
                        blob.count,
                    )
                    .map_err(code)?;
                    continue;
                }
                table_entry(
                    &mut self.retained_table,
                    ResourceHeader {
                        size_in_wim: resource.header.size_in_wim,
                        offset_in_wim: resource.header.offset_in_wim,
                        uncompressed_size: descriptor.size,
                        flags: resource.header.flags,
                    },
                    blob.hash(),
                    blob.count,
                )
                .map_err(code)?;
            }
        }
        self.blobs
            .retain(|b| !b.backing.is_some_and(|b| std::ptr::eq(b, backing)));
        if unchanged {
            self.header.blob_table = self.wim.header.blob_table;
        }
        if end > backing.len() {
            return Err(55);
        }
        // The prefix is already on disk. Keep absolute offsets without cloning
        // the entire archive (or doubling its allocation on the first append).
        let output = Output::appended(self.header.encode_canonical(), end);
        self.run_output(file, effects, output)
    }
    fn run_output(
        mut self,
        file: &mut File,
        effects: &mut Effects,
        mut output: Output,
    ) -> Result<WrittenOutput, c_int> {
        let streaming = self.wim.output_compression == Compression::None
            && !self.selected.solid
            && !self.append
            && !self.compact;
        output.streaming = streaming;
        let mut sink = Sink {
            file,
            written: output.len() as u64,
        };
        let file = &mut sink;
        let mut table = std::mem::take(&mut self.retained_table);
        if self.selected.pipable {
            self.prehash_pipable_captures()?;
            self.xml.set_total_bytes(None).map_err(code)?;
            let xml = self.xml.encode_utf16le().map_err(code)?;
            pwm(&mut output, xml.len() as u64, Sha1::digest(&xml).into(), 2).map_err(code)?;
            append(&mut output, &xml, 2).map_err(code)?;
            flush(file, &output)?;
            self.metadata(&mut output, &mut table, file)?;
        }
        if self.selected.flags & (0x1000 | 0x4000) == 0x1000
            && !self.compact
            && let Err(_error) = self.sort_solid_blobs()
        {
            crate::engine::diagnostics::message(
                true,
                b"Failed to sort blobs for solid compression. Continuing anyways.",
                false,
            );
        }
        let total_bytes = self.blobs.iter().map(Blob::size).sum();
        let total_parts = self
            .blobs
            .iter()
            .filter(|b| b.backing.is_some())
            .map(|b| b.origin)
            .collect::<BTreeSet<_>>()
            .len()
            .max(1) as u32;
        let mut streams = Streams {
            registration: self.wim.progress.get(),
            info: ProgressInfo::zeroed(),
            next: 0,
            remaining_files: BTreeMap::new(),
        };
        if self.selected.flags & 0x2000 != 0 {
            for blob in &self.blobs {
                if let Some((identity, _)) = blob.file_source() {
                    *streams.remaining_files.entry(identity).or_insert(0) += 1;
                }
            }
        }
        streams.info.write_streams = WriteStreamsProgress {
            total_bytes,
            total_streams: self.blobs.len() as u64,
            completed_bytes: 0,
            completed_streams: 0,
            num_threads: 1,
            compression_type: if self.selected.solid {
                self.wim.output_solid_compression as i32
            } else {
                self.wim.output_compression as i32
            },
            total_parts,
            completed_parts: 0,
            completed_compressed_bytes: 0,
        };
        if !self.blobs.is_empty() {
            streams.call()?;
        }
        if self.selected.solid && !self.compact {
            self.solid(&mut output, &mut table, file, &mut streams)?;
        } else {
            let mut written_hashes = BTreeMap::new();
            for (index, entry) in table.chunks_exact(50).enumerate() {
                let hash: [u8; 20] = entry[30..50].try_into().map_err(|_| 55)?;
                if hash != [0; 20] {
                    written_hashes.insert(hash, index * 50);
                }
            }
            let mut copied_solid = BTreeSet::new();
            for blob in &self.blobs {
                if blob
                    .captured
                    .as_ref()
                    .is_some_and(|captured| captured.done_early.get())
                {
                    continue;
                }
                if blob.captured.is_some()
                    && self
                        .blobs
                        .iter()
                        .filter(|b| b.size() == blob.size())
                        .count()
                        > 1
                {
                    blob.prehash().map_err(code)?;
                    self.mark_ready(blob);
                    if self.selected.flags & 0x2000 != 0 {
                        self.publish_ready_hashes()?;
                    }
                }
                if blob.captured.is_some()
                    && let Some(&entry) = written_hashes.get(&blob.hash())
                {
                    let count = u32::from_le_bytes(
                        table[entry + 26..entry + 30].try_into().map_err(|_| 55)?,
                    )
                    .checked_add(blob.count)
                    .ok_or(24)?;
                    table[entry + 26..entry + 30].copy_from_slice(&count.to_le_bytes());
                    streams.discard(blob.size())?;
                    streams.done_with_file(blob)?;
                    continue;
                }
                if self.compact
                    && let Some((_, resource)) = &blob.descriptor
                    && resource.solid
                {
                    if !copied_solid.insert((blob.origin, resource.header.offset_in_wim)) {
                        continue;
                    }
                    let bytes = range(
                        blob.backing.ok_or(55)?,
                        resource.header.offset_in_wim,
                        resource.header.size_in_wim,
                    )
                    .map_err(code)?;
                    let header = append(&mut output, bytes, 16).map_err(code)?;
                    table_entry(
                        &mut table,
                        ResourceHeader {
                            uncompressed_size: wim_format::lookup::SOLID_RESOURCE_MARKER,
                            ..header
                        },
                        [0; 20],
                        1,
                    )
                    .map_err(code)?;
                    let mut total = 0;
                    let mut count = 0;
                    for b in &self.blobs {
                        if let Some((descriptor, r)) = &b.descriptor
                            && r.solid
                            && b.origin == blob.origin
                            && r.header.offset_in_wim == resource.header.offset_in_wim
                        {
                            table_entry(
                                &mut table,
                                ResourceHeader {
                                    size_in_wim: descriptor.size,
                                    offset_in_wim: descriptor.offset,
                                    uncompressed_size: 0,
                                    flags: 16,
                                },
                                b.hash(),
                                b.count,
                            )
                            .map_err(code)?;
                            total += b.size();
                            count += 1;
                        }
                    }
                    flush(file, &output)?;
                    streams.done(total, bytes.len() as u64, count)?;
                    continue;
                }
                let resource = if self.can_copy(blob) {
                    let (_, r) = blob.descriptor.as_ref().ok_or(55)?;
                    let backing = blob.backing.ok_or(55)?;
                    let bytes = range(backing, r.header.offset_in_wim, r.header.size_in_wim)
                        .map_err(code)?;
                    let mut resource = r.header;
                    if self.selected.pipable {
                        pwm(&mut output, blob.size(), blob.hash(), r.header.flags).map_err(code)?;
                    }
                    resource.offset_in_wim = output.len() as u64;
                    output.extend_from_slice(bytes);
                    flush(file, &output)?;
                    output.discard_flushed();
                    streams.done(blob.size(), bytes.len() as u64, 1)?;
                    resource
                } else {
                    self.encode_data(&mut output, file, &mut streams, blob)?
                };
                let metadata = blob
                    .descriptor
                    .as_ref()
                    .is_some_and(|(_, r)| r.header.flags & 2 != 0);
                let mut resource = resource;
                if metadata {
                    resource.flags |= 2;
                }
                if metadata && self.header.boot_index != 0 {
                    let source = self.selected.images[(self.header.boot_index - 1) as usize];
                    if self
                        .metadata
                        .get(source)
                        .is_some_and(|bytes| <[u8; 20]>::from(Sha1::digest(bytes)) == blob.hash())
                    {
                        self.header.boot_metadata = resource;
                    }
                }
                written_hashes.insert(blob.hash(), table.len());
                table_entry(&mut table, resource, blob.hash(), blob.reference_count()?)
                    .map_err(code)?;
            }
        }
        if !self.selected.pipable {
            self.metadata(&mut output, &mut table, file)?;
        }
        if !self.reuse_table && !output.streaming {
            relocate_solid_table_offsets(&mut table, |offset| {
                let bytes = if offset < output.base as u64 {
                    let backing = self.wim.backing.as_deref().ok_or(55)?;
                    range(backing.bytes().map_err(code)?, offset, 8).map_err(code)?
                } else {
                    range(&output.bytes, offset - output.base as u64, 8).map_err(code)?
                };
                Ok(u64::from_le_bytes(bytes.try_into().map_err(|_| 55)?))
            })?;
        }
        for entry in table.chunks_exact_mut(50) {
            entry[24..26].copy_from_slice(&self.header.part_number.to_le_bytes());
        }
        if !self.reuse_table {
            self.header.blob_table = if table.is_empty() {
                ResourceHeader::default()
            } else if self.selected.pipable {
                pwm(
                    &mut output,
                    table.len() as u64,
                    Sha1::digest(&table).into(),
                    2,
                )
                .map_err(code)?;
                append(&mut output, &table, 2).map_err(code)?
            } else {
                append(&mut output, &table, 2).map_err(code)?
            };
            flush(file, &output)?;
        }
        let check_end = output.len() as u64;
        self.xml.set_total_bytes(Some(check_end)).map_err(code)?;
        if !self.wim.xml.has_total_bytes() && effects.total_bytes.is_none() {
            effects.total_bytes = Some(check_end);
        }
        let xml = self.xml.encode_utf16le().map_err(code)?;
        if self.selected.pipable {
            pwm(&mut output, xml.len() as u64, Sha1::digest(&xml).into(), 2).map_err(code)?;
        }
        self.header.xml_data = append(&mut output, &xml, 2).map_err(code)?;
        flush(file, &output)?;
        if self.selected.integrity && self.header.blob_table.size_in_wim != 0 {
            if self.append && self.reuse_table {
                let mut checkpoint = self.header.clone();
                checkpoint.flags |= 0x40;
                checkpoint.integrity_table = ResourceHeader::default();
                file.seek(SeekFrom::Start(0)).map_err(|_| 72)?;
                file.write_all(&checkpoint.encode_canonical())
                    .map_err(|_| 72)?;
                file.seek(SeekFrom::Start(output.len() as u64))
                    .map_err(|_| 72)?;
            }
            let old_end =
                self.wim.header.blob_table.offset_in_wim + self.wim.header.blob_table.size_in_wim;
            let old_table = if self.append {
                self.wim
                    .backing
                    .as_deref()
                    .map(|b| b.bytes())
                    .transpose()
                    .map_err(code)?
                    .and_then(|b| {
                        range(
                            b,
                            self.wim.header.integrity_table.offset_in_wim,
                            self.wim.header.integrity_table.size_in_wim,
                        )
                        .ok()
                    })
                    .and_then(|b| {
                        IntegrityTable::parse(b, old_end.saturating_sub(HEADER_SIZE as u64)).ok()
                    })
                    .filter(|t| {
                        !t.digests().is_empty() && (4096..=134217728).contains(&t.chunk_size())
                    })
            } else {
                None
            };
            let chunk_size = old_table
                .as_ref()
                .map_or(DEFAULT_CHUNK_SIZE, IntegrityTable::chunk_size);
            let registration = self.wim.progress.get();
            let mut info = ProgressInfo::zeroed();
            info.integrity = IntegrityProgress {
                total_bytes: check_end - HEADER_SIZE as u64,
                completed_bytes: 0,
                total_chunks: (check_end - HEADER_SIZE as u64).div_ceil(chunk_size as u64) as u32,
                completed_chunks: 0,
                chunk_size,
                filename: std::ptr::null(),
            };
            let mut progress = |completed, bytes| {
                info.integrity.completed_chunks = completed;
                info.integrity.completed_bytes = bytes;
                unsafe { registration.call(17, &mut info) }
            };
            let integrity = if output.streaming || output.base != 0 {
                let result = wim_format::integrity::calculate_file(
                    file.file,
                    check_end,
                    chunk_size,
                    &mut progress,
                );
                file.seek(SeekFrom::Start(output.len() as u64))
                    .map_err(|_| 72)?;
                result
            } else {
                IntegrityTable::calculate_with_reuse(
                    &output.bytes,
                    check_end,
                    chunk_size,
                    old_table.as_ref().map(|t| (t, old_end)),
                    &mut progress,
                )
            }
            .map_err(code)?
            .encode()
            .map_err(code)?;
            self.header.integrity_table = append(&mut output, &integrity, 0).map_err(code)?;
            flush(file, &output)?;
        }
        if self.selected.pipable {
            output.extend_from_slice(&self.header.encode_canonical());
            flush(file, &output)?;
        } else {
            output.header = self.header.encode_canonical();
            file.seek(SeekFrom::Start(0)).map_err(|_| 72)?;
            file.write_all(&output.header).map_err(|_| 72)?;
            file.seek(SeekFrom::Start(output.len() as u64))
                .map_err(|_| 72)?;
        }
        if self.selected.flags & 0x20 != 0 {
            file.sync_all().map_err(|_| 72)?;
        }
        Ok(WrittenOutput {
            length: output.len(),
            total_bytes: check_end,
        })
    }
    fn can_copy(&self, b: &Blob<'_>) -> bool {
        (!self.selected.solid || self.compact)
            && self.selected.flags & 16 == 0
            && b.backing.is_some()
            && b.descriptor.as_ref().is_some_and(|(_, r)| {
                !r.solid
                    && r.header.flags & 4 != 0
                    && r.compression_code == self.wim.output_compression as u32
                    && r.chunk_size == self.wim.output_chunk_size
                    && b.file_backing
                        .and_then(|file| file.header().ok())
                        .is_some_and(|header| {
                            (header.magic == PIPABLE_MAGIC) == self.selected.pipable
                        })
            })
    }
    fn publish_capture_hashes(&mut self) -> Result<(), c_int> {
        let mut canonical = BTreeMap::new();
        let known = self
            .blobs
            .iter()
            .filter(|b| b.captured.is_none())
            .map(Blob::hash)
            .collect::<BTreeSet<_>>();
        for blob in &self.blobs {
            if let Some(c) = &blob.captured
                && blob.hash() != [0; 20]
            {
                canonical
                    .entry(blob.hash())
                    .or_insert_with(|| c.stream.clone());
            }
        }
        for (i, capture) in self.captures.iter_mut().enumerate() {
            let Some(capture) = capture else {
                continue;
            };
            for binding in &capture.bindings {
                let entry = capture.tree.nodes.get_mut(binding.node).ok_or(21)?;
                let slot = if binding.slot == 0 {
                    &mut entry.main_hash
                } else {
                    &mut entry
                        .extra_streams
                        .get_mut(binding.slot - 1)
                        .ok_or(21)?
                        .hash
                };
                if let Some(blob) = self.blobs.iter().find(|b| {
                    b.captured
                        .as_ref()
                        .is_some_and(|c| Arc::ptr_eq(&c.stream, &binding.stream))
                }) {
                    *slot = blob.hash();
                }
                if *slot == [0; 20] {
                    return Err(55);
                }
            }
            capture.bindings.retain_mut(|binding| {
                let entry = &capture.tree.nodes[binding.node];
                let hash = if binding.slot == 0 {
                    entry.main_hash
                } else {
                    entry.extra_streams[binding.slot - 1].hash
                };
                if known.contains(&hash) {
                    return false;
                }
                if let Some(stream) = canonical.get(&hash) {
                    binding.stream = stream.clone();
                }
                true
            });
            let shared =
                crate::engine::handles::pending_metadata(&self.wim.images[self.selected.images[i]])
                    .ok_or(21)?;
            shared.lock().map_err(|_| 24)?.capture = Some(capture.clone());
        }
        self.publish_index_hashes(false)
    }
    fn prehash_pipable_captures(&mut self) -> Result<(), c_int> {
        for blob in &self.blobs {
            if blob.captured.is_some() && blob.hash() == [0; 20] {
                blob.prehash().map_err(code)?;
                self.mark_ready(blob);
            }
        }
        self.publish_capture_hashes()?;
        let mut hashes = BTreeMap::new();
        let mut index = 0;
        while index < self.blobs.len() {
            let hash = self.blobs[index].hash();
            if let Some(&previous) = hashes.get(&hash) {
                let count = self.blobs[index].count;
                let b: &mut Blob<'_> = &mut self.blobs[previous];
                b.count = b.count.checked_add(count).ok_or(24)?;
                self.blobs.remove(index);
            } else {
                hashes.insert(hash, index);
                index += 1;
            }
        }
        Ok(())
    }
    fn metadata(
        &mut self,
        output: &mut Output,
        table: &mut Vec<u8>,
        file: &mut Sink<'_>,
    ) -> Result<(), c_int> {
        if self.header.part_number != 1 {
            return Ok(());
        }
        self.publish_capture_hashes()?;
        unsafe { self.wim.progress.get().call(13, std::ptr::null_mut()) }.map_err(code)?;
        super::materialize_empty_images(self.wim, self.selected)?;
        for (i, &index) in self.selected.images.iter().enumerate() {
            if let Some(shared) = crate::engine::handles::pending_metadata(&self.wim.images[index])
            {
                let mut shared = shared.lock().map_err(|_| 24)?;
                if let Some(capture) = &mut self.captures[i] {
                    if capture.tree.nodes.is_empty() {
                        let metadata = Metadata::parse(&shared.metadata).map_err(code)?;
                        capture.tree =
                            wim_format::metadata_write::OwnedMetadata::from_metadata(&metadata)
                                .map_err(code)?;
                        drop(metadata);
                        shared.capture = Some(capture.clone());
                    }
                    shared.metadata = capture.tree.encode().map_err(code)?;
                }
                self.metadata[i] = shared.metadata.clone();
                self.metadata_copy[i] = None;
            }
        }
        for (i, bytes) in self.metadata.iter().enumerate() {
            if (self.append || self.compact)
                && let crate::engine::handles::HandleImage::Source(source) =
                    self.wim.images[self.selected.images[i]]
            {
                let lookup = self.wim.lookup.as_ref().ok_or(21)?;
                let blob = lookup.metadata.get((source - 1) as usize).ok_or(21)?;
                let resource = lookup.resources[blob.resource_index].header;
                if !self.compact && self.header.boot_index == i as u32 + 1 {
                    self.header.boot_metadata = resource;
                }
                if !self.compact && !self.reuse_table {
                    table_entry(table, resource, blob.hash, 1).map_err(code)?;
                }
                continue;
            }
            let hash = Sha1::digest(bytes).into();
            if self.selected.pipable {
                pwm(output, bytes.len() as u64, hash, 2).map_err(code)?;
            }
            let encoded = if let Some((header, bytes)) = &self.metadata_copy[i] {
                wim_format::resource_write::EncodedResource {
                    header: *header,
                    bytes: bytes.clone(),
                }
            } else {
                encode_resource(
                    bytes,
                    self.wim.output_compression,
                    self.wim.output_chunk_size,
                    if self.selected.pipable {
                        ResourceLayout::Pipable
                    } else {
                        ResourceLayout::Ordinary
                    },
                    |kind, chunk| {
                        self.compressor.borrow_mut().compress(
                            kind,
                            chunk,
                            self.wim.output_chunk_size,
                        )
                    },
                )
                .map_err(code)?
            };
            let mut r = encoded.header;
            r.flags |= 2;
            r.offset_in_wim = output.len() as u64;
            output.extend_from_slice(&encoded.bytes);
            if self.header.boot_index == i as u32 + 1 {
                self.header.boot_metadata = r;
            }
            table_entry(table, r, hash, 1).map_err(code)?;
            flush(file, output)?;
            if let Some(shared) =
                crate::engine::handles::pending_metadata(&self.wim.images[self.selected.images[i]])
            {
                shared.lock().map_err(|_| 24)?.hash = hash;
            }
        }
        unsafe { self.wim.progress.get().call(14, std::ptr::null_mut()) }.map_err(code)
    }
    fn mark_ready(&self, blob: &Blob<'_>) {
        if let Some(captured) = &blob.captured {
            captured.hash_ready.set(true);
            if captured.ready_order.get().is_none() {
                let order = self.ready_clock.get();
                captured.ready_order.set(Some(order));
                self.ready_clock.set(order + 1);
            }
        }
    }
    fn publish_index_hashes(&self, only_ready: bool) -> Result<(), c_int> {
        let mut published: Vec<_> = self
            .blobs
            .iter()
            .filter(|blob| {
                blob.captured.as_ref().is_some_and(|captured| {
                    (!only_ready || captured.hash_ready.get()) && blob.hash() != [0; 20]
                })
            })
            .collect();
        published.sort_by_key(|blob| {
            blob.captured
                .as_ref()
                .and_then(|captured| captured.ready_order.get())
                .unwrap_or(usize::MAX)
        });
        for blob in published {
            self.wim
                .blob_index
                .insert(blob.hash(), crate::engine::blob_index::BlobOwner::Captured)
                .map_err(code)?;
        }
        Ok(())
    }
    fn publish_ready_hashes(&self) -> Result<(), c_int> {
        let mut ready = BTreeMap::new();
        let mut canonical = BTreeMap::new();
        for blob in &self.blobs {
            if let Some(captured) = &blob.captured
                && captured.hash_ready.get()
                && blob.hash() != [0; 20]
            {
                ready.insert(Arc::as_ptr(&captured.stream) as usize, blob.hash());
                canonical
                    .entry(blob.hash())
                    .or_insert_with(|| captured.stream.clone());
            }
        }
        if ready.is_empty() {
            return Ok(());
        }
        for image in &self.wim.images {
            let Some(pending) = crate::engine::handles::pending_metadata(image) else {
                continue;
            };
            let mut pending = pending.lock().map_err(|_| 24)?;
            let Some(capture) = &mut pending.capture else {
                continue;
            };
            let mut changed = false;
            for binding in &mut capture.bindings {
                let Some(&hash) = ready.get(&(Arc::as_ptr(&binding.stream) as usize)) else {
                    continue;
                };
                let entry = capture.tree.nodes.get_mut(binding.node).ok_or(21)?;
                let slot = if binding.slot == 0 {
                    &mut entry.main_hash
                } else {
                    &mut entry
                        .extra_streams
                        .get_mut(binding.slot - 1)
                        .ok_or(21)?
                        .hash
                };
                changed |= *slot != hash;
                *slot = hash;
                if let Some(stream) = canonical.get(&hash) {
                    binding.stream = stream.clone();
                }
            }
            if changed && !capture.tree.nodes.is_empty() {
                let metadata = capture.tree.encode().map_err(code)?;
                pending.metadata = metadata;
            }
        }
        self.publish_index_hashes(true)
    }
    fn finish_duplicate_lookahead(
        &self,
        blob: &Blob<'_>,
        streams: &mut Streams,
    ) -> Result<(), c_int> {
        let Some(current) = &blob.captured else {
            return Ok(());
        };
        // The current captured read has completed before compressor lookahead
        // starts another source, so preserve that hash publication order.
        self.mark_ready(blob);
        let mut after_current = false;
        for other in &self.blobs {
            if std::ptr::eq(blob, other) {
                after_current = true;
                continue;
            }
            if !after_current || other.size() != blob.size() {
                continue;
            }
            let Some(candidate) = &other.captured else {
                continue;
            };
            if candidate.done_early.get() {
                continue;
            }
            other.prehash().map_err(code)?;
            self.mark_ready(other);
            self.publish_ready_hashes()?;
            if other.hash() != blob.hash() {
                continue;
            }
            current.extra_references.set(
                current
                    .extra_references
                    .get()
                    .checked_add(other.count)
                    .ok_or(24)?,
            );
            candidate.done_early.set(true);
            streams.discard(other.size())?;
            streams.done_with_file(other)?;
        }
        Ok(())
    }
    fn encode_data(
        &self,
        output: &mut Output,
        file: &mut Sink<'_>,
        streams: &mut Streams,
        blob: &Blob<'_>,
    ) -> Result<ResourceHeader, c_int> {
        let hash = blob.hash();
        let size = usize::try_from(blob.size()).map_err(|_| 39)?;
        let compression = self.wim.output_compression;
        let chunk = self.wim.output_chunk_size;
        let layout = if self.selected.pipable {
            ResourceLayout::Pipable
        } else {
            ResourceLayout::Ordinary
        };
        if self.selected.pipable {
            pwm(
                output,
                size as u64,
                hash,
                if compression == Compression::None {
                    0
                } else {
                    4
                },
            )
            .map_err(code)?;
        }
        let start = output.len();
        let count = if compression == Compression::None {
            0
        } else {
            size.div_ceil(chunk as usize)
        };
        let width = if size as u64 > u32::MAX as u64 { 8 } else { 4 };
        if compression != Compression::None && !self.selected.pipable {
            output.resize(output.len() + count.saturating_sub(1) * width, 0);
        }
        // Captured reads already hash each chunk for deferred hash publication.
        // Reuse that emission digest, including when checking a prehashed source.
        let mut digest = blob.captured.is_none().then(Sha1::new);
        let mut final_progress = None;
        let mut store = |stored: &[u8], size: usize, last: bool| {
            if last && compression != Compression::None && self.selected.flags & 0x2000 != 0 {
                self.finish_duplicate_lookahead(blob, streams)
                    .map_err(error)?;
            }
            if self.selected.pipable && compression != Compression::None {
                output.extend_from_slice(&(stored.len() as u32).to_le_bytes());
            }
            if output.streaming {
                flush(file, output).map_err(error)?;
                output.discard_flushed();
                file.write_all(stored).map_err(|_| ParseError::Write)?;
                output.base = output
                    .base
                    .checked_add(stored.len())
                    .ok_or(ParseError::Write)?;
            } else {
                output.extend_from_slice(stored);
                flush(file, output).map_err(error)?;
            }
            if last && self.selected.flags & 0x2000 != 0 && blob.file_source().is_some() {
                final_progress = Some((size as u64, stored.len() as u64));
                Ok(())
            } else {
                streams
                    .done(size as u64, stored.len() as u64, u64::from(last))
                    .map_err(error)
            }
        };
        let encoded = if compression == Compression::None && blob.captured.is_some() {
            let mut buffer = [0; 32768];
            for offset in (0..size).step_by(buffer.len()) {
                let end = (offset + buffer.len()).min(size);
                let bytes = &mut buffer[..end - offset];
                blob.read_captured_range(offset..end, bytes).map_err(code)?;
                store(bytes, bytes.len(), end == size).map_err(code)?;
            }
            wim_format::resource_write::StreamedResource {
                prefix: Vec::new(),
                suffix: Vec::new(),
                header: ResourceHeader {
                    size_in_wim: size as u64,
                    flags: 0,
                    offset_in_wim: 0,
                    uncompressed_size: size as u64,
                },
            }
        } else {
            stream_resource_from_chunks(
                size,
                compression,
                chunk,
                layout,
                |range| {
                    let data = blob.read_range(range)?;
                    if let Some(digest) = &mut digest {
                        digest.update(&data);
                    }
                    Ok(data)
                },
                |kind, data| self.compressor.borrow_mut().compress(kind, data, chunk),
                store,
            )
            .map_err(code)?
        };
        let actual_hash: [u8; 20] = match &blob.captured {
            Some(captured) => captured.digest.borrow().clone().finalize().into(),
            None => digest.ok_or(24)?.finalize().into(),
        };
        if hash != [0; 20] && actual_hash != hash {
            return Err(if blob.captured.is_some() { 88 } else { 28 });
        }
        if !encoded.prefix.is_empty() {
            output.patch(start, &encoded.prefix)?;
        }
        if !encoded.prefix.is_empty() {
            file.seek(SeekFrom::Start(start as u64)).map_err(|_| 72)?;
            file.write_all(&encoded.prefix).map_err(|_| 72)?;
            file.seek(SeekFrom::Start(output.len() as u64))
                .map_err(|_| 72)?;
        }
        output.extend_from_slice(&encoded.suffix);
        flush(file, output)?;
        if let Some((bytes, compressed)) = final_progress {
            if compression != Compression::None
                && let Some(captured) = &blob.captured
            {
                captured.hash_ready.set(true);
                self.mark_ready(blob);
            }
            self.publish_ready_hashes()?;
            streams.done_with_file(blob)?;
            streams.done(bytes, compressed, 1)?;
        }
        if let Some(captured) = &blob.captured {
            captured.hash_ready.set(true);
            self.mark_ready(blob);
        }
        if self.selected.flags & 0x2000 != 0 {
            self.publish_ready_hashes()?;
        }
        let mut resource = encoded.header;
        resource.offset_in_wim = start as u64;
        Ok(resource)
    }
    fn solid(
        &mut self,
        output: &mut Output,
        table: &mut Vec<u8>,
        file: &mut Sink<'_>,
        streams: &mut Streams,
    ) -> Result<(), c_int> {
        if self.blobs.is_empty() {
            return Ok(());
        }
        let copy = self.selected.flags & 16 == 0
            && self.blobs.iter().all(|b| {
                b.backing.is_some() && b.descriptor.as_ref().is_some_and(|(_, r)| r.solid)
            });
        if copy {
            let mut groups: BTreeMap<_, Vec<&Blob<'_>>> = BTreeMap::new();
            for b in &self.blobs {
                let (_, r) = b.descriptor.as_ref().ok_or(55)?;
                groups
                    .entry((b.origin, r.header.offset_in_wim))
                    .or_default()
                    .push(b);
            }
            for blobs in groups.values() {
                let first = blobs[0];
                let (_, resource) = first.descriptor.as_ref().ok_or(55)?;
                let backing = first.backing.ok_or(55)?;
                let bytes = range(
                    backing,
                    resource.header.offset_in_wim,
                    resource.header.size_in_wim,
                )
                .map_err(code)?;
                let r = append(output, bytes, 16).map_err(code)?;
                table_entry(
                    table,
                    ResourceHeader {
                        uncompressed_size: wim_format::lookup::SOLID_RESOURCE_MARKER,
                        ..r
                    },
                    [0; 20],
                    1,
                )
                .map_err(code)?;
                for b in blobs {
                    let (blob, _) = b.descriptor.as_ref().ok_or(55)?;
                    table_entry(
                        table,
                        ResourceHeader {
                            size_in_wim: blob.size,
                            offset_in_wim: blob.offset,
                            uncompressed_size: 0,
                            flags: 16,
                        },
                        b.hash(),
                        b.count,
                    )
                    .map_err(code)?;
                }
                flush(file, output)?;
                // write_raw_copy_resources copies the shared resource on the
                // first descriptor, then completes each descriptor separately.
                for (index, blob) in blobs.iter().enumerate() {
                    streams.done(
                        blob.size(),
                        if index == 0 { bytes.len() as u64 } else { 0 },
                        1,
                    )?;
                }
            }
            return Ok(());
        }
        let chunk = self.wim.output_solid_chunk_size;
        let candidates: Vec<_> = self
            .blobs
            .iter()
            .enumerate()
            .filter_map(|(index, blob)| {
                (blob.captured.is_some()
                    && self
                        .blobs
                        .iter()
                        .filter(|other| other.size() == blob.size())
                        .count()
                        > 1)
                .then_some(index)
            })
            .collect();
        if !candidates.is_empty() {
            // The source reader starts after the solid resource header has reached the target.
            let start = output.len();
            let total: u64 = self.blobs.iter().map(Blob::size).sum();
            output.extend_from_slice(&total.to_le_bytes());
            output.extend_from_slice(&chunk.to_le_bytes());
            output.extend_from_slice(&(self.wim.output_solid_compression as u32).to_le_bytes());
            output.resize(output.len() + 4 * total.div_ceil(chunk as u64) as usize, 0);
            flush(file, output)?;
            for blob in &self.blobs {
                if blob.captured.is_some() {
                    blob.prehash().map_err(code)?;
                    self.mark_ready(blob);
                }
            }
            self.publish_capture_hashes()?;
            let mut hashes = BTreeMap::new();
            let mut index = 0;
            while index < self.blobs.len() {
                let hash = self.blobs[index].hash();
                if hash != [0; 20]
                    && let Some(&previous) = hashes.get(&hash)
                {
                    let duplicate = self.blobs.remove(index);
                    let retained: &mut Blob<'_> = &mut self.blobs[previous];
                    retained.count = retained.count.checked_add(duplicate.count).ok_or(24)?;
                    streams.discard(duplicate.size())?;
                    streams.done_with_file(&duplicate)?;
                } else {
                    hashes.insert(hash, index);
                    index += 1;
                }
            }
            output.truncate(start);
            file.seek(SeekFrom::Start(start as u64)).map_err(|_| 72)?;
        }
        let total = self.blobs.iter().try_fold(0usize, |total, b| {
            total
                .checked_add(usize::try_from(b.size()).map_err(|_| 39)?)
                .ok_or(39)
        })?;
        let start = output.len();
        let count = total.div_ceil(chunk as usize);
        output.resize(start + 16 + 4 * count, 0);
        output.patch(start, &(total as u64).to_le_bytes())?;
        output.patch(start + 8, &chunk.to_le_bytes())?;
        output.patch(
            start + 12,
            &(self.wim.output_solid_compression as u32).to_le_bytes(),
        )?;
        flush(file, output)?;
        let mut ends = Vec::new();
        let mut end = 0;
        for blob in &self.blobs {
            end += blob.size() as usize;
            ends.push(end);
        }
        let mut digests = self.blobs.iter().map(|_| Sha1::new()).collect::<Vec<_>>();
        let mut completed = 0;
        let mut completed_blobs = 0;
        let encoded = stream_resource_from_chunks(
            total,
            self.wim.output_solid_compression,
            chunk,
            ResourceLayout::Solid,
            |requested| {
                let mut bytes = Vec::new();
                bytes
                    .try_reserve_exact(requested.len())
                    .map_err(|_| ParseError::Nomem)?;
                let mut begin = 0;
                for (i, (blob, &end)) in self.blobs.iter().zip(&ends).enumerate() {
                    let read_start = requested.start.max(begin);
                    let read_end = requested.end.min(end);
                    if read_start < read_end {
                        let data = blob.read_range(read_start - begin..read_end - begin)?;
                        digests[i].update(&data);
                        bytes.extend_from_slice(&data);
                        if read_end == end
                            && <[u8; 20]>::from(digests[i].clone().finalize()) != blob.hash()
                        {
                            return Err(if blob.captured.is_some() {
                                ParseError::ConcurrentModificationDetected
                            } else {
                                ParseError::InvalidResourceHash
                            });
                        }
                    }
                    begin = end;
                }
                Ok(std::borrow::Cow::Owned(bytes))
            },
            |kind, bytes| self.compressor.borrow_mut().compress(kind, bytes, chunk),
            |stored, size, _| {
                output.extend_from_slice(stored);
                completed += size;
                let n = ends.iter().filter(|&&end| end <= completed).count();
                let count = n - completed_blobs;
                flush(file, output).map_err(error)?;
                if self.selected.flags & 0x2000 != 0 {
                    for blob in &self.blobs {
                        if let Some(captured) = &blob.captured
                            && captured.hash.get() != [0; 20]
                        {
                            captured.hash_ready.set(true);
                            self.mark_ready(blob);
                        }
                    }
                    self.publish_ready_hashes().map_err(error)?;
                }
                for blob in &self.blobs[completed_blobs..n] {
                    streams.done_with_file(blob).map_err(error)?;
                }
                completed_blobs = n;
                streams
                    .done(size as u64, stored.len() as u64, count as u64)
                    .map_err(error)
            },
        )
        .map_err(code)?;
        if !encoded.prefix.is_empty() {
            output.patch(start, &encoded.prefix)?;
        }
        file.seek(SeekFrom::Start(start as u64)).map_err(|_| 72)?;
        file.write_all(&encoded.prefix).map_err(|_| 72)?;
        file.seek(SeekFrom::Start(output.len() as u64))
            .map_err(|_| 72)?;
        table_entry(
            table,
            ResourceHeader {
                offset_in_wim: start as u64,
                uncompressed_size: wim_format::lookup::SOLID_RESOURCE_MARKER,
                ..encoded.header
            },
            [0; 20],
            1,
        )
        .map_err(code)?;
        let mut offset = 0;
        for b in &self.blobs {
            table_entry(
                table,
                ResourceHeader {
                    size_in_wim: b.size(),
                    offset_in_wim: offset,
                    uncompressed_size: 0,
                    flags: 16,
                },
                b.hash(),
                b.count,
            )
            .map_err(code)?;
            offset += b.size();
        }
        Ok(())
    }
}
use std::ffi::c_int;
impl Blob<'_> {
    fn reference_count(&self) -> Result<u32, c_int> {
        self.count
            .checked_add(
                self.captured
                    .as_ref()
                    .map_or(0, |captured| captured.extra_references.get()),
            )
            .ok_or(24)
    }

    fn file_source(&self) -> Option<((u64, u64, u64), &std::path::Path)> {
        let captured = self.captured.as_ref()?;
        let crate::engine::capture::CapturedSource::File(path) = &captured.stream.source else {
            return None;
        };
        let id = captured.stream.identity;
        Some(((id.session, id.device, id.inode), path.as_path()))
    }

    fn hash(&self) -> [u8; 20] {
        self.captured.as_ref().map_or(self.hash, |c| c.hash.get())
    }
    fn prehash(&self) -> Result<(), ParseError> {
        if self.captured.is_none() {
            return Ok(());
        }
        let mut offset = 0usize;
        let size = usize::try_from(self.size()).map_err(|_| ParseError::Nomem)?;
        let mut buffer = [0; 32768];
        while offset < size {
            let end = (offset + buffer.len()).min(size);
            self.read_captured_range(offset..end, &mut buffer[..end - offset])?;
            offset = end;
        }
        if let Some(captured) = &self.captured {
            captured.hash_ready.set(true);
        }
        Ok(())
    }
    fn read_captured_range(
        &self,
        range: std::ops::Range<usize>,
        bytes: &mut [u8],
    ) -> Result<(), ParseError> {
        if range.len() != bytes.len() {
            return Err(ParseError::InvalidParam);
        }
        let capture = self.captured.as_ref().ok_or(ParseError::InvalidParam)?;
        match &capture.stream.source {
            crate::engine::capture::CapturedSource::File(_) => {
                let mut reader = capture.reader.borrow_mut();
                if reader.is_none() {
                    let crate::engine::capture::CapturedReader::File(file) =
                        capture.stream.open()?
                    else {
                        return Err(ParseError::InvalidParam);
                    };
                    *reader = Some(file);
                }
                let mut source = crate::engine::capture::CapturedReader::File(
                    reader.take().ok_or(ParseError::Read)?,
                );
                let result = source.read_range(range.start as u64, bytes);
                let crate::engine::capture::CapturedReader::File(file) = source else {
                    return Err(ParseError::InvalidParam);
                };
                *reader = Some(file);
                result?;
            }
            #[cfg(windows)]
            crate::engine::capture::CapturedSource::Temporary(_) => {
                capture
                    .stream
                    .open()?
                    .read_range(range.start as u64, bytes)?;
            }
            #[cfg(feature = "disk-capture")]
            crate::engine::capture::CapturedSource::Volume(source) => {
                source
                    .read_exact_at(range.start as u64, bytes)
                    .map_err(|_| ParseError::Read)?;
            }
            crate::engine::capture::CapturedSource::Inline(source) => {
                bytes.copy_from_slice(source.get(range.clone()).ok_or(ParseError::Read)?)
            }
        }
        let mut digest = capture.digest.borrow_mut();
        if range.start == 0 {
            *digest = Sha1::new();
        }
        digest.update(bytes);
        if range.end as u64 == capture.stream.size && capture.hash.get() == [0; 20] {
            capture.hash.set(digest.clone().finalize().into());
        }
        Ok(())
    }
    fn read_range(
        &self,
        range: std::ops::Range<usize>,
    ) -> Result<std::borrow::Cow<'_, [u8]>, ParseError> {
        if self.captured.is_some() {
            let mut bytes = Vec::new();
            bytes
                .try_reserve_exact(range.len())
                .map_err(|_| ParseError::Nomem)?;
            bytes.resize(range.len(), 0);
            self.read_captured_range(range, &mut bytes)?;
            return Ok(std::borrow::Cow::Owned(bytes));
        }
        if let Some(bytes) = self.bytes {
            return Ok(std::borrow::Cow::Borrowed(&bytes[range]));
        }
        let archive = self
            .file_backing
            .ok_or(ParseError::ResourceNotFound)?
            .archive()?;
        if self
            .descriptor
            .as_ref()
            .is_some_and(|(_, resource)| resource.header.flags & 2 != 0)
        {
            let index = archive
                .lookup
                .metadata
                .iter()
                .position(|b| b.hash == self.hash)
                .ok_or(ParseError::ResourceNotFound)?;
            let bytes = archive.read_metadata(index as u32 + 1)?;
            return Ok(std::borrow::Cow::Owned(bytes[range].to_vec()));
        }
        Ok(std::borrow::Cow::Owned(archive.read_blob_range(
            &self.hash,
            range.start as u64..range.end as u64,
        )?))
    }
    fn size(&self) -> u64 {
        if let Some(captured) = &self.captured {
            return captured.stream.size;
        }
        self.bytes.map_or_else(
            || self.descriptor.as_ref().map_or(0, |(b, _)| b.size),
            |b| b.len() as u64,
        )
    }
}
fn code(e: ParseError) -> c_int {
    e as c_int
}
fn error(e: c_int) -> ParseError {
    match e {
        76 => ParseError::AbortedByProgress,
        77 => ParseError::UnknownProgressStatus,
        _ => ParseError::Write,
    }
}
fn range(bytes: &[u8], offset: u64, size: u64) -> Result<&[u8], ParseError> {
    let start = usize::try_from(offset).map_err(|_| ParseError::UnexpectedEndOfFile)?;
    let end = usize::try_from(
        offset
            .checked_add(size)
            .ok_or(ParseError::UnexpectedEndOfFile)?,
    )
    .map_err(|_| ParseError::UnexpectedEndOfFile)?;
    bytes.get(start..end).ok_or(ParseError::UnexpectedEndOfFile)
}
// Writer paths emit a marker followed by blobs with resource-local offsets.
// On disk, consecutive solid groups form one concatenated address space.
fn relocate_solid_table_offsets(
    table: &mut [u8],
    mut resource_size: impl FnMut(u64) -> Result<u64, c_int>,
) -> Result<(), c_int> {
    let mut base = 0u64;
    let mut next_base = 0u64;
    for entry in table.chunks_exact_mut(50) {
        let descriptor = ResourceHeader::parse(entry).map_err(code)?;
        if descriptor.flags & 16 == 0 {
            base = 0;
            next_base = 0;
        } else if descriptor.uncompressed_size == wim_format::lookup::SOLID_RESOURCE_MARKER {
            base = next_base;
            let size = resource_size(descriptor.offset_in_wim)?;
            next_base = base.checked_add(size).ok_or(24)?;
        } else {
            let offset = base.checked_add(descriptor.offset_in_wim).ok_or(24)?;
            entry[8..16].copy_from_slice(&offset.to_le_bytes());
        }
    }
    Ok(())
}
struct Sink<'a> {
    file: &'a mut File,
    written: u64,
}
impl Write for Sink<'_> {
    fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
        let n = self.file.write(bytes)?;
        self.written += n as u64;
        Ok(n)
    }
    fn flush(&mut self) -> std::io::Result<()> {
        self.file.flush()
    }
}
impl Seek for Sink<'_> {
    fn seek(&mut self, pos: SeekFrom) -> std::io::Result<u64> {
        let n = self.file.seek(pos)?;
        self.written = n;
        Ok(n)
    }
}
impl Sink<'_> {
    fn sync_all(&self) -> std::io::Result<()> {
        self.file.sync_all()
    }
}
fn flush(file: &mut Sink<'_>, output: &Output) -> Result<(), c_int> {
    let pos = usize::try_from(file.written).map_err(|_| 72)?;
    if pos < output.len() {
        let offset = pos.checked_sub(output.base).ok_or(72)?;
        file.write_all(output.bytes.get(offset..).ok_or(72)?)
            .map_err(|_| 72)?;
    }
    Ok(())
}

fn pwm(output: &mut Output, size: u64, hash: [u8; 20], flags: u8) -> Result<(), ParseError> {
    output.try_reserve(40).map_err(|_| ParseError::Nomem)?;
    output.extend_from_slice(&0x2b9b9ba2443db9d8u64.to_le_bytes());
    output.extend_from_slice(&size.to_le_bytes());
    output.extend_from_slice(&hash);
    output.extend_from_slice(&u32::from(flags).to_le_bytes());
    Ok(())
}
#[derive(Default)]
struct CompressorCache {
    current: Option<(
        Compression,
        u32,
        ms_compress::context::FixedStrategyCompressor,
    )>,
}
impl CompressorCache {
    fn compress(
        &mut self,
        kind: Compression,
        data: &[u8],
        chunk: u32,
    ) -> Result<Option<Vec<u8>>, ParseError> {
        if kind == Compression::None
            || data.is_empty()
            || (kind == Compression::Xpress && data.len() <= 260)
            || (kind == Compression::Lzms && data.len() < 8)
        {
            return Ok(None);
        }
        let maximum = if kind == Compression::Lzx {
            chunk as usize
        } else {
            data.len()
        };
        if !self
            .current
            .as_ref()
            .is_some_and(|(codec, window, compressor)| {
                *codec == kind && *window == chunk && compressor.maximum() >= maximum
            })
        {
            // Contexts reset their independent-block state while retaining scratch.
            // Drop the old workspace before growing or switching codec so its peak
            // storage does not overlap with the replacement.
            self.current = None;
            let compressor =
                ms_compress::context::FixedStrategyCompressor::new(kind as i32, maximum)
                    .map_err(|_| ParseError::Nomem)?;
            self.current = Some((kind, chunk, compressor));
        }
        self.current
            .as_mut()
            .ok_or(ParseError::Nomem)?
            .2
            .compress(data, data.len())
            .map_err(|_| ParseError::Nomem)
    }
}

struct Streams {
    registration: ProgressRegistration,
    info: ProgressInfo,
    next: u64,
    remaining_files: BTreeMap<(u64, u64, u64), usize>,
}
impl Streams {
    fn done_with_file(&mut self, blob: &Blob<'_>) -> Result<(), c_int> {
        let Some((identity, path)) = blob.file_source() else {
            return Ok(());
        };
        let Some(remaining) = self.remaining_files.get_mut(&identity) else {
            return Ok(());
        };
        *remaining -= 1;
        if *remaining != 0 || !self.registration.is_registered() {
            return Ok(());
        }
        if let Some(captured) = &blob.captured {
            captured.reader.borrow_mut().take();
        }
        let path = filename_buffer(Some(path)).map_err(code)?.ok_or(24)?;
        let mut info = ProgressInfo::zeroed();
        info.done_with_file = DoneWithFileProgress {
            path_to_file: path.as_ptr(),
        };
        // SAFETY: The phase snapshot and borrowed pathname remain live; no handle lock is held.
        unsafe { self.registration.call(26, &mut info) }.map_err(code)
    }

    fn call(&mut self) -> Result<(), c_int> {
        unsafe { self.registration.call(12, &mut self.info) }.map_err(code)
    }
    fn discard(&mut self, size: u64) -> Result<(), c_int> {
        let p = unsafe { &mut self.info.write_streams };
        p.total_bytes -= size;
        p.total_streams -= 1;
        self.next = self.next.min(p.total_bytes);
        if p.completed_bytes >= self.next {
            self.call()?;
            let p = unsafe { self.info.write_streams };
            self.next = next_progress(p.completed_bytes, p.total_bytes, self.next);
        }
        Ok(())
    }
    fn done(&mut self, bytes: u64, compressed: u64, streams: u64) -> Result<(), c_int> {
        let p = unsafe { &mut self.info.write_streams };
        p.completed_bytes += bytes;
        p.completed_compressed_bytes += compressed;
        p.completed_streams += streams;
        if p.completed_bytes >= self.next {
            self.call()?;
            let p = unsafe { self.info.write_streams };
            self.next = next_progress(p.completed_bytes, p.total_bytes, self.next);
        }
        Ok(())
    }
}

/// Completion information independent of the enclosing archive bytes.
pub(crate) struct WrittenOutput {
    length: usize,
    pub(crate) total_bytes: u64,
}
impl WrittenOutput {
    pub(crate) fn len(&self) -> usize {
        self.length
    }
}
struct Output {
    header: [u8; HEADER_SIZE],
    bytes: Vec<u8>,
    base: usize,
    streaming: bool,
}
impl Output {
    fn appended(header: [u8; HEADER_SIZE], base: usize) -> Self {
        Self {
            header,
            bytes: Vec::new(),
            base,
            streaming: false,
        }
    }
    fn new(bytes: Vec<u8>, streaming: bool) -> Self {
        let mut header = [0; HEADER_SIZE];
        header.copy_from_slice(&bytes[..HEADER_SIZE]);
        Self {
            header,
            bytes,
            base: 0,
            streaming,
        }
    }
    fn len(&self) -> usize {
        self.base + self.bytes.len()
    }
    fn extend_from_slice(&mut self, bytes: &[u8]) {
        self.bytes.extend_from_slice(bytes);
    }
    fn try_reserve(&mut self, size: usize) -> Result<(), std::collections::TryReserveError> {
        self.bytes.try_reserve(size)
    }
    fn resize(&mut self, length: usize, value: u8) {
        self.bytes.resize(length - self.base, value);
    }
    fn truncate(&mut self, length: usize) {
        self.bytes.truncate(length - self.base);
    }
    fn patch(&mut self, start: usize, bytes: &[u8]) -> Result<(), c_int> {
        let offset = start.checked_sub(self.base).ok_or(72)?;
        self.bytes
            .get_mut(offset..offset.checked_add(bytes.len()).ok_or(72)?)
            .ok_or(72)?
            .copy_from_slice(bytes);
        Ok(())
    }
    fn discard_flushed(&mut self) {
        if self.streaming {
            self.base = self.len();
            self.bytes.clear();
        }
    }
}
fn append(output: &mut Output, bytes: &[u8], flags: u8) -> Result<ResourceHeader, ParseError> {
    let offset = output.len() as u64;
    output
        .try_reserve(bytes.len())
        .map_err(|_| ParseError::Nomem)?;
    output.extend_from_slice(bytes);
    Ok(ResourceHeader {
        size_in_wim: bytes.len() as u64,
        flags,
        offset_in_wim: offset,
        uncompressed_size: bytes.len() as u64,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn directory_reparse_with_ads_moves_only_rp_and_is_idempotent() {
        use crate::engine::capture::CapturePlan;
        use wim_format::metadata_write::{OwnedDentry, OwnedStream};
        let mut plan = CapturePlan::default();
        let mut node = OwnedDentry::new(Vec::new(), 0x410);
        node.main_hash = [1; 20];
        node.extra_streams.push(OwnedStream {
            name: vec![b'a', 0],
            hash: [2; 20],
            ..Default::default()
        });
        plan.tree.nodes.push(node);
        for _ in 0..2 {
            normalize_capture_reparse_streams(&mut plan).unwrap();
            let node = &plan.tree.nodes[0];
            assert_eq!(node.main_hash, [0; 20]);
            assert_eq!(node.extra_streams.len(), 2);
            assert!(node.extra_streams[0].name.is_empty());
            assert_eq!(node.extra_streams[0].hash, [1; 20]);
            assert_eq!(node.extra_streams[1].hash, [2; 20]);
            assert_eq!(node.extra_streams[1].name, vec![b'a', 0]);
        }
    }

    #[test]
    fn deferred_directory_reparse_with_ads_remaps_bindings_once() {
        use crate::engine::capture::{
            CaptureBinding, CaptureIdentity, CapturePlan, CapturedSource, CapturedStream,
        };
        use wim_format::metadata_write::{OwnedDentry, OwnedStream};
        let mut plan = CapturePlan::default();
        let mut node = OwnedDentry::new(Vec::new(), 0x410);
        node.extra_streams.push(OwnedStream {
            name: vec![b'a', 0],
            ..Default::default()
        });
        plan.tree.nodes.push(node);
        for slot in [0, 1] {
            plan.bindings.push(CaptureBinding {
                node: 0,
                slot,
                stream: Arc::new(CapturedStream {
                    size: 1,
                    identity: CaptureIdentity {
                        session: 1,
                        device: 2,
                        inode: 3,
                    },
                    source: CapturedSource::Inline(vec![slot as u8]),
                }),
            });
        }
        for _ in 0..2 {
            normalize_capture_reparse_streams(&mut plan).unwrap();
            assert_eq!(plan.tree.nodes[0].extra_streams.len(), 2);
            assert_eq!(
                plan.tree.nodes[0]
                    .extra_streams
                    .iter()
                    .filter(|s| s.name.is_empty())
                    .count(),
                1
            );
            assert_eq!(
                plan.bindings.iter().map(|b| b.slot).collect::<Vec<_>>(),
                [1, 2]
            );
        }
    }

    #[test]
    fn directory_reparse_without_ads_keeps_main_hash_and_no_data() {
        use crate::engine::capture::CapturePlan;
        use wim_format::metadata_write::OwnedDentry;
        let mut plan = CapturePlan::default();
        let mut node = OwnedDentry::new(Vec::new(), 0x410);
        node.main_hash = [1; 20];
        plan.tree.nodes.push(node);
        normalize_capture_reparse_streams(&mut plan).unwrap();
        assert_eq!(plan.tree.nodes[0].main_hash, [1; 20]);
        assert!(plan.tree.nodes[0].extra_streams.is_empty());
    }

    #[test]
    fn directory_reparse_with_only_unnamed_data_is_rejected() {
        use crate::engine::capture::CapturePlan;
        use wim_format::metadata_write::{OwnedDentry, OwnedStream};
        let mut plan = CapturePlan::default();
        let mut node = OwnedDentry::new(Vec::new(), 0x410);
        node.main_hash = [1; 20];
        node.extra_streams.push(OwnedStream::default());
        plan.tree.nodes.push(node);
        assert_eq!(normalize_capture_reparse_streams(&mut plan), Err(68));
    }

    #[test]
    fn directory_reparse_with_named_and_unnamed_data_is_rejected() {
        use crate::engine::capture::CapturePlan;
        use wim_format::metadata_write::{OwnedDentry, OwnedStream};
        let mut plan = CapturePlan::default();
        let mut node = OwnedDentry::new(Vec::new(), 0x410);
        node.main_hash = [1; 20];
        node.extra_streams = vec![
            OwnedStream::default(),
            OwnedStream {
                name: vec![b'a', 0],
                ..Default::default()
            },
        ];
        plan.tree.nodes.push(node);
        assert_eq!(normalize_capture_reparse_streams(&mut plan), Err(68));
    }

    #[test]
    fn reparse_normalization_keeps_nonempty_data_and_remaps_named_bindings() {
        use crate::engine::capture::{
            CaptureBinding, CaptureIdentity, CapturePlan, CapturedSource, CapturedStream,
        };
        use wim_format::metadata_write::{OwnedDentry, OwnedStream};
        let mut plan = CapturePlan::default();
        let mut node = OwnedDentry::new(Vec::new(), 0x400);
        node.main_hash = [1; 20];
        node.extra_streams = vec![
            OwnedStream {
                name: vec![b'a', 0],
                hash: [2; 20],
                ..Default::default()
            },
            OwnedStream {
                hash: [3; 20],
                ..Default::default()
            },
            OwnedStream {
                name: vec![b'b', 0],
                hash: [4; 20],
                ..Default::default()
            },
        ];
        plan.tree.nodes.push(node);
        for slot in 0..4 {
            plan.bindings.push(CaptureBinding {
                node: 0,
                slot,
                stream: Arc::new(CapturedStream {
                    size: slot as u64,
                    identity: CaptureIdentity {
                        session: 1,
                        device: 2,
                        inode: 3,
                    },
                    source: CapturedSource::Inline(vec![]),
                }),
            });
        }
        normalize_capture_reparse_streams(&mut plan).unwrap();
        assert_eq!(plan.tree.nodes[0].main_hash, [0; 20]);
        assert_eq!(
            plan.tree.nodes[0]
                .extra_streams
                .iter()
                .map(|s| s.hash[0])
                .collect::<Vec<_>>(),
            [1, 3, 2, 4]
        );
        assert_eq!(
            plan.bindings.iter().map(|b| b.slot).collect::<Vec<_>>(),
            [1, 3, 2, 4]
        );
        normalize_capture_reparse_streams(&mut plan).unwrap();
        assert_eq!(plan.tree.nodes[0].extra_streams.len(), 4);
        assert_eq!(
            plan.bindings.iter().map(|b| b.slot).collect::<Vec<_>>(),
            [1, 3, 2, 4]
        );
    }

    #[test]
    fn deferred_reparse_with_empty_data_and_ads_is_normalized_once() {
        use crate::engine::capture::{
            CaptureBinding, CaptureIdentity, CapturePlan, CapturedSource, CapturedStream,
        };
        use wim_format::metadata_write::{OwnedDentry, OwnedStream};
        let mut plan = CapturePlan::default();
        let mut node = OwnedDentry::new(Vec::new(), 0x420);
        node.extra_streams = vec![
            OwnedStream::default(),
            OwnedStream {
                name: vec![b'a', 0],
                ..Default::default()
            },
        ];
        plan.tree.nodes.push(node);
        for slot in [0, 2] {
            plan.bindings.push(CaptureBinding {
                node: 0,
                slot,
                stream: Arc::new(CapturedStream {
                    size: 1,
                    identity: CaptureIdentity {
                        session: 1,
                        device: 2,
                        inode: 3,
                    },
                    source: CapturedSource::Inline(vec![slot as u8]),
                }),
            });
        }
        normalize_capture_reparse_streams(&mut plan).unwrap();
        assert_eq!(plan.tree.nodes[0].main_hash, [0; 20]);
        assert_eq!(plan.tree.nodes[0].extra_streams.len(), 3);
        assert!(plan.tree.nodes[0].extra_streams[0].name.is_empty());
        assert!(plan.tree.nodes[0].extra_streams[1].name.is_empty());
        assert_eq!(plan.tree.nodes[0].extra_streams[2].name, vec![b'a', 0]);
        assert_eq!(
            plan.bindings.iter().map(|b| b.slot).collect::<Vec<_>>(),
            [1, 3]
        );
        normalize_capture_reparse_streams(&mut plan).unwrap();
        assert_eq!(plan.tree.nodes[0].extra_streams.len(), 3);
        assert_eq!(
            plan.bindings.iter().map(|b| b.slot).collect::<Vec<_>>(),
            [1, 3]
        );
    }

    #[test]
    fn append_buffers_only_new_bytes_at_large_absolute_offsets() {
        let base = 3_303_906_296usize;
        let mut output = Output::appended([0; HEADER_SIZE], base);
        let resource = append(&mut output, b"new resource", 0).unwrap();
        assert_eq!(resource.offset_in_wim, base as u64);
        assert_eq!(output.bytes, b"new resource");
        assert_eq!(output.len(), base + 12);
        output.patch(base, b"NEW").unwrap();
        assert_eq!(output.bytes, b"NEW resource");
        assert_eq!(output.patch(base - 1, b"x"), Err(72));
    }

    #[test]
    fn reused_compressor_matches_fresh_blocks_after_growth_and_codec_switches() {
        let mut cache = CompressorCache::default();
        for (kind, length, chunk) in [
            (Compression::Xpress, 4096, 32768),
            (Compression::Xpress, 32768, 32768),
            (Compression::Xpress, 2048, 32768),
            (Compression::Lzms, 8192, 32768),
            (Compression::Lzms, 32768, 32768),
            (Compression::Lzms, 1024, 32768),
            (Compression::Lzx, 32768, 32768),
            (Compression::Lzx, 8192, 65536),
            (Compression::Xpress, 128, 32768),
        ] {
            let bytes: Vec<u8> = (0..length).map(|index| (index % 131) as u8).collect();
            let expected = match kind {
                Compression::Xpress => {
                    ms_compress::xpress_encode::compress_xpress(&bytes, length).unwrap()
                }
                Compression::Lzx => {
                    ms_compress::lzx_encode::compress_lzx(&bytes, length, chunk as usize).unwrap()
                }
                Compression::Lzms => {
                    ms_compress::lzms::encode::compress_lzms(&bytes, length).unwrap()
                }
                Compression::None => None,
            };
            assert_eq!(
                cache.compress(kind, &bytes, chunk).unwrap(),
                expected,
                "{kind:?}, {length}, {chunk}"
            );
        }
    }
}
