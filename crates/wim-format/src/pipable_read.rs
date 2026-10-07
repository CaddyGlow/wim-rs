// SPDX-License-Identifier: LGPL-2.1-or-later
//! Seekless pipable WIM framing derived from wimlib `extract.c` and `resource.c`.
//! Resources are consumed one chunk at a time. No enclosing file, resource-sized
//! buffer, seek operation, or read-ahead is required.
use crate::file_resource::FileReadError;
use crate::{Compression, HEADER_SIZE, Header, PIPABLE_MAGIC, ParseError};
use alloc::vec::Vec;
use ms_compress::context::{ContextError, Decompressor};
use sha1::{Digest, Sha1};
use std::io::{self, Read};

const BLOB_MAGIC: u64 = 0x2b9b9ba2443db9d8;
const RAW_BUFFER_SIZE: usize = 32768;

/// The 40-byte header preceding a pipable resource.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BlobHeader {
    /// Size after decompression. Zero-sized pipable resources are invalid.
    pub uncompressed_size: u64,
    /// SHA-1 of the resource's uncompressed bytes.
    pub hash: [u8; 20],
    /// Uninterpreted 32-bit on-pipe flags; bit 2 denotes compression.
    pub flags: u32,
    /// Position immediately after this header in the input stream.
    pub offset: u64,
}

/// A subsequent 208-byte pipable header encountered between resource frames.
/// Upstream extraction reads its identity without validating its other fields.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PartHeader {
    /// Complete original header bytes.
    pub bytes: [u8; HEADER_SIZE],
    /// Archive identifier used for split-part progress notifications.
    pub guid: [u8; 16],
    /// One-based part number, retained without normalization.
    pub part_number: u16,
    /// Total part count, retained without normalization.
    pub total_parts: u16,
}

/// The next resource or split/final header in a pipable input.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Frame {
    /// Consume this resource before requesting another frame.
    Blob(BlobHeader),
    /// A header encountered while scanning for selected payload resources.
    Part(PartHeader),
}

/// A sequential reader with bounded reusable scratch and codec storage.
/// A failed resource read leaves the reader unusable; callers must not retry a
/// partially consumed resource. The underlying input remains caller-owned.
pub struct PipableReader<R> {
    input: R,
    header: Header,
    compression: Compression,
    read: u64,
    pending: Option<BlobHeader>,
    failed: bool,
    decoded: Option<Vec<u8>>,
    encoded: Option<Vec<u8>>,
    decoder: Option<Decompressor>,
}

impl<R: Read> PipableReader<R> {
    /// Read and validate the initial fixed header without reading the first blob.
    pub fn new(mut input: R) -> Result<Self, FileReadError> {
        let mut read = 0;
        let mut bytes = [0; HEADER_SIZE];
        read_exact(&mut input, &mut read, &mut bytes)?;
        let header = Header::parse(&bytes, None)?;
        let compression = header.validate_compression()?;
        if header.magic != PIPABLE_MAGIC {
            return Err(ParseError::NotPipable.into());
        }
        Ok(Self {
            input,
            header,
            compression,
            read,
            pending: None,
            failed: false,
            decoded: None,
            encoded: None,
            decoder: None,
        })
    }

    /// Initial header. Later part headers do not replace its compression settings.
    pub fn header(&self) -> &Header {
        &self.header
    }

    /// Number of bytes actually consumed, including partial reads before failure.
    pub fn bytes_read(&self) -> u64 {
        self.read
    }

    /// Recover the input at its actual current position, even after cancellation.
    pub fn into_inner(self) -> R {
        self.input
    }

    /// Read exactly one frame header. No resource data is prefetched.
    /// Part headers are accepted only where the original payload loop permits them.
    pub fn next_frame(&mut self, allow_part_headers: bool) -> Result<Frame, FileReadError> {
        if self.failed || self.pending.is_some() {
            return Err(ParseError::InvalidParam.into());
        }
        let mut bytes = [0; 40];
        self.failed = true;
        read_exact(&mut self.input, &mut self.read, &mut bytes)?;
        if bytes[..8] == PIPABLE_MAGIC && allow_part_headers {
            let mut header = [0; HEADER_SIZE];
            header[..40].copy_from_slice(&bytes);
            read_exact(&mut self.input, &mut self.read, &mut header[40..])?;
            let mut guid = [0; 16];
            guid.copy_from_slice(&header[24..40]);
            let part = PartHeader {
                guid,
                part_number: u16::from_le_bytes([header[40], header[41]]),
                total_parts: u16::from_le_bytes([header[42], header[43]]),
                bytes: header,
            };
            self.failed = false;
            return Ok(Frame::Part(part));
        }
        if u64::from_le_bytes(
            bytes[..8]
                .try_into()
                .map_err(|_| ParseError::InvalidPipableWim)?,
        ) != BLOB_MAGIC
        {
            return Err(ParseError::InvalidPipableWim.into());
        }
        let mut hash = [0; 20];
        hash.copy_from_slice(&bytes[16..36]);
        let blob = BlobHeader {
            uncompressed_size: u64::from_le_bytes(
                bytes[8..16]
                    .try_into()
                    .map_err(|_| ParseError::InvalidPipableWim)?,
            ),
            hash,
            flags: u32::from_le_bytes(
                bytes[36..40]
                    .try_into()
                    .map_err(|_| ParseError::InvalidPipableWim)?,
            ),
            offset: self.read,
        };
        if blob.uncompressed_size == 0 {
            return Err(ParseError::InvalidPipableWim.into());
        }
        self.pending = Some(blob);
        self.failed = false;
        Ok(Frame::Blob(blob))
    }

