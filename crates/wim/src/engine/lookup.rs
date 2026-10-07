// SPDX-License-Identifier: LGPL-2.1-or-later
//! Resource lookup callbacks translated from upstream blob_table.c.

use crate::engine::blob_index::BlobOwner;
use crate::engine::collections::FallibleSet as _;
use crate::engine::handles::{HandleImage, WimHandle, image_metadata_hash};
use std::ffi::{c_int, c_void};
use wim_format::ParseError;
use wim_format::lookup::{LookupBlob, LookupResource};

/// Original public resource structure, with the six bitfields in `flags`.
/// Only little-endian host C layout has been validated.
#[repr(C)]
#[derive(Debug, Default)]
pub struct WimResourceEntry {
    /// Blob length.
    pub uncompressed_size: u64,
    /// Ordinary resource stored length; zero for solid blobs.
    pub compressed_size: u64,
    /// Ordinary file offset or uncompressed solid offset.
    pub offset: u64,
    /// Blob content digest.
    pub sha1_hash: [u8; 20],
    /// Backing part number.
    pub part_number: u32,
    /// Recorded reference count.
    pub reference_count: u32,
    /// Compressed, metadata, free, spanned, missing and packed bits, respectively.
    pub flags: u32,
    /// Backing resource file offset, also populated for ordinary resources.
    pub raw_resource_offset_in_wim: u64,
    /// Backing resource stored length.
    pub raw_resource_compressed_size: u64,
    /// Backing resource expanded length.
    pub raw_resource_uncompressed_size: u64,
    /// Reserved zero field.
    pub reserved: [u64; 1],
}

pub(crate) fn resource_entry(
    blob: &LookupBlob,
    resource: &LookupResource,
    part: u16,
) -> WimResourceEntry {
    let raw = resource.header;
    WimResourceEntry {
        uncompressed_size: blob.size,
        compressed_size: if resource.solid { 0 } else { raw.size_in_wim },
        offset: if resource.solid {
            blob.offset
        } else {
            raw.offset_in_wim
        },
        sha1_hash: blob.hash,
        part_number: part.into(),
        reference_count: blob.reference_count,
        flags: u32::from(raw.flags & 4 != 0)
            | (u32::from(blob.flags & 2 != 0) << 1)
            | (u32::from(raw.flags & 1 != 0) << 2)
            | (u32::from(raw.flags & 8 != 0) << 3)
            | (u32::from(resource.solid) << 5),
        raw_resource_offset_in_wim: raw.offset_in_wim,
        raw_resource_compressed_size: raw.size_in_wim,
        raw_resource_uncompressed_size: resource.uncompressed_size,
        reserved: [0],
    }
}

