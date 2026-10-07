// SPDX-License-Identifier: LGPL-2.1-or-later
//! Platform-neutral retained resource input for filesystem and pipe consumers.
use crate::engine::handles::WimHandle;
use wim_format::{Compression, ParseError, lookup::LookupResource, resource::ResourceLayout};

trait ReadExactAtNative {
    fn read_exact_at_native(&self, bytes: &mut [u8], offset: u64) -> std::io::Result<()>;
}
impl ReadExactAtNative for std::fs::File {
    fn read_exact_at_native(&self, bytes: &mut [u8], offset: u64) -> std::io::Result<()> {
        #[cfg(unix)]
        {
            use std::os::unix::fs::FileExt;
            self.read_exact_at(bytes, offset)
        }
        #[cfg(windows)]
        {
            use std::os::windows::fs::FileExt;
            let mut remaining = bytes;
            let mut position = offset;
            while !remaining.is_empty() {
                match self.seek_read(remaining, position) {
                    Ok(0) => return Err(std::io::ErrorKind::UnexpectedEof.into()),
                    Ok(count) => {
                        position = position
                            .checked_add(count as u64)
                            .ok_or(std::io::ErrorKind::InvalidInput)?;
                        remaining = &mut remaining[count..];
                    }
                    Err(error) if error.kind() == std::io::ErrorKind::Interrupted => continue,
                    Err(error) => return Err(error),
                }
            }
            Ok(())
        }
    }
}

pub(super) type BlobOrder = (u8, [u8; 16], u16, u64, u64);

pub(super) enum BlobSource<'a> {
    Captured {
        stream: std::sync::Arc<crate::engine::capture::CapturedStream>,
        file: std::cell::RefCell<Option<std::fs::File>>,
    },
    Backing {
        file: &'a crate::engine::backing::Backing,
        resource: &'a LookupResource,
        offset: u64,
        decoder: std::cell::RefCell<Option<Box<ms_compress::lzms::LzmsDecoder>>>,
    },
    Decoded(&'a [u8]),
}

pub(super) fn chunk_end(source: &BlobSource<'_>, offset: u64, size: u64, chunk: u64) -> u64 {
    let base = match source {
        BlobSource::Backing {
            resource, offset, ..
        } if resource.header.flags & (4 | 16) != 0 => *offset,
        _ => 0,
    };
    offset + (chunk - (base + offset) % chunk).min(size - offset)
}
pub(super) fn blob_source<'a>(
    handle: &'a WimHandle,
    hash: &[u8; 20],
) -> Result<(BlobSource<'a>, u64, u64, BlobOrder), ParseError> {
    if handle.removed_blobs.contains(hash) {
        return Err(ParseError::ResourceNotFound);
    }
    if let Some(captured) = crate::engine::lookup::captured_resources(handle)?
        .into_iter()
        .find(|b| b.hash == *hash)
    {
        let size = captured.stream.size;
        let order = match &captured.stream.source {
            crate::engine::capture::CapturedSource::File(_) => 2,
            crate::engine::capture::CapturedSource::Inline(_) => 3,
            #[cfg(feature = "disk-capture")]
            crate::engine::capture::CapturedSource::Volume(_) => 3,
            #[cfg(windows)]
            crate::engine::capture::CapturedSource::Temporary(_) => 3,
        };
        return Ok((
            BlobSource::Captured {
                stream: captured.stream,
                file: std::cell::RefCell::new(None),
            },
            size,
            32768,
            (order, [0; 16], 0, 0, 0),
        ));
    }
    if let Some(blob) = handle.owned_blobs.get(hash) {
        if let Some(stream) = &blob.captured {
            let order = match &stream.source {
                crate::engine::capture::CapturedSource::File(_) => 2,
                crate::engine::capture::CapturedSource::Inline(_) => 3,
                #[cfg(feature = "disk-capture")]
                crate::engine::capture::CapturedSource::Volume(_) => 3,
                #[cfg(windows)]
                crate::engine::capture::CapturedSource::Temporary(_) => 3,
            };
            return Ok((
                BlobSource::Captured {
                    stream: stream.clone(),
                    file: std::cell::RefCell::new(None),
                },
                stream.size,
                32768,
                (order, [0; 16], 0, 0, 0),
            ));
        }
        let size = blob.descriptor.blob.size;
        let offset = blob.descriptor.resource.header.offset_in_wim;
        if let Some(backing) = &blob.backing {
            let header = backing.header()?;
            let chunk = u64::from(blob.descriptor.resource.chunk_size).max(32768);
            return Ok((
                BlobSource::Backing {
                    file: backing,
                    resource: &blob.descriptor.resource,
                    offset: blob.descriptor.blob.offset,
                    decoder: std::cell::RefCell::new(None),
                },
                size,
                chunk,
                (
                    1,
                    header.guid,
                    header.part_number,
                    offset,
                    blob.descriptor.blob.offset,
                ),
            ));
        }
        return Ok((
            BlobSource::Decoded(&blob.bytes),
            size,
            32768,
            (4, [0; 16], 0, offset, blob.descriptor.blob.offset),
        ));
    }
    let backing = handle
        .backing
        .as_deref()
        .ok_or(ParseError::ResourceNotFound)?;
    let lookup = handle.lookup.as_ref().ok_or(ParseError::ResourceNotFound)?;
    let blob = lookup.find(hash).ok_or(ParseError::ResourceNotFound)?;
    let resource = &lookup.resources[blob.resource_index];
    let result = (
        blob.size,
        u64::from(resource.chunk_size).max(32768),
        (
            1,
            handle.header.guid,
            handle.header.part_number,
            resource.header.offset_in_wim,
            blob.offset,
        ),
    );
    Ok((
        BlobSource::Backing {
            file: backing,
            resource,
            offset: blob.offset,
            decoder: std::cell::RefCell::new(None),
        },
        result.0,
        result.1,
        result.2,
    ))
}

