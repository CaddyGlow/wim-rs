//! Native image export with independently owned, lazily decoded source snapshots.

use crate::engine::collections::FallibleCollections as _;
use crate::engine::collections::FallibleMap as _;
use crate::engine::collections::FallibleSet as _;
use crate::engine::{
    TChar,
    handles::{
        HandleImage, OwnedBlob, OwnedImage, OwnedResource, WimHandle, handle_mut, image_identity,
        image_metadata_bytes, image_metadata_hash, pending_metadata,
    },
};
use std::collections::HashMap;
use std::ffi::c_int;
use std::sync::Arc;
use wim_format::{ParseError, metadata::Metadata};

unsafe fn text(pointer: *const TChar) -> Result<Option<Vec<u8>>, ParseError> {
    if pointer.is_null() {
        return Ok(None);
    }
    #[cfg(not(windows))]
    {
        // SAFETY: Caller supplies readable NUL-terminated platform text.
        Ok(Some(
            unsafe { std::ffi::CStr::from_ptr(pointer) }
                .to_bytes()
                .to_vec(),
        ))
    }
    #[cfg(windows)]
    {
        let mut length = 0;
        // SAFETY: Caller supplies readable NUL-terminated platform text.
        while unsafe { *pointer.add(length) } != 0 {
            length += 1;
        }
        // SAFETY: Scan identified the readable UTF-16 slice.
        String::from_utf16(unsafe { std::slice::from_raw_parts(pointer, length) })
            .map(|s| Some(s.into_bytes()))
            .map_err(|_| ParseError::InvalidUtf16String)
    }
}
fn has_metadata(handle: &WimHandle) -> bool {
    handle.header.image_count == 0 || handle.images.len() == handle.header.image_count as usize
}
fn copy_bytes(bytes: &[u8]) -> Result<Vec<u8>, ParseError> {
    let mut copy = Vec::new();
    copy.try_reserve_exact(bytes.len())
        .map_err(|_| ParseError::Nomem)?;
    copy.extend_from_slice(bytes);
    Ok(copy)
}
fn metadata_descriptor(
    handle: &WimHandle,
    image: &HandleImage,
) -> Result<Option<OwnedResource>, ParseError> {
    match image {
        HandleImage::Source(index) => {
            let lookup = handle.lookup.as_ref().ok_or(ParseError::MetadataNotFound)?;
            let blob = lookup
                .metadata
                .get((*index - 1) as usize)
                .ok_or(ParseError::MetadataNotFound)?
                .clone();
            let resource = lookup
                .resources
                .get(blob.resource_index)
                .ok_or(ParseError::InvalidLookupTableEntry)?
                .clone();
            Ok(Some(OwnedResource {
                blob,
                resource,
                part: handle.header.part_number,
            }))
        }
        HandleImage::Empty(_) => Ok(None),
        HandleImage::Owned(image) => Ok(image.descriptor.clone()),
    }
}
fn contains_blob(handle: &WimHandle, hash: &[u8; 20]) -> bool {
    !handle.removed_blobs.contains(hash)
        && (handle.owned_blobs.contains_key(hash)
            || handle
                .lookup
                .as_ref()
                .is_some_and(|lookup| lookup.find(hash).is_some()))
}
fn clone_blob(
    handle: &WimHandle,
    hash: &[u8; 20],
    snapshot: &mut Option<Arc<crate::engine::backing::Backing>>,
) -> Result<OwnedBlob, ParseError> {
    if handle.removed_blobs.contains(hash) {
        return Err(ParseError::ResourceNotFound);
    }
    if let Some(blob) = handle.owned_blobs.get(hash) {
        let mut descriptor = blob.descriptor.clone();
        descriptor.blob.reference_count = 0;
        return Ok(OwnedBlob {
            bytes: blob.bytes.try_clone()?,
            backing: blob.backing.clone(),
            captured: blob.captured.clone(),
            descriptor,
        });
    }
    let lookup = handle.lookup.as_ref().ok_or(ParseError::ResourceNotFound)?;
    let mut blob = lookup
        .find(hash)
        .ok_or(ParseError::ResourceNotFound)?
        .clone();
    let resource = lookup
        .resources
        .get(blob.resource_index)
        .ok_or(ParseError::InvalidLookupTableEntry)?
        .clone();
    if snapshot.is_none() {
        *snapshot = Some(
            handle
                .backing
                .as_ref()
                .ok_or(ParseError::ResourceNotFound)?
                .clone(),
        );
    }
    blob.reference_count = 0;
    Ok(OwnedBlob {
        bytes: crate::engine::handles::OwnedBlobData::Decoded(Vec::new()),
        backing: snapshot.clone(),
        captured: None,
        descriptor: OwnedResource {
            blob,
            resource,
            part: handle.header.part_number,
        },
    })
}
fn export_images(
    source: &mut WimHandle,
    image: c_int,
    destination: &mut WimHandle,
    name: Option<&[u8]>,
    description: Option<&[u8]>,
    flags: c_int,
) -> Result<(), ParseError> {
    if !has_metadata(source) || !has_metadata(destination) {
        return Err(ParseError::MetadataNotFound);
    }
    let all = image == -1;
    if all && ((flags & 2 == 0 && name.is_some()) || (flags & 4 == 0 && description.is_some())) {
        return Err(ParseError::InvalidParam);
    }
    let mut indices = Vec::new();
    if all {
        indices
            .try_reserve_exact(source.images.len())
            .map_err(|_| ParseError::Nomem)?;
        indices.extend(1..=source.header.image_count);
    } else {
        if image < 1 || image as u32 > source.header.image_count {
            return Err(ParseError::InvalidImage);
        }
        indices.push(image as u32);
    }
    for index in &indices {
        let identity = image_identity(source, &source.images[(*index - 1) as usize]);
        if destination
            .images
            .iter()
            .any(|image| image_identity(destination, image) == identity)
        {
            return Err(ParseError::DuplicateExportedImage);
        }
    }
    #[cfg(unix)]
    {
        crate::engine::capture::checksum_pending(source)?;
        crate::engine::capture::checksum_pending(destination)?;
    }
    if flags & 2 == 0 {
        for index in &indices {
            let name = name.or_else(|| source.xml.name_bytes(*index as i32));
            if name.is_some_and(|name| destination.xml.name_in_use_bytes(name)) {
                return Err(ParseError::ImageNameCollision);
            }
        }
    }
    if destination
        .images
        .len()
        .checked_add(indices.len())
        .is_none_or(|count| count > 65535)
    {
        return Err(ParseError::ImageCount);
    }
    let mut staged_xml = destination
        .xml
        .select_images(&(1..=destination.header.image_count).collect::<Vec<_>>())?;
    let mut staged_images = Vec::new();
    staged_images
        .try_reserve_exact(indices.len())
        .map_err(|_| ParseError::Nomem)?;
    let mut increments = HashMap::<[u8; 20], u32>::new();
    let mut new_blobs = HashMap::new();
    let mut publication_order = Vec::new();
    let mut snapshot = None;
    let captured = crate::engine::lookup::captured_resources(source)?;
    for index in &indices {
        let current = &source.images[(*index - 1) as usize];
        let descriptor = metadata_descriptor(source, current)?;
        let identity = image_identity(source, current);
        let hash = image_metadata_hash(source, current)?;
        let pending = pending_metadata(current).cloned();
        let metadata_bytes = copy_bytes(&image_metadata_bytes(source, (*index - 1) as usize)?)?;
        let metadata = Metadata::parse(&metadata_bytes)?;
        for node in 0..metadata.nodes.len() {
            let entry = metadata
                .inode_entry(node)
                .ok_or(ParseError::InvalidMetadataResource)?;
            for stream in &entry.streams {
                if stream.hash == [0; 20] {
                    continue;
                }
                if !increments.contains_key(&stream.hash) {
                    increments.try_reserve(1).map_err(|_| ParseError::Nomem)?;
                }
                let increment = increments.entry(stream.hash).or_default();
                *increment = increment.wrapping_add(1);
                if destination.blob_index.owner(&stream.hash).is_none()
                    && !new_blobs.contains_key(&stream.hash)
                {
                    publication_order
                        .try_push(stream.hash)
                        .map_err(|_| ParseError::Nomem)?;
                    if !contains_blob(source, &stream.hash)
                        && captured.iter().any(|resource| resource.hash == stream.hash)
                    {
                        if flags & 8 != 0 {
                            source.blob_index.unlink(&stream.hash);
                        }
                        // Pending metadata retains the actual stream Arc, including
                        // its file path or inline payload, after the source is freed.
                        continue;
                    }
                    new_blobs.try_reserve(1).map_err(|_| ParseError::Nomem)?;
                    let blob = clone_blob(source, &stream.hash, &mut snapshot)?;
                    if flags & 8 != 0 {
                        source
                            .removed_blobs
                            .try_reserve(1)
                            .map_err(|_| ParseError::Nomem)?;
                        // Original GIFT unlinks source blobs immediately and does
                        // not restore them when a later XML/export step fails.
                        source
                            .removed_blobs
                            .try_insert(stream.hash)
                            .map_err(|_| ParseError::Nomem)?;
                        source.blob_index.unlink(&stream.hash);
                        source.owned_blobs.remove(&stream.hash);
                    }
                    new_blobs.insert(stream.hash, blob);
                }
            }
        }
        drop(metadata);
        staged_images.push(HandleImage::Owned(OwnedImage {
            metadata: metadata_bytes,
            hash,
            pending,
            descriptor,
            identity,
        }));
        let mut xml = source.xml.select_images(&[*index])?;
        let next_name = if flags & 2 != 0 {
            None
        } else {
            name.or_else(|| source.xml.name_bytes(*index as i32))
        };
        let next_description = if flags & 4 != 0 {
            None
        } else {
            description.or_else(|| source.xml.description_bytes(*index as i32))
        };
        xml.set_property_bytes(1, b"NAME", next_name)?;
        xml.set_property_bytes(1, b"DESCRIPTION", next_description)?;
        if flags & 16 != 0 {
            xml.set_property_bytes(1, b"WIMBOOT", Some(b"1"))?;
        }
        staged_xml.append_images(&xml, &[1])?;
    }
    let staged_xml = crate::engine::handles::own_xml(staged_xml)?;
    destination
        .images
        .try_reserve(staged_images.len())
        .map_err(|_| ParseError::Nomem)?;
    destination
        .image_owners
        .try_reserve(staged_images.len())
        .map_err(|_| ParseError::Nomem)?;
    destination
        .dirty_images
        .try_reserve(indices.len())
        .map_err(|_| ParseError::Nomem)?;
    destination
        .owned_blobs
        .try_reserve(new_blobs.len())
        .map_err(|_| ParseError::Nomem)?;
    if flags & 8 != 0 {
        source
            .removed_blobs
            .try_reserve(new_blobs.len())
            .map_err(|_| ParseError::Nomem)?;
    }
    let staged_index = destination.blob_index.try_clone()?;
    for hash in &publication_order {
        staged_index.insert(
            *hash,
            if new_blobs.contains_key(hash) {
                crate::engine::blob_index::BlobOwner::Owned
            } else {
                crate::engine::blob_index::BlobOwner::Captured
            },
        )?;
    }
    for &hash in &publication_order {
        let Some(mut blob) = new_blobs.remove(&hash) else {
            continue;
        };
        blob.descriptor.blob.reference_count = increments.remove(&hash).unwrap_or(0);
        if flags & 8 != 0 {
            source
                .removed_blobs
                .try_insert(hash)
                .map_err(|_| ParseError::Nomem)?;
            source.blob_index.unlink(&hash);
            source.owned_blobs.remove(&hash);
        }
        destination.removed_blobs.remove(&hash);
        destination
            .owned_blobs
            .try_insert_reserved(hash, blob)
            .map_err(|_| ParseError::Nomem)?;
    }
    for (hash, count) in increments {
        if let Some(blob) = destination.owned_blobs.get_mut(&hash) {
            blob.descriptor.blob.reference_count =
                blob.descriptor.blob.reference_count.wrapping_add(count);
        } else if let Some(lookup) = destination.lookup.as_mut()
            && let Some(blob) = lookup.blobs.iter_mut().find(|blob| blob.hash == hash)
        {
            blob.reference_count = blob.reference_count.wrapping_add(count);
        }
    }
    let original_count = destination.header.image_count;
    destination.xml = staged_xml;
    for index in &indices {
        let identity = image_identity(source, &source.images[*index as usize - 1]);
        if source.dirty_images.contains(&identity) {
            destination
                .dirty_images
                .try_insert(identity)
                .map_err(|_| ParseError::Nomem)?;
        }
    }
    let owners = indices
        .iter()
        .map(|index| source.image_owners[*index as usize - 1].clone());
    destination
        .image_owners
        .try_extend(owners)
        .map_err(|_| ParseError::Nomem)?;
    destination
        .images
        .try_extend(staged_images)
        .map_err(|_| ParseError::Nomem)?;
    destination.blob_index = staged_index;
    destination.header.image_count = destination.images.len() as u32;
    if source.header.flags & 0x80 != 0 {
        destination.header.flags |= 0x80;
    }
    if flags & 1 != 0 {
        for (position, index) in indices.iter().enumerate() {
            if !all || *index == source.header.boot_index {
                destination.header.boot_index = original_count + position as u32 + 1;
            }
        }
    }
    Ok(())
}

