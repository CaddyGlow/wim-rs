//! Metadata-based reuse of checksums for deferred incremental captures.
use crate::engine::collections::FallibleCollections as _;
use crate::engine::handles::{HandleImage, WimHandle, image_capture_plan, image_metadata_bytes};
use std::{
    collections::{HashMap, HashSet},
    ffi::c_int,
};

struct ReferenceUpdate {
    plan: crate::engine::capture::CapturePlan,
    bytes: Vec<u8>,
    reused: HashMap<[u8; 20], u32>,
}
use wim_format::{ParseError, metadata::Metadata, metadata_write::OwnedMetadata};

fn tree(handle: &WimHandle, index: usize) -> Result<OwnedMetadata, ParseError> {
    if let Some(plan) = image_capture_plan(&handle.images[index])? {
        return Ok(plan.tree);
    }
    if matches!(handle.images[index], HandleImage::Empty(_)) {
        return Ok(OwnedMetadata::default());
    }
    OwnedMetadata::from_metadata(&Metadata::parse(&image_metadata_bytes(handle, index)?)?)
}

fn paths(tree: &OwnedMetadata) -> HashMap<Vec<Vec<u8>>, usize> {
    let mut result = HashMap::new();
    let mut stack = if tree.nodes.is_empty() {
        Vec::new()
    } else {
        vec![(0, Vec::new())]
    };
    while let Some((index, path)) = stack.pop() {
        result.insert(path.clone(), index);
        for &child in &tree.nodes[index].children {
            let mut next = path.clone();
            next.push(tree.nodes[child].name.clone());
            stack.push((child, next));
        }
    }
    result
}

fn sizes(handle: &WimHandle) -> Result<HashMap<[u8; 20], u64>, ParseError> {
    let mut result = HashMap::new();
    for resource in crate::engine::lookup::captured_resources(handle)? {
        if resource.hash != [0; 20] {
            result.insert(resource.hash, resource.stream.size);
        }
    }
    if let Some(table) = &handle.lookup {
        for blob in &table.blobs {
            if !handle.removed_blobs.contains(&blob.hash) {
                result.insert(blob.hash, blob.size);
            }
        }
    }
    for (hash, blob) in handle.owned_blobs.iter() {
        if !handle.removed_blobs.contains(hash) {
            result.insert(*hash, blob.descriptor.blob.size);
        }
    }
    Ok(result)
}

fn reference(
    handle: &WimHandle,
    index: usize,
    template: &WimHandle,
    template_index: usize,
) -> Result<Option<ReferenceUpdate>, ParseError> {
    let template_tree = tree(template, template_index)?;
    let Some(mut plan) = image_capture_plan(&handle.images[index])? else {
        return Ok(None);
    };
    let template_paths = paths(&template_tree);
    let new_paths = paths(&plan.tree);
    let template_sizes = sizes(template)?;
    let new_sizes = sizes(handle)?;
    let mut copied = HashMap::new();
    let node_paths: HashMap<_, _> = new_paths
        .iter()
        .map(|(path, &index)| (index, path))
        .collect();
    let mut seen_inodes = HashSet::new();
    // Original inode_any_dentry selects the head of the alias list; capture
    // adds each alias at the head, so the last captured dentry is authoritative.
    for node_index in (0..plan.tree.nodes.len()).rev() {
        if let Some(identity) = plan.identities.get(node_index).copied().flatten()
            && !seen_inodes.insert((identity.session, identity.device, identity.inode))
        {
            continue;
        }
        let path = node_paths
            .get(&node_index)
            .ok_or(ParseError::InvalidMetadataResource)?;
        let Some(&old_index) = template_paths.get(*path) else {
            #[cfg(target_os = "linux")]
            // SAFETY: Original case-sensitive template lookup records ENOENT.
            unsafe {
                *libc::__errno_location() = libc::ENOENT;
            }
            continue;
        };
        let node = &plan.tree.nodes[node_index];
        let old = &template_tree.nodes[old_index];
        if node.creation_time != old.creation_time
            || node.last_write_time != old.last_write_time
            || node.last_access_time < old.last_access_time
        {
            continue;
        }
        let streams = std::iter::once((0, b"".as_slice(), node.main_hash)).chain(
            node.extra_streams
                .iter()
                .enumerate()
                .map(|(i, stream)| (i + 1, stream.name.as_slice(), stream.hash)),
        );
        let mut matches = Vec::new();
        let mut consistent = true;
        for (slot, name, hash) in streams {
            let old_hash = if slot == 0 {
                if node.attributes & 0x400 != old.attributes & 0x400 {
                    consistent = false;
                    break;
                }
                old.main_hash
            } else if let Some(stream) = old.extra_streams.iter().find(|stream| stream.name == name)
            {
                stream.hash
            } else {
                consistent = false;
                break;
            };
            let binding = plan
                .bindings
                .iter()
                .find(|binding| binding.node == node_index && binding.slot == slot);
            let size = binding.map_or_else(
                || new_sizes.get(&hash).copied().unwrap_or(0),
                |binding| binding.stream.size,
            );
            if size != template_sizes.get(&old_hash).copied().unwrap_or(0) {
                consistent = false;
                break;
            }
            if hash == [0; 20]
                && old_hash != [0; 20]
                && let Some(binding) = binding
            {
                matches.push((std::sync::Arc::as_ptr(&binding.stream) as usize, old_hash));
            }
        }
        if consistent {
            for (identity, hash) in matches {
                copied.entry(identity).or_insert(hash);
            }
        }
    }
    if copied.is_empty() {
        return Ok(None);
    }
    for binding in &plan.bindings {
        if let Some(&hash) = copied.get(&(std::sync::Arc::as_ptr(&binding.stream) as usize)) {
            let node = &mut plan.tree.nodes[binding.node];
            if binding.slot == 0 {
                node.main_hash = hash;
            } else {
                node.extra_streams[binding.slot - 1].hash = hash;
            }
        }
    }
    let mut stored = HashSet::new();
    if let Some(table) = &handle.lookup {
        stored.extend(
            table
                .blobs
                .iter()
                .filter(|blob| !handle.removed_blobs.contains(&blob.hash))
                .map(|blob| blob.hash),
        );
    }
    stored.extend(
        handle
            .owned_blobs
            .keys()
            .filter(|hash| !handle.removed_blobs.contains(*hash))
            .copied(),
    );
    let mut canonical: HashMap<_, _> = crate::engine::lookup::captured_resources(handle)?
        .into_iter()
        .filter(|resource| resource.hash != [0; 20])
        .map(|resource| (resource.hash, resource.stream))
        .collect();
    let mut reused = HashMap::new();
    plan.bindings.retain_mut(|binding| {
        let Some(&hash) = copied.get(&(std::sync::Arc::as_ptr(&binding.stream) as usize)) else {
            return true;
        };
        if stored.contains(&hash) {
            let count: &mut u32 = reused.entry(hash).or_default();
            *count = count.wrapping_add(1);
            return false;
        }
        binding.stream = canonical
            .entry(hash)
            .or_insert_with(|| binding.stream.clone())
            .clone();
        true
    });
    let bytes = plan.tree.encode()?;
    Ok(Some(ReferenceUpdate {
        plan,
        bytes,
        reused,
    }))
}