pub(super) fn read_chunk(
    source: &BlobSource<'_>,
    offset: u64,
    end: u64,
    flags: u32,
) -> Result<Vec<u8>, ParseError> {
    Ok(match source {
        BlobSource::Captured { stream, file } => {
            let mut bytes = vec![0; usize::try_from(end - offset).map_err(|_| ParseError::Read)?];
            match &stream.source {
                crate::engine::capture::CapturedSource::Inline(source) => {
                    bytes.copy_from_slice(
                        source
                            .get(offset as usize..end as usize)
                            .ok_or(ParseError::Read)?,
                    );
                }
                #[cfg(windows)]
                crate::engine::capture::CapturedSource::Temporary(source) => {
                    source
                        .read_exact_at_native(&mut bytes, offset)
                        .map_err(|_| ParseError::Read)?;
                }
                #[cfg(feature = "disk-capture")]
                crate::engine::capture::CapturedSource::Volume(source) => {
                    source
                        .read_exact_at(offset, &mut bytes)
                        .map_err(|_| ParseError::Read)?;
                }
                crate::engine::capture::CapturedSource::File(path) => {
                    let mut file = file.borrow_mut();
                    if file.is_none() {
                        *file = Some(std::fs::File::open(path).map_err(|_| ParseError::Open)?);
                    }
                    file.as_ref()
                        .ok_or(ParseError::Open)?
                        .read_exact_at_native(&mut bytes, offset)
                        .map_err(|error| {
                            if error.kind() == std::io::ErrorKind::UnexpectedEof {
                                #[cfg(target_os = "linux")]
                                // SAFETY: Preserve original concurrently truncated file error.
                                unsafe {
                                    *libc::__errno_location() = libc::EINVAL;
                                }
                                ParseError::ConcurrentModificationDetected
                            } else {
                                ParseError::Read
                            }
                        })?;
                }
            }
            bytes
        }
        BlobSource::Backing {
            file,
            resource,
            offset: base,
            decoder,
        } => {
            let layout = if resource.solid {
                ResourceLayout::Solid
            } else if file.header()?.magic == wim_format::PIPABLE_MAGIC {
                ResourceLayout::Pipable
            } else {
                ResourceLayout::Ordinary
            };
            let mut cache = if flags & 2 == 0 {
                Some(file.chunk_cache()?.lock().map_err(|_| ParseError::Read)?)
            } else {
                None
            };
            wim_format::file_resource::read_resource_range_cached(
                &mut file.reader(),
                &resource.header,
                Compression::from_i32(resource.compression_code as i32)?,
                resource.chunk_size,
                layout,
                base + offset..base + end,
                |kind, input, output| {
                    let decode = |output: &mut [u8]| match kind {
                        Compression::Xpress => ms_compress::decompress_xpress(input, output)
                            .map_err(|_| ParseError::Decompression),
                        Compression::Lzx => ms_compress::lzx::decompress_lzx(
                            input,
                            output,
                            resource.chunk_size as usize,
                        )
                        .map_err(|_| ParseError::Decompression),
                        Compression::Lzms => {
                            let mut decoder = decoder.borrow_mut();
                            if decoder.is_none() {
                                let scratch = ms_compress::lzms::LzmsDecoder::new()
                                    .map_err(|_| ParseError::Nomem)?;
                                *decoder = Some(Box::new(scratch));
                            }
                            decoder
                                .as_mut()
                                .ok_or(ParseError::Nomem)?
                                .decompress(input, output)
                                .map_err(|e| {
                                    if e == ms_compress::lzms::LzmsError::OutOfMemory {
                                        ParseError::Nomem
                                    } else {
                                        ParseError::Decompression
                                    }
                                })
                        }
                        Compression::None => Err(ParseError::Decompression),
                    };
                    match decode(output) {
                        Err(ParseError::Decompression) if flags & 2 != 0 => {
                            output.fill(0);
                            let _ = decode(output);
                            Ok(())
                        }
                        Err(ParseError::Decompression) => {
                            #[cfg(target_os = "linux")]
                            // SAFETY: Upstream resource.c reports failed decompression with EINVAL.
                            unsafe {
                                *libc::__errno_location() = libc::EINVAL;
                            }
                            Err(ParseError::Decompression)
                        }
                        result => result,
                    }
                },
                cache.as_deref_mut(),
            )
            .map_err(wim_format::file_archive::read_error)?
        }
        BlobSource::Decoded(b) => b
            .get(offset as usize..end as usize)
            .ok_or(ParseError::UnexpectedEndOfFile)?
            .to_vec(),
    })
}
