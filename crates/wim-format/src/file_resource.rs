//! Seekable, file-backed WIM resource ranges without loading the enclosing file.
//! Based on the framing rules in wimlib 1.14.5 `src/resource.c`. This reader
//! streams solid size-table prefixes through an eight-byte buffer. Ordinary
//! tables require only the selected chunks' boundaries. Input and decoded
//! scratch buffers are each bounded by one chunk; output is the requested range.
use crate::{Compression, ParseError, ResourceHeader, resource::ResourceLayout};
use alloc::vec::Vec;
use std::{
    fmt,
    io::{self, Read, Seek, SeekFrom},
    ops::Range,
};

/// One decoded solid chunk, scoped to a single input and consistent decoder policy.
/// Compressed bytes are reread and compared on every hit, including after mutation.
#[derive(Debug, Default)]
pub struct ChunkCache {
    key: Option<(u64, Compression, u32, u64)>,
    input: Vec<u8>,
    output: Vec<u8>,
}

/// A framing/codec failure or an underlying seek/read failure.
#[derive(Debug)]
pub enum FileReadError {
    /// WIM format, range, allocation, or codec failure.
    Format(ParseError),
    /// Original operating system error, retained for callers' diagnostics.
    Io(io::Error),
}
impl fmt::Display for FileReadError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Format(e) => e.fmt(f),
            Self::Io(e) => e.fmt(f),
        }
    }
}
impl std::error::Error for FileReadError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Format(e) => Some(e),
            Self::Io(e) => Some(e),
        }
    }
}
impl From<ParseError> for FileReadError {
    fn from(e: ParseError) -> Self {
        Self::Format(e)
    }
}
impl From<io::Error> for FileReadError {
    fn from(e: io::Error) -> Self {
        Self::Io(e)
    }
}

/// Read an uncompressed resource range from a seekable source.
///
/// The source cursor is changed. Empty selections perform framing validation but
/// do not decode or read chunk bodies. Returned bytes are not SHA-1 verified:
/// a partial range cannot prove the hash of its containing blob. The callback
/// receives only compressed chunks and may use additional codec workspace.
pub fn read_resource_range<R, F>(
    file: &mut R,
    header: &ResourceHeader,
    compression: Compression,
    chunk_size: u32,
    layout: ResourceLayout,
    selection: Range<u64>,
    decode: F,
) -> Result<Vec<u8>, FileReadError>
where
    R: Read + Seek,
    F: FnMut(Compression, &[u8], &mut [u8]) -> Result<(), ParseError>,
{
    read_resource_range_cached(
        file,
        header,
        compression,
        chunk_size,
        layout,
        selection,
        decode,
        None,
    )
}