pub(crate) struct CapturedResource {
    pub hash: [u8; 20],
    pub stream: std::sync::Arc<crate::engine::capture::CapturedStream>,
    pub references: u32,
    pub image_index: usize,
}
impl CapturedResource {
    pub fn entry(&self) -> WimResourceEntry {
        WimResourceEntry {
            uncompressed_size: self.stream.size,
            sha1_hash: self.hash,
            reference_count: self.references,
            ..WimResourceEntry::default()
        }
    }
}
/// Prepare actual captured descriptor insertion and retirement for a graph replacement.
pub(crate) fn prepare_capture_index(
    handle: &WimHandle,
    replaced: usize,
    replacement: Option<&crate::engine::capture::CapturePlan>,
) -> Result<crate::engine::blob_index::BlobIndex, ParseError> {
    let index = handle.blob_index.try_clone()?;
    let mut live = hashbrown::HashSet::new();
    for (image_index, image) in handle.images.iter().enumerate() {
        let retained;
        let plan = if image_index == replaced {
            replacement
        } else {
            retained = crate::engine::handles::image_capture_plan(image)?;
            retained.as_ref()
        };
        let Some(plan) = plan else {
            continue;
        };
        for binding in &plan.bindings {
            let node = plan
                .tree
                .nodes
                .get(binding.node)
                .ok_or(ParseError::InvalidMetadataResource)?;
            let hash = if binding.slot == 0 {
                node.main_hash
            } else {
                node.extra_streams
                    .get(binding.slot - 1)
                    .ok_or(ParseError::InvalidMetadataResource)?
                    .hash
            };
            if hash != [0; 20] {
                live.try_insert(hash).map_err(|_| ParseError::Nomem)?;
                if image_index == replaced {
                    index.insert(hash, BlobOwner::Captured)?;
                }
            }
        }
    }
    let mut cursor = crate::engine::blob_index::BlobCursor::default();
    while let Some((hash, owner)) = index.next(&mut cursor) {
        if owner == BlobOwner::Captured && !live.contains(&hash) {
            if handle.owned_blobs.contains_key(&hash) {
                index.set_owner(&hash, BlobOwner::Owned);
            } else if handle
                .lookup
                .as_ref()
                .is_some_and(|table| table.find(&hash).is_some())
            {
                index.set_owner(&hash, BlobOwner::Stored);
            } else {
                index.unlink(&hash);
            }
        }
    }
    Ok(index)
}
pub(crate) fn captured_resources(handle: &WimHandle) -> Result<Vec<CapturedResource>, ParseError> {
    let mut resources: Vec<CapturedResource> = Vec::new();
    let mut positions = std::collections::HashMap::new();
    for (image_index, image) in handle.images.iter().enumerate() {
        let Some(plan) = crate::engine::handles::image_capture_plan(image)? else {
            continue;
        };
        for binding in plan.bindings {
            let node = plan
                .tree
                .nodes
                .get(binding.node)
                .ok_or(ParseError::InvalidMetadataResource)?;
            let hash = if binding.slot == 0 {
                node.main_hash
            } else {
                node.extra_streams
                    .get(binding.slot - 1)
                    .ok_or(ParseError::InvalidMetadataResource)?
                    .hash
            };
            // Unhashed streams merge only by shared ownership; hashed streams
            // merge by digest. Preserve insertion order without a full scan per binding.
            let key = (
                hash,
                if hash == [0; 20] {
                    std::sync::Arc::as_ptr(&binding.stream) as usize
                } else {
                    0
                },
            );
            if let Some(&index) = positions.get(&key) {
                let existing: &mut CapturedResource = &mut resources[index];
                if hash != [0; 20]
                    && let (
                        crate::engine::capture::CapturedSource::File(current),
                        crate::engine::capture::CapturedSource::File(candidate),
                    ) = (&existing.stream.source, &binding.stream.source)
                    && candidate < current
                {
                    existing.stream = binding.stream.clone();
                }
                existing.references = existing
                    .references
                    .checked_add(1)
                    .ok_or(ParseError::InvalidMetadataResource)?;
            } else {
                resources.try_reserve(1).map_err(|_| ParseError::Nomem)?;
                positions.try_reserve(1).map_err(|_| ParseError::Nomem)?;
                positions.insert(key, resources.len());
                resources.push(CapturedResource {
                    hash,
                    stream: binding.stream,
                    references: 1,
                    image_index,
                });
            }
        }
    }
    Ok(resources)
}

/// Callback for one borrowed resource entry; a nonzero return stops traversal.
pub type LookupCallback = unsafe extern "C" fn(*const WimResourceEntry, *mut c_void) -> c_int;

fn metadata_entry(handle: &WimHandle, index: usize) -> Result<WimResourceEntry, ParseError> {
    let image = handle
        .images
        .get(index)
        .ok_or(ParseError::MetadataNotFound)?;
    Ok(match image {
        HandleImage::Source(index) => {
            let lookup = handle.lookup.as_ref().ok_or(ParseError::MetadataNotFound)?;
            let blob = lookup
                .metadata
                .get((*index - 1) as usize)
                .ok_or(ParseError::MetadataNotFound)?;
            resource_entry(
                blob,
                &lookup.resources[blob.resource_index],
                handle.header.part_number,
            )
        }
        HandleImage::Owned(owned) if owned.descriptor.is_some() => {
            let descriptor = owned.descriptor.as_ref().expect("matched descriptor");
            resource_entry(&descriptor.blob, &descriptor.resource, descriptor.part)
        }
        HandleImage::Empty(_) | HandleImage::Owned(_) => WimResourceEntry {
            sha1_hash: image_metadata_hash(handle, image)?,
            reference_count: 1,
            flags: 2,
            ..WimResourceEntry::default()
        },
    })
}
fn hashed_entry(
    handle: &WimHandle,
    hash: &[u8; 20],
    owner: crate::engine::blob_index::BlobOwner,
    captured: &[CapturedResource],
) -> Result<Option<WimResourceEntry>, ParseError> {
    if handle.removed_blobs.contains(hash) {
        return Ok(None);
    }
    Ok(Some(match owner {
        BlobOwner::Stored => {
            let table = handle.lookup.as_ref().ok_or(ParseError::ResourceNotFound)?;
            let blob = table.find(hash).ok_or(ParseError::ResourceNotFound)?;
            resource_entry(
                blob,
                &table.resources[blob.resource_index],
                handle.header.part_number,
            )
        }
        BlobOwner::Owned => {
            let blob = handle
                .owned_blobs
                .get(hash)
                .ok_or(ParseError::ResourceNotFound)?;
            if blob.captured.is_some() || blob.bytes.is_memory() {
                WimResourceEntry {
                    sha1_hash: *hash,
                    uncompressed_size: blob
                        .captured
                        .as_ref()
                        .map_or(blob.bytes.len() as u64, |stream| stream.size),
                    reference_count: blob.descriptor.blob.reference_count,
                    ..WimResourceEntry::default()
                }
            } else {
                let descriptor = &blob.descriptor;
                resource_entry(&descriptor.blob, &descriptor.resource, descriptor.part)
            }
        }
        BlobOwner::Captured => captured
            .iter()
            .find(|resource| resource.hash == *hash)
            .ok_or(ParseError::ResourceNotFound)?
            .entry(),
    }))
}
unsafe fn call(
    callback: Option<LookupCallback>,
    context: *mut c_void,
    entry: &WimResourceEntry,
) -> Result<(), c_int> {
    let callback = callback.ok_or(24)?;
    // SAFETY: Entry is live for this call; context validity is caller-owned.
    let result = unsafe { callback(entry, context) };
    if result == 0 { Ok(()) } else { Err(result) }
}

