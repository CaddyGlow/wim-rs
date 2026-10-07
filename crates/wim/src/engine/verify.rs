// SPDX-License-Identifier: LGPL-2.1-or-later
//! Current metadata verification and chunked SHA-1 with real progress callbacks.
use crate::engine::{
    handles::{HandleImage, OwnedBlob, WimHandle, image_metadata_bytes},
    progress::{
        ProgressInfo, ProgressRegistration, VerifyImageProgress, VerifyStreamsProgress,
        filename_buffer, next_progress,
    },
};
use sha1::{Digest, Sha1};
use std::ffi::c_int;
use wim_format::{ParseError, file_archive::FileArchive, metadata::Metadata};

struct StreamVerifier {
    registration: ProgressRegistration,
    info: ProgressInfo,
    next: u64,
}
impl StreamVerifier {
    fn consumed(&mut self, size: u64, complete: bool) -> Result<(), ParseError> {
        // SAFETY: This context always stores the verify_streams member.
        let progress = unsafe { &mut self.info.verify_streams };
        progress.completed_streams += u64::from(complete);
        progress.completed_bytes += size;
        if progress.completed_bytes >= self.next {
            // SAFETY: Registration/context and complete union remain valid for
            // the operation. No shared pending-metadata lock is held here.
            unsafe { self.registration.call(29, &mut self.info) }?;
            // SAFETY: Callback must preserve readonly event fields.
            let progress = unsafe { self.info.verify_streams };
            self.next = next_progress(progress.completed_bytes, progress.total_bytes, self.next);
        }
        Ok(())
    }
}

fn verify_archive_blob(
    archive: &FileArchive<crate::engine::backing::Reader<'_>>,
    hash: &[u8; 20],
    ctx: &mut StreamVerifier,
) -> Result<(), ParseError> {
    let blob = archive
        .lookup
        .find(hash)
        .ok_or(ParseError::ResourceNotFound)?;
    let resource = archive
        .lookup
        .resources
        .get(blob.resource_index)
        .ok_or(ParseError::InvalidLookupTableEntry)?;
    let chunk = if resource.header.flags & (4 | 16) != 0 {
        u64::from(resource.chunk_size)
    } else {
        32768
    };
    if chunk == 0 {
        return Err(ParseError::InvalidChunkSize);
    }
    let mut offset = 0u64;
    let mut digest = Sha1::new();
    let mut decoder = None;
    while offset < blob.size {
        let boundary = if resource.header.flags & (4 | 16) != 0 {
            (blob.offset + offset) % chunk
        } else {
            offset % chunk
        };
        let end = offset + (chunk - boundary).min(blob.size - offset);
        let bytes = archive.read_blob_range_with_decoder(
            hash,
            offset..end,
            |kind, input, output, chunk| match kind {
                wim_format::Compression::Lzms => {
                    if decoder.is_none() {
                        decoder = Some(
                            ms_compress::lzms::LzmsDecoder::new().map_err(|_| ParseError::Nomem)?,
                        );
                    }
                    decoder
                        .as_mut()
                        .ok_or(ParseError::Nomem)?
                        .decompress(input, output)
                        .map_err(|_| ParseError::Decompression)
                }
                wim_format::Compression::Xpress => ms_compress::decompress_xpress(input, output)
                    .map_err(|_| ParseError::Decompression),
                wim_format::Compression::Lzx => {
                    ms_compress::lzx::decompress_lzx(input, output, chunk as usize)
                        .map_err(|_| ParseError::Decompression)
                }
                wim_format::Compression::None => Err(ParseError::Decompression),
            },
        )?;
        digest.update(&bytes);
        ctx.consumed(bytes.len() as u64, end == blob.size)?;
        offset = end;
    }
    let actual: [u8; 20] = digest.finalize().into();
    if actual != *hash {
        return Err(ParseError::InvalidResourceHash);
    }
    Ok(())
}
fn verify_owned_blob(
    hash: &[u8; 20],
    blob: &OwnedBlob,
    ctx: &mut StreamVerifier,
) -> Result<(), ParseError> {
    if let Some(stream) = &blob.captured {
        let mut reader = stream.open()?;
        let mut buffer = crate::engine::collections::filled(
            usize::try_from(stream.size.min(32768)).map_err(|_| ParseError::Nomem)?,
            0u8,
        )
        .map_err(|_| ParseError::Nomem)?;
        let mut digest = Sha1::new();
        let mut offset = 0;
        while offset < stream.size {
            let length = (stream.size - offset).min(buffer.len() as u64) as usize;
            reader.read_range(offset, &mut buffer[..length])?;
            digest.update(&buffer[..length]);
            offset += length as u64;
            ctx.consumed(length as u64, offset == stream.size)?;
        }
        let actual: [u8; 20] = digest.finalize().into();
        if actual != *hash {
            return Err(match &stream.source {
                crate::engine::capture::CapturedSource::File(_) => {
                    ParseError::ConcurrentModificationDetected
                }
                #[cfg(feature = "disk-capture")]
                crate::engine::capture::CapturedSource::Volume(_) => {
                    ParseError::ConcurrentModificationDetected
                }
                crate::engine::capture::CapturedSource::Inline(_) => {
                    ParseError::InvalidResourceHash
                }
                #[cfg(windows)]
                crate::engine::capture::CapturedSource::Temporary(_) => {
                    ParseError::InvalidResourceHash
                }
            });
        }
        return Ok(());
    }
    if let Some(backing) = &blob.backing {
        verify_archive_blob(&backing.archive()?, hash, ctx)
    } else {
        // Attached buffers are consumed once by the original reader.
        let actual: [u8; 20] = Sha1::digest(&blob.bytes).into();
        if !blob.bytes.is_empty() {
            ctx.consumed(blob.bytes.len() as u64, true)?;
        }
        if actual != *hash {
            return Err(ParseError::InvalidResourceHash);
        }
        Ok(())
    }
}

