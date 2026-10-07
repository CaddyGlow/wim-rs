//! Optional SHA-1 integrity tables covering raw WIM bytes after the 208-byte
//! header through the end of the blob table, including gaps and framing.

use crate::resource::{ResourceLayout, read_resource};
use crate::{Compression, HEADER_SIZE, Header, ParseError};
#[cfg(test)]
use alloc::vec;
use alloc::vec::Vec;
use sha1::{Digest, Sha1};

/// Upstream default integrity chunk size (10 MiB).
pub const DEFAULT_CHUNK_SIZE: u32 = 10_485_760;

/// Validated integrity table. Digests are SHA-1 bytes, not hexadecimal strings.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IntegrityTable {
    chunk_size: u32,
    digests: Vec<[u8; 20]>,
}
/// Progress at the beginning and after every successfully verified chunk.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct IntegrityProgress {
    pub total_bytes: u64,
    pub completed_bytes: u64,
    pub total_chunks: u32,
    pub completed_chunks: u32,
    pub chunk_size: u32,
}
/// Upstream distinguishes absent tables from checksum mismatches.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum IntegrityStatus {
    Ok,
    Mismatch,
    Nonexistent,
}
impl IntegrityTable {
    /// Decode an already decompressed table with upstream structural validation.
    /// Chunk size need only be nonzero; reader accepts sizes outside writer bounds.
    pub fn parse(bytes: &[u8], checked_bytes: u64) -> Result<Self, ParseError> {
        if bytes.len() < 12 {
            return Err(ParseError::InvalidIntegrityTable);
        }
        let size = read_u32(bytes, 0);
        let count = read_u32(bytes, 4);
        let chunk_size = read_u32(bytes, 8);
        if u64::from(size) != bytes.len() as u64
            || u64::from(size) != 12 + u64::from(count) * 20
            || chunk_size == 0
            || u64::from(count) != checked_bytes.div_ceil(u64::from(chunk_size))
        {
            return Err(ParseError::InvalidIntegrityTable);
        }
        let mut digests = Vec::new();
        digests
            .try_reserve_exact(count as usize)
            .map_err(|_| ParseError::Nomem)?;
        for digest in bytes[12..].chunks_exact(20) {
            let mut value = [0; 20];
            value.copy_from_slice(digest);
            digests.push(value);
        }
        Ok(Self {
            chunk_size,
            digests,
        })
    }
    /// Integrity chunk size in bytes.
    pub fn chunk_size(&self) -> u32 {
        self.chunk_size
    }
    /// Stored SHA-1 digests in file order.
    pub fn digests(&self) -> &[[u8; 20]] {
        &self.digests
    }
    /// Serialize in the upstream little-endian wire format.
    pub fn encode(&self) -> Result<Vec<u8>, ParseError> {
        let size = self
            .digests
            .len()
            .checked_mul(20)
            .and_then(|n| n.checked_add(12))
            .ok_or(ParseError::Nomem)?;
        let size32 = u32::try_from(size).map_err(|_| ParseError::Nomem)?;
        let mut bytes = Vec::new();
        bytes
            .try_reserve_exact(size)
            .map_err(|_| ParseError::Nomem)?;
        bytes.extend_from_slice(&size32.to_le_bytes());
        bytes.extend_from_slice(&(self.digests.len() as u32).to_le_bytes());
        bytes.extend_from_slice(&self.chunk_size.to_le_bytes());
        for digest in &self.digests {
            bytes.extend_from_slice(digest);
        }
        Ok(bytes)
    }
    /// Compute a table for `file[208..check_end]`. Explicit chunk size permits
    /// preserving an existing table's chunk size; new writers use the default.
    pub fn calculate(file: &[u8], check_end: u64, chunk_size: u32) -> Result<Self, ParseError> {
        Self::calculate_with_progress(file, check_end, chunk_size, |_, _| Ok(()))
    }
    /// Calculate SHA-1 chunks, notifying before work and after each completed chunk.
    pub fn calculate_with_progress<F>(
        file: &[u8],
        check_end: u64,
        chunk_size: u32,
        progress: F,
    ) -> Result<Self, ParseError>
    where
        F: FnMut(u32, u64) -> Result<(), ParseError>,
    {
        Self::calculate_with_reuse(file, check_end, chunk_size, None, progress)
    }
    /// Calculate while reusing unchanged prefix chunks from a validated old table.
    /// Each reused digest is copied between callbacks; other chunks are hashed.
    pub fn calculate_with_reuse<F>(
        file: &[u8],
        check_end: u64,
        chunk_size: u32,
        old: Option<(&Self, u64)>,
        mut progress: F,
    ) -> Result<Self, ParseError>
    where
        F: FnMut(u32, u64) -> Result<(), ParseError>,
    {
        if chunk_size == 0 || check_end < HEADER_SIZE as u64 {
            return Err(ParseError::InvalidIntegrityTable);
        }
        let data = checked_range(file, check_end - HEADER_SIZE as u64)?;
        let count = data.len().div_ceil(chunk_size as usize);
        if count > (u32::MAX as usize - 12) / 20 {
            return Err(ParseError::Nomem);
        }
        let mut digests = Vec::new();
        digests
            .try_reserve_exact(count)
            .map_err(|_| ParseError::Nomem)?;
        progress(0, 0)?;
        let mut completed = 0u64;
        for (index, chunk) in data.chunks(chunk_size as usize).enumerate() {
            let reused = old.and_then(|(table, old_end)| {
                if table.chunk_size != chunk_size || old_end < HEADER_SIZE as u64 {
                    return None;
                }
                let old_bytes = old_end - HEADER_SIZE as u64;
                let begin = index as u64 * chunk_size as u64;
                let old_size = old_bytes.saturating_sub(begin).min(chunk_size as u64);
                (old_size == chunk.len() as u64 && old_size != 0)
                    .then(|| table.digests.get(index).copied())
                    .flatten()
            });
            digests.push(reused.unwrap_or_else(|| Sha1::digest(chunk).into()));
            completed += chunk.len() as u64;
            progress(digests.len() as u32, completed)?;
        }
        Ok(Self {
            chunk_size,
            digests,
        })
    }
    /// Verify raw file bytes, returning false for a checksum mismatch. I/O-size
    /// errors remain errors, distinct from a successfully read corrupt chunk.
    pub fn verify(&self, file: &[u8], checked_bytes: u64) -> Result<bool, ParseError> {
        self.verify_with_progress(file, checked_bytes, |_| Ok(()))
    }
    /// Callback is called initially and after each matching chunk. An error
    /// cancels verification immediately (use `AbortedByProgress` for ABI mapping).
    pub fn verify_with_progress<F>(
        &self,
        file: &[u8],
        checked_bytes: u64,
        mut progress: F,
    ) -> Result<bool, ParseError>
    where
        F: FnMut(IntegrityProgress) -> Result<(), ParseError>,
    {
        if self.digests.len() as u64 != checked_bytes.div_ceil(u64::from(self.chunk_size)) {
            return Err(ParseError::InvalidIntegrityTable);
        }
        let mut state = IntegrityProgress {
            total_bytes: checked_bytes,
            completed_bytes: 0,
            total_chunks: self.digests.len() as u32,
            completed_chunks: 0,
            chunk_size: self.chunk_size,
        };
        progress(state)?;
        // Check ranges per chunk, as upstream does: an early mismatch wins over
        // a later truncation, rather than validating the entire file first.
        for expected in &self.digests {
            let len = (checked_bytes - state.completed_bytes).min(u64::from(self.chunk_size));
            let start = (HEADER_SIZE as u64)
                .checked_add(state.completed_bytes)
                .ok_or(ParseError::UnexpectedEndOfFile)?;
            let end = start
                .checked_add(len)
                .ok_or(ParseError::UnexpectedEndOfFile)?;
            let start = usize::try_from(start).map_err(|_| ParseError::UnexpectedEndOfFile)?;
            let end = usize::try_from(end).map_err(|_| ParseError::UnexpectedEndOfFile)?;
            let chunk = file
                .get(start..end)
                .ok_or(ParseError::UnexpectedEndOfFile)?;
            let actual: [u8; 20] = Sha1::digest(chunk).into();
            if actual != *expected {
                return Ok(false);
            }
            state.completed_bytes += len;
            state.completed_chunks += 1;
            progress(state)?;
        }
        Ok(true)
    }
}
/// Check the optional integrity resource using an already selected WIM header.
/// Compressed integrity resources use the enclosing WIM's compression settings.
pub fn check_wim_integrity<F>(
    file: &[u8],
    header: &Header,
    decode: F,
) -> Result<IntegrityStatus, ParseError>
where
    F: FnMut(Compression, &[u8], &mut [u8]) -> Result<(), ParseError>,
{
    check_wim_integrity_with_progress(file, header, decode, |_| Ok(()))
}
/// Check an archive's integrity table with initial and completed-chunk progress.
/// Missing tables emit no callbacks. Callback failures stop checking immediately.
pub fn check_wim_integrity_with_progress<F, P>(
    file: &[u8],
    header: &Header,
    decode: F,
    progress: P,
) -> Result<IntegrityStatus, ParseError>
where
    F: FnMut(Compression, &[u8], &mut [u8]) -> Result<(), ParseError>,
    P: FnMut(IntegrityProgress) -> Result<(), ParseError>,
{
    if header.integrity_table.offset_in_wim == 0 {
        return Ok(IntegrityStatus::Nonexistent);
    }
    let end = header
        .blob_table
        .offset_in_wim
        .checked_add(header.blob_table.size_in_wim)
        .ok_or(ParseError::InvalidIntegrityTable)?;
    let checked = end
        .checked_sub(HEADER_SIZE as u64)
        .ok_or(ParseError::InvalidIntegrityTable)?;
    if header.integrity_table.uncompressed_size < 12 {
        return Err(ParseError::InvalidIntegrityTable);
    }
    let layout = if header.magic == crate::PIPABLE_MAGIC {
        ResourceLayout::Pipable
    } else {
        ResourceLayout::Ordinary
    };
    let compression = header.validate_compression()?;
    let bytes = read_resource(
        file,
        &header.integrity_table,
        compression,
        header.chunk_size,
        layout,
        decode,
    )?;
    let table = IntegrityTable::parse(&bytes, checked)?;
    Ok(if table.verify_with_progress(file, checked, progress)? {
        IntegrityStatus::Ok
    } else {
        IntegrityStatus::Mismatch
    })
}
fn checked_range(file: &[u8], checked_bytes: u64) -> Result<&[u8], ParseError> {
    let end = checked_bytes
        .checked_add(HEADER_SIZE as u64)
        .ok_or(ParseError::UnexpectedEndOfFile)?;
    let end = usize::try_from(end).map_err(|_| ParseError::UnexpectedEndOfFile)?;
    file.get(HEADER_SIZE..end)
        .ok_or(ParseError::UnexpectedEndOfFile)
}
fn read_u32(bytes: &[u8], offset: usize) -> u32 {
    u32::from_le_bytes([
        bytes[offset],
        bytes[offset + 1],
        bytes[offset + 2],
        bytes[offset + 3],
    ])
}