/// Read resource bytes with optional bounded solid-chunk reuse.
/// The cache must belong to this input; callers must use a consistent decoder
/// policy, and must bypass the cache for corruption-recovery reads.
#[expect(
    clippy::too_many_arguments,
    reason = "Matches the resource reader plus its optional operation cache"
)]
pub fn read_resource_range_cached<R, F>(
    file: &mut R,
    header: &ResourceHeader,
    mut compression: Compression,
    mut chunk_size: u32,
    layout: ResourceLayout,
    selection: Range<u64>,
    mut decode: F,
    mut cache: Option<&mut ChunkCache>,
) -> Result<Vec<u8>, FileReadError>
where
    R: Read + Seek,
    F: FnMut(Compression, &[u8], &mut [u8]) -> Result<(), ParseError>,
{
    let file_size = file.seek(SeekFrom::End(0))?;
    if header.flags & (4 | 16) == 0 {
        validate_selection(&selection, header.uncompressed_size)?;
        let offset = add(header.offset_in_wim, selection.start)?;
        // Reject truncated raw input before reserving its advertised output size.
        check_span(file_size, offset, selection.end - selection.start)?;
        let mut output = allocate(selection.end - selection.start)?;
        read_at(file, file_size, offset, &mut output)?;
        return Ok(output);
    }
    check_span(file_size, header.offset_in_wim, header.size_in_wim)?;
    let mut uncompressed = header.uncompressed_size;
    let prefix = if layout == ResourceLayout::Solid {
        if header.size_in_wim < 16 {
            return Err(ParseError::UnexpectedEndOfFile.into());
        }
        let mut bytes = [0; 16];
        read_at(file, file_size, header.offset_in_wim, &mut bytes)?;
        let mut size = [0; 8];
        size.copy_from_slice(&bytes[..8]);
        uncompressed = u64::from_le_bytes(size);
        chunk_size = u32::from_le_bytes(
            bytes[8..12]
                .try_into()
                .map_err(|_| ParseError::Decompression)?,
        );
        compression = Compression::from_i32(
            (u32::from_le_bytes(
                bytes[12..16]
                    .try_into()
                    .map_err(|_| ParseError::Decompression)?,
            ) & 0x3f_ffff) as i32,
        )?;
        16
    } else {
        0
    };
    if !chunk_size.is_power_of_two() {
        return Err(ParseError::InvalidChunkSize.into());
    }
    if compression == Compression::None {
        return Err(ParseError::InvalidCompressionType.into());
    }
    validate_selection(&selection, uncompressed)?;
    if uncompressed == 0 {
        return Ok(Vec::new());
    }
    let chunk_size = u64::from(chunk_size);
    let chunks = uncompressed.div_ceil(chunk_size);
    let width = if uncompressed <= u64::from(u32::MAX) || layout == ResourceLayout::Solid {
        4
    } else {
        8
    };
    let entries = if layout == ResourceLayout::Solid {
        chunks
    } else {
        chunks - 1
    };
    let table_size = entries
        .checked_mul(width)
        .ok_or(ParseError::Decompression)?;
    let table_start = if layout == ResourceLayout::Pipable {
        header
            .size_in_wim
            .checked_sub(table_size)
            .ok_or(ParseError::Decompression)?
    } else {
        prefix
    };
    let data_start = if layout == ResourceLayout::Pipable {
        0
    } else {
        add(prefix, table_size)?
    };
    let framing_size = if layout == ResourceLayout::Pipable {
        chunks.checked_mul(4).ok_or(ParseError::Decompression)?
    } else {
        0
    };
    let data_size = header
        .size_in_wim
        .checked_sub(prefix)
        .and_then(|x| x.checked_sub(table_size))
        .and_then(|x| x.checked_sub(framing_size))
        .ok_or(ParseError::Decompression)?;
    if selection.is_empty() {
        return Ok(Vec::new());
    }
    let table_offset = add(header.offset_in_wim, table_start)?;
    let first = selection.start / chunk_size;
    let last = (selection.end - 1) / chunk_size;
    let mut offset = if layout == ResourceLayout::Solid {
        let mut sum = 0u64;
        for index in 0..first {
            sum = add(sum, entry(file, file_size, table_offset, index, width)?)?;
        }
        sum
    } else if first == 0 {
        0
    } else {
        entry(file, file_size, table_offset, first - 1, width)?
    };
    let mut output = allocate(selection.end - selection.start)?;
    for index in first..=last {
        let next = if index + 1 == chunks {
            data_size
        } else if layout == ResourceLayout::Solid {
            add(offset, entry(file, file_size, table_offset, index, width)?)?
        } else {
            entry(file, file_size, table_offset, index, width)?
        };
        let length = next.checked_sub(offset).ok_or(ParseError::Decompression)?;
        let out_start = index * chunk_size;
        let out_length = (uncompressed - out_start).min(chunk_size);
        if length == 0 || length > out_length {
            return Err(ParseError::Decompression.into());
        }
        let framing = if layout == ResourceLayout::Pipable {
            (index + 1)
                .checked_mul(4)
                .ok_or(ParseError::Decompression)?
        } else {
            0
        };
        let input_start = add(
            add(add(header.offset_in_wim, data_start)?, offset)?,
            framing,
        )?;
        // next is a resource-relative data offset, not an arbitrary file span.
        if next > data_size {
            return Err(ParseError::Decompression.into());
        }
        let overlap_start = out_start.max(selection.start);
        let overlap_end = (out_start + out_length).min(selection.end);
        let target_start =
            usize::try_from(overlap_start - selection.start).map_err(|_| ParseError::Nomem)?;
        let target_end =
            usize::try_from(overlap_end - selection.start).map_err(|_| ParseError::Nomem)?;
        if length == out_length {
            read_at(
                file,
                file_size,
                add(input_start, overlap_start - out_start)?,
                &mut output[target_start..target_end],
            )?;
        } else {
            let mut input = allocate(length)?;
            read_at(file, file_size, input_start, &mut input)?;
            if let Some(cache) = cache
                .as_deref_mut()
                .filter(|_| layout == ResourceLayout::Solid)
            {
                let key = (input_start, compression, chunk_size as u32, out_length);
                if cache.key != Some(key) || cache.input != input {
                    // Release the previous chunk before allocating the next one.
                    cache.key = None;
                    cache.input = Vec::new();
                    cache.output = Vec::new();
                    let mut decoded = allocate(out_length)?;
                    decode(compression, &input, &mut decoded)?;
                    cache.input = input;
                    cache.output = decoded;
                    cache.key = Some(key);
                }
                let local_start =
                    usize::try_from(overlap_start - out_start).map_err(|_| ParseError::Nomem)?;
                let local_end =
                    usize::try_from(overlap_end - out_start).map_err(|_| ParseError::Nomem)?;
                output[target_start..target_end]
                    .copy_from_slice(&cache.output[local_start..local_end]);
            } else if overlap_start == out_start && overlap_end == out_start + out_length {
                decode(compression, &input, &mut output[target_start..target_end])?;
            } else {
                let mut scratch = allocate(out_length)?;
                decode(compression, &input, &mut scratch)?;
                let local_start =
                    usize::try_from(overlap_start - out_start).map_err(|_| ParseError::Nomem)?;
                let local_end =
                    usize::try_from(overlap_end - out_start).map_err(|_| ParseError::Nomem)?;
                output[target_start..target_end].copy_from_slice(&scratch[local_start..local_end]);
            }
        }
        offset = next;
    }
    Ok(output)
}
fn add(a: u64, b: u64) -> Result<u64, ParseError> {
    a.checked_add(b).ok_or(ParseError::Decompression)
}
fn validate_selection(selection: &Range<u64>, size: u64) -> Result<(), ParseError> {
    if selection.start > selection.end || selection.end > size {
        Err(ParseError::InvalidParam)
    } else {
        Ok(())
    }
}
fn check_span(size: u64, offset: u64, length: u64) -> Result<(), ParseError> {
    if offset.checked_add(length).is_none_or(|end| end > size) {
        Err(ParseError::UnexpectedEndOfFile)
    } else {
        Ok(())
    }
}
fn read_at<R: Read + Seek>(
    file: &mut R,
    size: u64,
    offset: u64,
    out: &mut [u8],
) -> Result<(), FileReadError> {
    check_span(size, offset, out.len() as u64)?;
    file.seek(SeekFrom::Start(offset))?;
    file.read_exact(out)?;
    Ok(())
}
fn allocate(size: u64) -> Result<Vec<u8>, ParseError> {
    let size = usize::try_from(size).map_err(|_| ParseError::Nomem)?;
    let mut bytes = Vec::new();
    bytes
        .try_reserve_exact(size)
        .map_err(|_| ParseError::Nomem)?;
    bytes.resize(size, 0);
    Ok(bytes)
}
fn entry<R: Read + Seek>(
    file: &mut R,
    size: u64,
    table: u64,
    index: u64,
    width: u64,
) -> Result<u64, FileReadError> {
    let offset = add(
        table,
        index.checked_mul(width).ok_or(ParseError::Decompression)?,
    )?;
    let mut bytes = [0; 8];
    read_at(file, size, offset, &mut bytes[..width as usize])?;
    Ok(u64::from_le_bytes(bytes))
}