fn verify(handle: &WimHandle) -> Result<(), ParseError> {
    let filename = filename_buffer(handle.filename.as_deref())?;
    let filename = filename.as_ref().map_or(std::ptr::null(), |s| s.as_ptr());
    let captures = crate::engine::lookup::captured_resources(handle)?;
    let archive = handle
        .backing
        .as_deref()
        .map(crate::engine::backing::Backing::archive)
        .transpose()?;
    let mut info = ProgressInfo::zeroed();
    info.verify_image = VerifyImageProgress {
        wimfile: filename,
        total_images: handle.header.image_count,
        current_image: 0,
    };
    for (image_index, image) in handle.images.iter().enumerate() {
        info.verify_image.current_image = image_index as u32 + 1;
        // Image callbacks re-read the registration every time, as upstream does.
        // SAFETY: The stored caller registration/context and filename remain live.
        unsafe { handle.progress.get().call(27, &mut info) }?;
        if let HandleImage::Source(index) = image {
            let archive = archive.as_ref().ok_or(ParseError::MetadataNotFound)?;
            let blob = archive
                .lookup
                .metadata
                .get((*index - 1) as usize)
                .ok_or(ParseError::MetadataNotFound)?;
            if blob.size / 512 > handle.backing.as_ref().map_or(0, |data| data.len() as u64) {
                return Err(ParseError::InvalidMetadataResource);
            }
        }
        let bytes = image_metadata_bytes(handle, image_index)?;
        let metadata = Metadata::parse(&bytes)?;
        for index in 0..metadata.nodes.len() {
            let entry = metadata
                .inode_entry(index)
                .ok_or(ParseError::InvalidMetadataResource)?;
            for stream in &entry.streams {
                if stream.hash != [0; 20]
                    && (handle.removed_blobs.contains(&stream.hash)
                        || handle
                            .lookup
                            .as_ref()
                            .and_then(|lookup| lookup.find(&stream.hash))
                            .is_none()
                            && !handle.owned_blobs.contains_key(&stream.hash)
                            && !captures.iter().any(|resource| resource.hash == stream.hash))
                {
                    return Err(ParseError::ResourceNotFound);
                }
            }
        }
        // SAFETY: Pending metadata locks have been released before callbacks.
        unsafe { handle.progress.get().call(28, &mut info) }?;
    }
    enum Data<'a> {
        Original([u8; 20]),
        Owned(&'a [u8; 20], &'a OwnedBlob),
        Captured(&'a crate::engine::lookup::CapturedResource),
    }
    let mut blobs = Vec::new();
    blobs
        .try_reserve_exact(
            handle.owned_blobs.len() + handle.lookup.as_ref().map_or(0, |l| l.blobs.len()),
        )
        .map_err(|_| ParseError::Nomem)?;
    for (hash, blob) in handle.owned_blobs.iter() {
        if !handle.removed_blobs.contains(hash) {
            let origin = if let Some(backing) = &blob.backing {
                let source = backing.archive()?;
                (
                    1,
                    source.header.guid,
                    source.header.part_number,
                    backing.as_ptr() as usize,
                )
            } else {
                (2, [0; 16], 0, 0)
            };
            blobs.push((
                origin,
                blob.descriptor.resource.header.offset_in_wim,
                blob.descriptor.blob.offset,
                blob.descriptor.blob.size,
                Data::Owned(hash, blob),
            ));
        }
    }
    if let Some(lookup) = &handle.lookup {
        for blob in &lookup.blobs {
            if !handle.removed_blobs.contains(&blob.hash)
                && !handle.owned_blobs.contains_key(&blob.hash)
            {
                blobs.push((
                    (
                        1,
                        handle.header.guid,
                        handle.header.part_number,
                        handle
                            .backing
                            .as_ref()
                            .map_or(0, |bytes| bytes.as_ptr() as usize),
                    ),
                    lookup.resources[blob.resource_index].header.offset_in_wim,
                    blob.offset,
                    blob.size,
                    Data::Original(blob.hash),
                ));
            }
        }
    }
    for (index, resource) in captures.iter().enumerate() {
        if resource.hash == [0; 20]
            || handle.owned_blobs.contains_key(&resource.hash)
            || handle
                .lookup
                .as_ref()
                .is_some_and(|table| table.find(&resource.hash).is_some())
        {
            continue;
        }
        blobs.try_reserve(1).map_err(|_| ParseError::Nomem)?;
        blobs.push((
            (3, [0; 16], 0, 0),
            index as u64,
            0,
            resource.stream.size,
            Data::Captured(resource),
        ));
    }
    blobs.sort_by_key(|(origin, offset, in_resource, _, _)| (*origin, *offset, *in_resource));
    let total = blobs.iter().try_fold(0u64, |sum, (_, _, _, size, _)| {
        sum.checked_add(*size).ok_or(ParseError::InvalidParam)
    })?;
    let mut info = ProgressInfo::zeroed();
    info.verify_streams = VerifyStreamsProgress {
        wimfile: filename,
        total_streams: blobs.len() as u64,
        total_bytes: total,
        completed_streams: 0,
        completed_bytes: 0,
    };
    let mut ctx = StreamVerifier {
        registration: handle.progress.get(),
        info,
        next: 0,
    };
    // The stream phase snapshots its registration before the initial event.
    // SAFETY: Complete event union, filename and caller context remain live.
    unsafe { ctx.registration.call(29, &mut ctx.info) }?;
    for (_, _, _, _, blob) in blobs {
        match blob {
            Data::Original(hash) => verify_archive_blob(
                archive.as_ref().ok_or(ParseError::ResourceNotFound)?,
                &hash,
                &mut ctx,
            )?,
            Data::Owned(hash, blob) => verify_owned_blob(hash, blob, &mut ctx)?,
            Data::Captured(resource) => {
                let mut reader = resource.stream.open()?;
                let mut buffer = Vec::new();
                let length = usize::try_from(resource.stream.size.min(32768))
                    .map_err(|_| ParseError::Nomem)?;
                buffer
                    .try_reserve_exact(length)
                    .map_err(|_| ParseError::Nomem)?;
                buffer.resize(length, 0);
                let mut digest = Sha1::new();
                let mut offset = 0;
                while offset < resource.stream.size {
                    let length = (resource.stream.size - offset).min(buffer.len() as u64) as usize;
                    reader.read_range(offset, &mut buffer[..length])?;
                    digest.update(&buffer[..length]);
                    offset += length as u64;
                    ctx.consumed(length as u64, offset == resource.stream.size)?;
                }
                let hash: [u8; 20] = digest.finalize().into();
                if hash != resource.hash {
                    return Err(match resource.stream.source {
                        crate::engine::capture::CapturedSource::File(_) => {
                            ParseError::ConcurrentModificationDetected
                        }
                        #[cfg(windows)]
                        crate::engine::capture::CapturedSource::Temporary(_) => {
                            ParseError::InvalidResourceHash
                        }
                        #[cfg(feature = "disk-capture")]
                        crate::engine::capture::CapturedSource::Volume(_) => {
                            ParseError::ConcurrentModificationDetected
                        }
                        crate::engine::capture::CapturedSource::Inline(_) => {
                            ParseError::InvalidResourceHash
                        }
                    });
                }
            }
        }
    }
    Ok(())
}

