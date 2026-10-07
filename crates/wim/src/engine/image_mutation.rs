//! Native image addition and deletion, with owned pending metadata.

use crate::engine::collections::FallibleCollections as _;
use crate::engine::{
    TChar,
    handles::{
        EmptyImage, HandleImage, PendingMetadata, WimHandle, handle_mut, image_metadata_bytes,
        new_identity,
    },
};
use std::ffi::c_int;
use wim_format::{ParseError, metadata::Metadata, xml::XmlInfo};

unsafe fn name_bytes(pointer: *const TChar) -> Result<Option<Vec<u8>>, c_int> {
    if pointer.is_null() {
        return Ok(None);
    }
    #[cfg(not(windows))]
    {
        // SAFETY: Caller supplies a readable NUL-terminated platform string.
        Ok(Some(
            unsafe { std::ffi::CStr::from_ptr(pointer) }
                .to_bytes()
                .to_vec(),
        ))
    }
    #[cfg(windows)]
    {
        let mut length = 0;
        // SAFETY: Caller supplies a readable NUL-terminated platform string.
        while unsafe { *pointer.add(length) } != 0 {
            length += 1;
        }
        // SAFETY: The scan identified the readable UTF-16 slice.
        String::from_utf16(unsafe { std::slice::from_raw_parts(pointer, length) })
            .map(|s| Some(s.into_bytes()))
            .map_err(|_| 30)
    }
}

fn has_metadata(handle: &WimHandle) -> bool {
    handle.header.image_count == 0 || handle.images.len() == handle.header.image_count as usize
}

pub(crate) fn add_empty(handle: &mut WimHandle, name: Option<&[u8]>) -> Result<u32, ParseError> {
    if name.is_some_and(|name| handle.xml.name_in_use_bytes(name)) {
        return Err(ParseError::ImageNameCollision);
    }
    if !has_metadata(handle) {
        return Err(ParseError::MetadataNotFound);
    }
    if handle.header.image_count == 65535 {
        return Err(ParseError::ImageCount);
    }
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_err(|_| ParseError::InvalidParam)?;
    let timestamp =
        (now.as_secs() + 11_644_473_600) * 10_000_000 + u64::from(now.subsec_nanos()) / 100;
    let mut appended = XmlInfo::parse(
        "<WIM><IMAGE INDEX=\"1\"><DIRCOUNT>0</DIRCOUNT><FILECOUNT>0</FILECOUNT><TOTALBYTES>0</TOTALBYTES><HARDLINKBYTES>0</HARDLINKBYTES></IMAGE></WIM>",
    )?;
    if name.is_some_and(|name| !name.is_empty()) {
        appended.set_property_bytes(1, b"NAME", name)?;
    }
    for path in ["CREATIONTIME", "LASTMODIFICATIONTIME"] {
        appended.set_property(
            1,
            &format!("{path}/HIGHPART"),
            Some(&format!("0x{:08X}", timestamp >> 32)),
        )?;
        appended.set_property(
            1,
            &format!("{path}/LOWPART"),
            Some(&format!("0x{:08X}", timestamp as u32)),
        )?;
    }
    let mut metadata = Vec::new();
    metadata
        .try_reserve_exact(16)
        .map_err(|_| ParseError::Nomem)?;
    metadata.extend_from_slice(&8u32.to_le_bytes());
    metadata.extend_from_slice(&0u32.to_le_bytes());
    metadata.extend_from_slice(&0u64.to_le_bytes());
    // Verify this is actual rootless metadata, not an XML-only image.
    Metadata::parse(&metadata)?;
    handle
        .images
        .try_reserve(1)
        .map_err(|_| ParseError::Nomem)?;
    handle
        .image_owners
        .try_reserve(1)
        .map_err(|_| ParseError::Nomem)?;
    let owner = crate::engine::handles::new_image_owner()?;
    let shared = crate::engine::handles::new_pending_metadata(PendingMetadata {
        capture: None,
        metadata,
        hash: [0; 20],
    })?;
    handle.xml.append_images(&appended, &[1])?;
    handle
        .image_owners
        .try_push(owner)
        .map_err(|_| ParseError::Nomem)?;
    handle
        .images
        .try_push(HandleImage::Empty(EmptyImage {
            identity: new_identity(),
            shared,
        }))
        .map_err(|_| ParseError::Nomem)?;
    handle.header.image_count += 1;
    Ok(handle.header.image_count)
}