/// Emit each image's metadata and unhashed streams, then the retained hashed
/// bucket chains. A nonzero callback status stops at that exact row.
///
/// # Safety
/// `handle` must remain live throughout traversal, and callback/context must
/// accept each entry borrowed for the duration of its call. Mutating resource
/// ownership during traversal invalidates the cursor as in the original API.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn wimlib_iterate_lookup_table(
    handle: *mut WimHandle,
    flags: c_int,
    callback: Option<LookupCallback>,
    context: *mut c_void,
) -> c_int {
    if flags != 0 {
        return 24;
    }
    // SAFETY: Caller provides a live handle when nonnull.
    let images = match unsafe { handle.as_ref() } {
        Some(h) => h.images.len(),
        None => return 24,
    };
    let mut captured = None;
    for index in 0..images {
        let entry = {
            // SAFETY: Caller retains the handle through every callback.
            let h = unsafe { &*handle };
            match metadata_entry(h, index) {
                Ok(entry) => entry,
                Err(error) => return error as c_int,
            }
        };
        // SAFETY: No handle borrow or graph lock is held over this callback.
        if let Err(result) = unsafe { call(callback, context, &entry) } {
            return result;
        }
        if captured.is_none() {
            // SAFETY: The callback retained the live handle and its resource graph.
            captured = Some(match captured_resources(unsafe { &*handle }) {
                Ok(resources) => resources,
                Err(error) => return error as c_int,
            });
        }
        for resource in captured
            .as_ref()
            .expect("initialized resources")
            .iter()
            .filter(|resource| resource.image_index == index && resource.hash == [0; 20])
        {
            let entry = resource.entry();
            // SAFETY: Borrowed entry/context are valid; only snapshot Arcs live here.
            if let Err(result) = unsafe { call(callback, context, &entry) } {
                return result;
            }
        }
    }
    // SAFETY: Caller preserves the live handle; this scope ends before callbacks.
    if unsafe { &*handle }.blob_index.is_empty() {
        return 0;
    }
    let captured = match captured {
        Some(resources) => resources,
        // SAFETY: The handle remains live and no borrowed graph escapes this call.
        None => match captured_resources(unsafe { &*handle }) {
            Ok(resources) => resources,
            Err(error) => return error as c_int,
        },
    };
    let mut cursor = crate::engine::blob_index::BlobCursor::default();
    loop {
        let entry = {
            // SAFETY: Reborrow only to copy this row and cursor; release before C.
            let h = unsafe { &*handle };
            let Some((hash, owner)) = h.blob_index.next(&mut cursor) else {
                break;
            };
            match hashed_entry(h, &hash, owner, &captured) {
                Ok(Some(entry)) => entry,
                Ok(None) => continue,
                Err(error) => return error as c_int,
            }
        };
        // SAFETY: Stack entry/context remain live and no handle/RefCell/graph guard survives.
        if let Err(result) = unsafe { call(callback, context, &entry) } {
            return result;
        }
    }
    0
}