/// Calculate integrity over a seekable output with bounded scratch storage.
/// Progress includes the initial state and each completed chunk, matching the
/// in-memory calculation. Existing bytes must have been written before calling.
#[cfg(feature = "std")]
pub fn calculate_file<R: std::io::Read + std::io::Seek>(
    file: &mut R,
    check_end: u64,
    chunk_size: u32,
    mut progress: impl FnMut(u32, u64) -> Result<(), ParseError>,
) -> Result<IntegrityTable, ParseError> {
    use std::io::SeekFrom;
    if chunk_size == 0 {
        return Err(ParseError::InvalidIntegrityTable);
    }
    let checked = check_end
        .checked_sub(HEADER_SIZE as u64)
        .ok_or(ParseError::InvalidIntegrityTable)?;
    let count = checked.div_ceil(u64::from(chunk_size));
    let mut digests = Vec::new();
    digests
        .try_reserve_exact(usize::try_from(count).map_err(|_| ParseError::Nomem)?)
        .map_err(|_| ParseError::Nomem)?;
    progress(0, 0)?;
    file.seek(SeekFrom::Start(HEADER_SIZE as u64))
        .map_err(|_| ParseError::Read)?;
    let mut buffer = [0; 65536];
    let mut completed = 0;
    while completed < checked {
        let length = (checked - completed).min(u64::from(chunk_size));
        let mut remaining = length;
        let mut digest = Sha1::new();
        while remaining != 0 {
            let n = remaining.min(buffer.len() as u64) as usize;
            file.read_exact(&mut buffer[..n]).map_err(|error| {
                if error.kind() == std::io::ErrorKind::UnexpectedEof {
                    ParseError::UnexpectedEndOfFile
                } else {
                    ParseError::Read
                }
            })?;
            digest.update(&buffer[..n]);
            remaining -= n as u64;
        }
        digests.push(digest.finalize().into());
        completed += length;
        progress(digests.len() as u32, completed)?;
    }
    Ok(IntegrityTable {
        chunk_size,
        digests,
    })
}

