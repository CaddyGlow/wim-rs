//! Whole-resource decoding of ordinary, seekable pipable, and solid WIM layouts.
//! Codec implementations are supplied by the caller. This is not pipe streaming
//! and does not implement recover-data extraction.

use crate::{Compression, ParseError, ResourceHeader};
use alloc::vec::Vec;
use core::ops::Range;

/// Read an uncompressed byte range, decoding only chunks intersecting it.
/// Reader scratch memory is at most one chunk in addition to the returned range;
/// the decoder callback may require additional codec workspace.
pub fn read_resource_range<F>(
    file: &[u8],
    header: &ResourceHeader,
    compression: Compression,
    chunk_size: u32,
    layout: ResourceLayout,
    selection: Range<u64>,
    decode: F,
) -> Result<Vec<u8>, ParseError>
where
    F: FnMut(Compression, &[u8], &mut [u8]) -> Result<(), ParseError>,
{
    read_selected(
        file,
        header,
        compression,
        chunk_size,
        layout,
        Some(selection),
        decode,
    )
}

/// Compressed resource framing selected by the enclosing WIM/blob table.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ResourceLayout {
    /// Offset table precedes the chunk data.
    Ordinary,
    /// Per-chunk size headers precede data; offset table follows the data.
    Pipable,
    /// Alternate 16-byte header and table of per-chunk sizes precede data.
    Solid,
}
/// Read a complete resource from a seekable file represented by a byte slice.
/// The decoder receives the exact output chunk length, including a partial last
/// chunk. Chunks stored raw (compressed size equals output size) bypass it.
/// Resource flags select compression; layout must reflect the enclosing format.
pub fn read_resource<F>(
    file: &[u8],
    header: &ResourceHeader,
    compression: Compression,
    chunk_size: u32,
    layout: ResourceLayout,
    decode: F,
) -> Result<Vec<u8>, ParseError>
where
    F: FnMut(Compression, &[u8], &mut [u8]) -> Result<(), ParseError>,
{
    read_selected(file, header, compression, chunk_size, layout, None, decode)
}
fn read_selected<F>(
    file: &[u8],
    header: &ResourceHeader,
    mut compression: Compression,
    mut chunk_size: u32,
    layout: ResourceLayout,
    selected: Option<Range<u64>>,
    mut decode: F,
) -> Result<Vec<u8>, ParseError>
where
    F: FnMut(Compression, &[u8], &mut [u8]) -> Result<(), ParseError>,
{
    if header.flags & (4 | 16) == 0 {
        let selected = validate_selection(selected, header.uncompressed_size)?;
        let offset = header
            .offset_in_wim
            .checked_add(selected.start)
            .ok_or(ParseError::UnexpectedEndOfFile)?;
        return copy_bytes(range(file, offset, selected.end - selected.start)?);
    }
    let bytes = range(file, header.offset_in_wim, header.size_in_wim)?;
    let mut uncompressed = header.uncompressed_size;
    let prefix = if layout == ResourceLayout::Solid {
        let h = range(bytes, 0, 16)?;
        uncompressed = read_u64(h, 0);
        chunk_size = read_u32(h, 8);
        compression = Compression::from_i32((read_u32(h, 12) & 0x3f_ffff) as i32)?;
        16u64
    } else {
        0
    };
    if !chunk_size.is_power_of_two() {
        return Err(ParseError::InvalidChunkSize);
    }
    // Upstream compressed reader validates power-of-two; codec construction
    // validates the algorithm. Header policy chunk bounds belong to open_wim.
    if compression == Compression::None {
        return Err(ParseError::InvalidCompressionType);
    }
    let selected = validate_selection(selected, uncompressed)?;
    if uncompressed == 0 {
        return Ok(Vec::new());
    }
    let chunks = uncompressed.div_ceil(u64::from(chunk_size));
    let entry_size = if uncompressed <= u64::from(u32::MAX) || layout == ResourceLayout::Solid {
        4u64
    } else {
        8
    };
    let entries = if layout == ResourceLayout::Solid {
        chunks
    } else {
        chunks - 1
    };
    let table_size = entries
        .checked_mul(entry_size)
        .ok_or(ParseError::Decompression)?;
    let table_start = if layout == ResourceLayout::Pipable {
        header
            .size_in_wim
            .checked_sub(table_size)
            .ok_or(ParseError::Decompression)?
    } else {
        prefix
    };
    let table = range(bytes, table_start, table_size)?;
    let data_start = if layout == ResourceLayout::Pipable {
        0
    } else {
        prefix
            .checked_add(table_size)
            .ok_or(ParseError::Decompression)?
    };
    let data_size = header
        .size_in_wim
        .checked_sub(prefix)
        .and_then(|x| x.checked_sub(table_size))
        .and_then(|x| {
            if layout == ResourceLayout::Pipable {
                x.checked_sub(chunks.checked_mul(4)?)
            } else {
                Some(x)
            }
        })
        .ok_or(ParseError::Decompression)?;
    // Validate every span before reserving attacker-controlled output sizes.
    let mut checked_offset = 0u64;
    for i in 0..chunks {
        let next = if i + 1 == chunks {
            data_size
        } else if layout == ResourceLayout::Solid {
            checked_offset
                .checked_add(table_entry(table, i, 4)?)
                .ok_or(ParseError::Decompression)?
        } else {
            table_entry(table, i, entry_size)?
        };
        let length = next
            .checked_sub(checked_offset)
            .ok_or(ParseError::Decompression)?;
        let output_length = (uncompressed - i * u64::from(chunk_size)).min(u64::from(chunk_size));
        if length == 0 || length > output_length {
            return Err(ParseError::Decompression);
        }
        let framing = if layout == ResourceLayout::Pipable {
            (i + 1).checked_mul(4).ok_or(ParseError::Decompression)?
        } else {
            0
        };
        let start = data_start
            .checked_add(checked_offset)
            .and_then(|x| x.checked_add(framing))
            .ok_or(ParseError::Decompression)?;
        range(bytes, start, length)?;
        checked_offset = next;
    }
    let output_size =
        usize::try_from(selected.end - selected.start).map_err(|_| ParseError::Nomem)?;
    let mut output = Vec::new();
    output
        .try_reserve_exact(output_size)
        .map_err(|_| ParseError::Nomem)?;
    output.resize(output_size, 0);
    let mut offset = 0u64;
    for i in 0..chunks {
        let next = if i + 1 == chunks {
            data_size
        } else if layout == ResourceLayout::Solid {
            offset
                .checked_add(table_entry(table, i, 4)?)
                .ok_or(ParseError::Decompression)?
        } else {
            table_entry(table, i, entry_size)?
        };
        let length = next.checked_sub(offset).ok_or(ParseError::Decompression)?;
        let out_start = i
            .checked_mul(u64::from(chunk_size))
            .ok_or(ParseError::Decompression)?;
        let out_length = (uncompressed - out_start).min(u64::from(chunk_size));
        if length == 0 || length > out_length {
            return Err(ParseError::Decompression);
        }
        let framing = if layout == ResourceLayout::Pipable {
            (i + 1).checked_mul(4).ok_or(ParseError::Decompression)?
        } else {
            0
        };
        let input_start = data_start
            .checked_add(offset)
            .and_then(|x| x.checked_add(framing))
            .ok_or(ParseError::Decompression)?;
        let overlap_start = out_start.max(selected.start);
        let overlap_end = (out_start + out_length).min(selected.end);
        if overlap_start < overlap_end {
            let input = range(bytes, input_start, length)?;
            let target_start =
                usize::try_from(overlap_start - selected.start).map_err(|_| ParseError::Nomem)?;
            let target_end =
                usize::try_from(overlap_end - selected.start).map_err(|_| ParseError::Nomem)?;
            let local_start = (overlap_start - out_start) as usize;
            let local_end = (overlap_end - out_start) as usize;
            if length == out_length {
                output[target_start..target_end].copy_from_slice(&input[local_start..local_end]);
            } else if overlap_start == out_start && overlap_end == out_start + out_length {
                decode(compression, input, &mut output[target_start..target_end])?;
            } else {
                let size = usize::try_from(out_length).map_err(|_| ParseError::Nomem)?;
                let mut scratch = Vec::new();
                scratch
                    .try_reserve_exact(size)
                    .map_err(|_| ParseError::Nomem)?;
                scratch.resize(size, 0);
                decode(compression, input, &mut scratch)?;
                output[target_start..target_end].copy_from_slice(&scratch[local_start..local_end]);
            }
        }
        offset = next;
    }
    Ok(output)
}
fn range(bytes: &[u8], offset: u64, size: u64) -> Result<&[u8], ParseError> {
    let end = offset
        .checked_add(size)
        .ok_or(ParseError::UnexpectedEndOfFile)?;
    let start = usize::try_from(offset).map_err(|_| ParseError::UnexpectedEndOfFile)?;
    let end = usize::try_from(end).map_err(|_| ParseError::UnexpectedEndOfFile)?;
    bytes.get(start..end).ok_or(ParseError::UnexpectedEndOfFile)
}
fn copy_bytes(bytes: &[u8]) -> Result<Vec<u8>, ParseError> {
    let mut v = Vec::new();
    v.try_reserve_exact(bytes.len())
        .map_err(|_| ParseError::Nomem)?;
    v.extend_from_slice(bytes);
    Ok(v)
}
fn read_u32(b: &[u8], i: usize) -> u32 {
    u32::from_le_bytes([b[i], b[i + 1], b[i + 2], b[i + 3]])
}
fn read_u64(b: &[u8], i: usize) -> u64 {
    let mut a = [0; 8];
    a.copy_from_slice(&b[i..i + 8]);
    u64::from_le_bytes(a)
}
fn table_entry(bytes: &[u8], i: u64, width: u64) -> Result<u64, ParseError> {
    let b = range(
        bytes,
        i.checked_mul(width).ok_or(ParseError::Decompression)?,
        width,
    )?;
    Ok(if width == 4 {
        u64::from(read_u32(b, 0))
    } else {
        read_u64(b, 0)
    })
}

fn validate_selection(selected: Option<Range<u64>>, size: u64) -> Result<Range<u64>, ParseError> {
    let selected = selected.unwrap_or(0..size);
    if selected.start > selected.end || selected.end > size {
        return Err(ParseError::InvalidParam);
    }
    Ok(selected)
}