/// Export images between typed archive handles with optional UTF-8 properties.
pub(crate) fn export_archive(
    source: &mut WimHandle,
    image: c_int,
    destination: &mut WimHandle,
    name: Option<&str>,
    description: Option<&str>,
    flags: c_int,
) -> Result<(), ParseError> {
    export_archive_bytes(
        source,
        image,
        destination,
        name.map(str::as_bytes),
        description.map(str::as_bytes),
        flags,
    )
}

fn export_archive_bytes(
    source: &mut WimHandle,
    image: c_int,
    destination: &mut WimHandle,
    name: Option<&[u8]>,
    description: Option<&[u8]>,
    flags: c_int,
) -> Result<(), ParseError> {
    if flags & !31 != 0 {
        return Err(ParseError::InvalidParam);
    }
    export_images(
        source,
        image,
        destination,
        if flags & 2 != 0 { None } else { name },
        if flags & 4 != 0 { None } else { description },
        flags,
    )
}

/// Export one image or all images (-1) into an independently owned destination.
/// File payload decoding remains deferred until verification or writing.
///
/// # Safety
/// Nonnull handles must be live and exclusively accessed. Optional text pointers
/// must be readable NUL-terminated platform strings. Source and destination may
/// alias; duplicate-image validation rejects exports into the same handle.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn wimlib_export_image(
    source: *mut WimHandle,
    image: c_int,
    destination: *mut WimHandle,
    name: *const TChar,
    description: *const TChar,
    flags: c_int,
) -> c_int {
    if flags & !31 != 0 || source.is_null() || destination.is_null() {
        return 24;
    }
    if source == destination {
        // SAFETY: Nonnull handle validity is a caller requirement.
        let handle = unsafe { &*source };
        if !has_metadata(handle) {
            return 36;
        }
        if image == -1
            && ((flags & 2 == 0 && !name.is_null()) || (flags & 4 == 0 && !description.is_null()))
        {
            return 24;
        }
        if image == -1 && handle.images.is_empty() {
            return 0;
        }
        if image != -1 && (image < 1 || image as u32 > handle.header.image_count) {
            return 18;
        }
        return 87;
    }
    // SAFETY: Nonaliasing live handles are exclusively accessible by the caller.
    let source = unsafe { handle_mut(source) };
    // SAFETY: As above, source/destination pointers were checked distinct.
    let destination = unsafe { handle_mut(destination) };
    let (Some(source), Some(destination)) = (source, destination) else {
        return 24;
    };
    // Ignored text pointers need not be read when their suppressing flags are set.
    // SAFETY: Caller guarantees readable optional platform text.
    let name = if flags & 2 != 0 {
        None
    } else {
        match unsafe { text(name) } {
            Ok(v) => v,
            Err(e) => return e as c_int,
        }
    };
    // SAFETY: Same text requirements as above.
    let description = if flags & 4 != 0 {
        None
    } else {
        match unsafe { text(description) } {
            Ok(v) => v,
            Err(e) => return e as c_int,
        }
    };
    match export_archive_bytes(
        source,
        image,
        destination,
        name.as_deref(),
        description.as_deref(),
        flags,
    ) {
        Ok(()) => 0,
        Err(error) => error as c_int,
    }
}