    /// Decode and consume one complete resource. The callback receives borrowed
    /// uncompressed chunks (at most the archive chunk size, or 32 KiB when raw).
    /// Callback errors prevent all subsequent input reads, including the trailing
    /// chunk-offset table. Hash verification happens after that table is consumed.
    /// Recovery retries malformed compressed data into a zeroed output buffer and
    /// tolerates a mismatched hash, matching upstream's data recovery policy.
    pub fn read_resource<F>(
        &mut self,
        blob: BlobHeader,
        verify_hash: bool,
        recover_data: bool,
        mut consume: F,
    ) -> Result<(), FileReadError>
    where
        F: FnMut(&[u8]) -> Result<(), ParseError>,
    {
        if self.failed || self.pending != Some(blob) {
            return Err(ParseError::InvalidParam.into());
        }
        self.failed = true;
        let compressed = blob.flags & 4 != 0;
        // A SOLID-only header enters upstream's compressed reader with the
        // zero chunk size assigned by wim_reshdr_to_desc(). Pipe reads never
        // select the alternate solid chunk-table format.
        if !compressed && blob.flags & 16 != 0 {
            return Err(ParseError::InvalidChunkSize.into());
        }
        let chunk_size = if compressed {
            self.header.chunk_size as usize
        } else {
            RAW_BUFFER_SIZE
        };
        if compressed && !chunk_size.is_power_of_two() {
            return Err(ParseError::InvalidChunkSize.into());
        }
        if compressed && self.decoder.is_none() {
            self.decoder = Some(
                Decompressor::new(self.compression as i32, chunk_size).map_err(context_error)?,
            );
        }
        ensure_buffer(&mut self.decoded, chunk_size)?;
        let mut digest = Sha1::new();
        let mut remaining = blob.uncompressed_size;
        while remaining != 0 {
            let length = remaining.min(chunk_size as u64) as usize;
            let encoded_length = if compressed {
                let mut bytes = [0; 4];
                read_exact(&mut self.input, &mut self.read, &mut bytes)?;
                let size = u32::from_le_bytes(bytes) as usize;
                if size == 0 || size > length {
                    return Err(ParseError::Decompression.into());
                }
                size
            } else {
                length
            };
            let decoded = self.decoded.as_mut().ok_or(ParseError::Nomem)?;
            if encoded_length == length {
                read_exact(&mut self.input, &mut self.read, &mut decoded[..length])?;
            } else {
                ensure_buffer(&mut self.encoded, chunk_size - 1)?;
                let encoded = self.encoded.as_mut().ok_or(ParseError::Nomem)?;
                read_exact(
                    &mut self.input,
                    &mut self.read,
                    &mut encoded[..encoded_length],
                )?;
                let decoder = self.decoder.as_mut().ok_or(ParseError::Decompression)?;
                if let Err(error) =
                    decoder.decompress(&encoded[..encoded_length], &mut decoded[..length])
                {
                    if !recover_data || error == ContextError::OutOfMemory {
                        return Err(context_error(error).into());
                    }
                    decoded[..length].fill(0);
                    let _ = decoder.decompress(&encoded[..encoded_length], &mut decoded[..length]);
                }
            }
            if verify_hash {
                digest.update(&decoded[..length]);
            }
            consume(&decoded[..length])?;
            remaining -= length as u64;
        }
        if compressed {
            let chunks = blob.uncompressed_size.div_ceil(chunk_size as u64);
            let width = if blob.uncompressed_size > u32::MAX as u64 {
                8
            } else {
                4
            };
            let mut remaining = (chunks - 1)
                .checked_mul(width)
                .ok_or(ParseError::Decompression)?;
            let decoded = self.decoded.as_mut().ok_or(ParseError::Nomem)?;
            while remaining != 0 {
                let length = remaining.min(decoded.len() as u64) as usize;
                read_exact(&mut self.input, &mut self.read, &mut decoded[..length])?;
                remaining -= length as u64;
            }
        }
        if verify_hash && <[u8; 20]>::from(digest.finalize()) != blob.hash && !recover_data {
            return Err(ParseError::InvalidResourceHash.into());
        }
        self.pending = None;
        self.failed = false;
        Ok(())
    }

    /// Consume an unselected resource using the same framing and decompression
    /// checks as selected resources, without verifying its SHA-1.
    pub fn skip_resource(&mut self, blob: BlobHeader) -> Result<(), FileReadError> {
        self.read_resource(blob, false, false, |_| Ok(()))
    }
}

fn context_error(error: ContextError) -> ParseError {
    match error {
        ContextError::OutOfMemory => ParseError::Nomem,
        ContextError::InvalidCompressionType => ParseError::InvalidCompressionType,
        _ => ParseError::Decompression,
    }
}
fn ensure_buffer(buffer: &mut Option<Vec<u8>>, size: usize) -> Result<(), ParseError> {
    if buffer.as_ref().is_none_or(|buffer| buffer.len() < size) {
        *buffer = Some({
            let mut bytes = Vec::new();
            bytes
                .try_reserve_exact(size)
                .map_err(|_| ParseError::Nomem)?;
            bytes.resize(size, 0);
            bytes
        });
    }
    Ok(())
}
fn read_exact<R: Read>(
    input: &mut R,
    count: &mut u64,
    mut bytes: &mut [u8],
) -> Result<(), FileReadError> {
    while !bytes.is_empty() {
        match input.read(bytes) {
            Ok(0) => return Err(ParseError::UnexpectedEndOfFile.into()),
            Ok(size) => {
                *count += size as u64;
                bytes = &mut bytes[size..];
            }
            Err(error) if error.kind() == io::ErrorKind::Interrupted => {}
            Err(error) => return Err(error.into()),
        }
    }
    Ok(())
}