/// Reuse checksums from a timestamp- and size-consistent template image.
/// # Safety
/// Nonnull handles must be live and exclusively available for this synchronous call.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn wimlib_reference_template_image(
    handle: *mut WimHandle,
    image: c_int,
    template: *mut WimHandle,
    template_image: c_int,
    flags: c_int,
) -> c_int {
    let result = (|| {
        if flags != 0
            || handle.is_null()
            || template.is_null()
            || (handle == template && image == template_image)
        {
            return Err(ParseError::InvalidParam);
        }
        // SAFETY: Live handles are borrowed only immutably, including when both pointers coincide.
        let (handle, template) = unsafe { (&*handle, &*template) };
        if image < 1 || image as u32 > handle.header.image_count {
            return Err(ParseError::InvalidImage);
        }
        let index = image as usize - 1;
        let current = handle
            .images
            .get(index)
            .ok_or(ParseError::MetadataNotFound)?;
        if !handle
            .dirty_images
            .contains(&crate::engine::handles::image_identity(handle, current))
            && matches!(current, HandleImage::Source(_))
        {
            return Err(ParseError::InvalidParam);
        }
        if template_image < 1 || template_image as u32 > template.header.image_count {
            return Err(ParseError::InvalidImage);
        }
        let template_index = template_image as usize - 1;
        if template.images.get(template_index).is_none() {
            return Err(ParseError::MetadataNotFound);
        }
        reference(handle, index, template, template_index).map(|update| (index, update))
    })();
    match result {
        Err(error) => error as c_int,
        Ok((_, None)) => 0,
        Ok((index, Some(update))) => {
            // SAFETY: Shared source/template borrows ended before mutation, including same-handle calls.
            let handle = unsafe { &mut *handle };
            let Some(shared) = crate::engine::handles::pending_metadata(&handle.images[index])
            else {
                return ParseError::InvalidMetadataResource as c_int;
            };
            let mut publication = Vec::new();
            for binding in &update.plan.bindings {
                let node = &update.plan.tree.nodes[binding.node];
                let hash = if binding.slot == 0 {
                    node.main_hash
                } else {
                    node.extra_streams[binding.slot - 1].hash
                };
                if hash != [0; 20] && publication.try_push(hash).is_err() {
                    return ParseError::Nomem as c_int;
                }
            }
            let mut pending = match shared.lock() {
                Ok(pending) => pending,
                Err(_) => return ParseError::InvalidParam as c_int,
            };
            for (hash, count) in update.reused {
                if let Some(blob) = handle.owned_blobs.get_mut(&hash) {
                    blob.descriptor.blob.reference_count =
                        blob.descriptor.blob.reference_count.wrapping_add(count);
                } else if let Some(table) = &mut handle.lookup
                    && let Some(blob) = table.blobs.iter_mut().find(|blob| blob.hash == hash)
                {
                    blob.reference_count = blob.reference_count.wrapping_add(count);
                }
            }
            pending.capture = Some(update.plan);
            pending.metadata = update.bytes;
            drop(pending);
            for hash in &publication {
                if let Err(error) = handle
                    .blob_index
                    .insert(*hash, crate::engine::blob_index::BlobOwner::Captured)
                {
                    return error as c_int;
                }
            }
            0
        }
    }
}