#[cfg(test)]
mod calculation_progress_tests {
    use super::*;
    #[test]
    fn reuse_preserves_unchanged_prefix_digest_and_hashes_expanded_last_chunk() {
        let old_file = vec![3; HEADER_SIZE + 8];
        let old = IntegrityTable::calculate(&old_file, old_file.len() as u64, 4096).unwrap();
        let altered = vec![4; HEADER_SIZE + 8];
        let reused = IntegrityTable::calculate_with_reuse(
            &altered,
            altered.len() as u64,
            4096,
            Some((&old, old_file.len() as u64)),
            |_, _| Ok(()),
        )
        .unwrap();
        assert_eq!(reused.digests(), old.digests());
        let expanded = vec![4; HEADER_SIZE + 16];
        let renewed = IntegrityTable::calculate_with_reuse(
            &expanded,
            expanded.len() as u64,
            4096,
            Some((&old, old_file.len() as u64)),
            |_, _| Ok(()),
        )
        .unwrap();
        assert_ne!(renewed.digests(), old.digests());
        assert!(renewed.verify(&expanded, 16).unwrap());
    }
    #[test]
    fn calculation_reports_initial_and_completed_real_sha1_chunks() {
        let file = vec![3; HEADER_SIZE + 10];
        let mut events = Vec::new();
        let table = IntegrityTable::calculate_with_progress(
            &file,
            file.len() as u64,
            4,
            |chunks, bytes| {
                events.push((chunks, bytes));
                Ok(())
            },
        )
        .unwrap();
        assert_eq!(events, [(0, 0), (1, 4), (2, 8), (3, 10)]);
        assert!(table.verify(&file, 10).unwrap());
        let mut calls = 0;
        assert_eq!(
            IntegrityTable::calculate_with_progress(&file, file.len() as u64, 4, |_, _| {
                calls += 1;
                Err(ParseError::AbortedByProgress)
            }),
            Err(ParseError::AbortedByProgress)
        );
        assert_eq!(calls, 1);
    }
}