/// Verify metadata and content SHA-1 with real per-image and per-chunk progress.
/// Callback aborts return 76; unknown statuses return 77. Replacing/unregistering
/// progress affects subsequent image events; stream verification snapshots its
/// registration for that phase. Integrity-table checking is an open operation.
///
/// # Safety
/// A nonnull handle must be live and exclusively accessed. Registered callback
/// code/context must remain valid through this synchronous operation, including
/// after replacement/unregistration while a stream phase is in flight. Callbacks
/// may only inspect supplied event payloads or replace progress on this handle;
/// they must not free or otherwise access/mutate its active resource state.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn wimlib_verify_wim(handle: *mut WimHandle, flags: c_int) -> c_int {
    if flags != 0 {
        return 24;
    }
    // SAFETY: Caller guarantees a live handle when nonnull.
    let Some(handle) = (unsafe { handle.as_ref() }) else {
        return 24;
    };
    verify_archive_inner(handle, flags).map_or_else(|error| error as c_int, |()| 0)
}

/// Verify a typed archive handle without an ABI call.
pub(crate) fn verify_archive(handle: &mut WimHandle, flags: c_int) -> Result<(), ParseError> {
    verify_archive_inner(handle, flags)
}

fn verify_archive_inner(handle: &WimHandle, flags: c_int) -> Result<(), ParseError> {
    if flags != 0 {
        return Err(ParseError::InvalidParam);
    }
    verify(handle)
}
