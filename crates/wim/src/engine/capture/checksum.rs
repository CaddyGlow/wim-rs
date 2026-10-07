//! Bounded deferred hashing before operations that require resolved streams.

use crate::engine::collections::FallibleCollections as _;
use crate::engine::handles::{WimHandle, pending_metadata};
use sha1::{Digest, Sha1};
use std::collections::{HashMap, HashSet};
use wim_format::ParseError;
/// Resolve all regular streams across images, without caching payload bytes.
pub(crate) fn checksum_pending(handle: &WimHandle) -> Result<(), ParseError> {
    let mut hashes = HashMap::new();
    let mut canonical = HashMap::new();
    let mut stored: HashSet<[u8; 20]> = handle.owned_blobs.keys().copied().collect();
    if let Some(lookup) = &handle.lookup {
        stored.extend(
            lookup
                .blobs
                .iter()
                .filter(|blob| !handle.removed_blobs.contains(&blob.hash))
                .map(|blob| blob.hash),
        );
    }
    for image in &handle.images {
        let Some(mut plan) = crate::engine::handles::image_capture_plan(image)? else {
            continue;
        };
        let mut changed = false;
        let mut publication = Vec::new();
        for binding in &mut plan.bindings {
            let node = plan
                .tree
                .nodes
                .get_mut(binding.node)
                .ok_or(ParseError::InvalidMetadataResource)?;
            let hash = if binding.slot == 0 {
                &mut node.main_hash
            } else {
                &mut node
                    .extra_streams
                    .get_mut(binding.slot - 1)
                    .ok_or(ParseError::InvalidMetadataResource)?
                    .hash
            };
            if *hash == [0; 20] {
                let identity = std::sync::Arc::as_ptr(&binding.stream) as usize;
                *hash = if let Some(hash) = hashes.get(&identity) {
                    *hash
                } else {
                    let mut reader = binding.stream.open()?;
                    let mut offset = 0;
                    let mut digest = Sha1::new();
                    let mut bytes = vec![0; 65_536];
                    while offset < binding.stream.size {
                        let length =
                            (binding.stream.size - offset).min(bytes.len() as u64) as usize;
                        reader.read_range(offset, &mut bytes[..length])?;
                        digest.update(&bytes[..length]);
                        offset += length as u64;
                    }
                    let hash: [u8; 20] = digest.finalize().into();
                    hashes.insert(identity, hash);
                    hash
                };
                changed = true;
            }
            if !stored.contains(hash) {
                publication.try_push(*hash).map_err(|_| ParseError::Nomem)?;
                let stream = canonical
                    .entry(*hash)
                    .or_insert_with(|| binding.stream.clone());
                if !std::sync::Arc::ptr_eq(stream, &binding.stream) {
                    binding.stream = stream.clone();
                    changed = true;
                }
            } else {
                changed = true;
            }
        }
        plan.bindings.retain(|binding| {
            let node = &plan.tree.nodes[binding.node];
            let hash = if binding.slot == 0 {
                node.main_hash
            } else {
                node.extra_streams[binding.slot - 1].hash
            };
            !stored.contains(&hash)
        });
        if changed {
            let bytes = plan.tree.encode()?;
            let mut pending = pending_metadata(image)
                .ok_or(ParseError::InvalidMetadataResource)?
                .lock()
                .map_err(|_| ParseError::InvalidParam)?;
            pending.capture = Some(plan);
            pending.metadata = bytes;
            drop(pending);
        }
        for hash in &publication {
            handle
                .blob_index
                .insert(*hash, crate::engine::blob_index::BlobOwner::Captured)?;
        }
    }
    Ok(())
}