/// Add a rootless image with owned metadata; return its new one-based index.
/// Name collisions and failures leave `new_index` unchanged.
///
/// # Safety
/// `handle` must be live and exclusively accessed. A nonnull `name` must be a
/// NUL-terminated platform string; nonnull `new_index` must be writable storage.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn wimlib_add_empty_image(
    handle: *mut WimHandle,
    name: *const TChar,
    new_index: *mut c_int,
) -> c_int {
    // SAFETY: Valid handle and platform string are caller requirements.
    let Some(handle) = (unsafe { handle_mut(handle) }) else {
        return 24;
    };
    // SAFETY: String validity is guaranteed by the caller.
    let name = match unsafe { name_bytes(name) } {
        Ok(name) => name,
        Err(error) => return error,
    };
    match add_empty(handle, name.as_deref()) {
        Ok(index) => {
            if !new_index.is_null() {
                // SAFETY: Caller guarantees writable optional index storage.
                unsafe {
                    new_index.write(index as c_int);
                }
            }
            0
        }
        Err(error) => error as c_int,
    }
}

fn delete_one(handle: &mut WimHandle, image: c_int) -> Result<(), ParseError> {
    if image < 1 || image as u32 > handle.header.image_count {
        return Err(ParseError::InvalidImage);
    }
    if !has_metadata(handle) {
        return Err(ParseError::MetadataNotFound);
    }
    let index = image as usize - 1;
    let mut decrements = Vec::new();
    {
        let metadata_bytes = image_metadata_bytes(handle, index)?;
        let metadata = Metadata::parse(&metadata_bytes)?;
        for node in 0..metadata.nodes.len() {
            let entry = metadata
                .inode_entry(node)
                .ok_or(ParseError::InvalidMetadataResource)?;
            for stream in &entry.streams {
                if stream.hash != [0; 20] {
                    decrements.try_reserve(1).map_err(|_| ParseError::Nomem)?;
                    decrements.push(stream.hash);
                }
            }
        }
    }
    let mut retained = Vec::new();
    retained
        .try_reserve_exact(handle.images.len().saturating_sub(1))
        .map_err(|_| ParseError::Nomem)?;
    retained.extend((1..=handle.header.image_count).filter(|&i| i != image as u32));
    let xml = crate::engine::handles::own_xml(handle.xml.select_images(&retained)?)?;
    // Prepare non-WIM descriptor retirement before releasing any image/stream
    // owner; published index entries must always retain their owners.
    let staged_index = crate::engine::lookup::prepare_capture_index(handle, index, None)?;
    let mut dead = Vec::new();
    for (hash, blob) in handle.owned_blobs.iter() {
        if (blob.bytes.is_memory() || blob.captured.is_some())
            && blob.descriptor.blob.reference_count
                <= decrements.iter().filter(|removed| *removed == hash).count() as u32
        {
            staged_index.unlink(hash);
            dead.try_push(*hash).map_err(|_| ParseError::Nomem)?;
        }
    }
    // Original WIM-backed blobs remain retained even when their reference count
    // reaches zero; disk reference counts cannot be trusted (blob_table.c).
    for hash in decrements {
        if let Some(blob) = handle.owned_blobs.get_mut(&hash) {
            blob.descriptor.blob.reference_count =
                blob.descriptor.blob.reference_count.saturating_sub(1);
        } else if let Some(lookup) = handle.lookup.as_mut()
            && let Some(blob) = lookup.blobs.iter_mut().find(|blob| blob.hash == hash)
        {
            blob.reference_count = blob.reference_count.saturating_sub(1);
        }
    }
    handle.blob_index = staged_index;
    for hash in &dead {
        handle.owned_blobs.remove(hash);
    }
    let identity = crate::engine::handles::image_identity(handle, &handle.images[index]);
    handle.images.remove(index);
    handle.image_deletion_occurred = true;
    handle.dirty_images.remove(&identity);
    handle.image_owners.remove(index);
    handle.xml = xml;
    handle.header.image_count -= 1;
    if handle.header.boot_index == image as u32 {
        handle.header.boot_index = 0;
    } else if handle.header.boot_index > image as u32 {
        handle.header.boot_index -= 1;
    }
    Ok(())
}

/// Delete one image, or all images when `image` is -1.
/// As upstream, reverse all-image deletion can stop after earlier deletions if
/// later metadata validation fails; source WIM files remain untouched.
///
/// # Safety
/// `handle` must be live and exclusively accessed for this call.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn wimlib_delete_image(handle: *mut WimHandle, image: c_int) -> c_int {
    // SAFETY: The caller guarantees a live exclusively accessed handle.
    let Some(handle) = (unsafe { handle_mut(handle) }) else {
        return 24;
    };
    delete_images(handle, image).map_or_else(|error| error as c_int, |()| 0)
}

pub(crate) fn delete_images(handle: &mut WimHandle, image: c_int) -> Result<(), ParseError> {
    let (first, last) = if image == -1 {
        (1, handle.header.image_count as c_int)
    } else {
        (image, image)
    };
    for image in (first..=last).rev() {
        delete_one(handle, image)?;
    }
    Ok(())
}
