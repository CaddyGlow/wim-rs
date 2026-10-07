// SPDX-License-Identifier: LGPL-2.1-or-later OR GPL-3.0-or-later
// Resource framing follows wimlib src/write.c at
// cd5e231c348c255ae5088873b5a66ee0eb96fa07 (Eric Biggers and contributors).
//! Native framing for WIM resource output; compression is supplied by the caller.
use crate::{Compression, ParseError, ResourceHeader, resource::ResourceLayout};
#[cfg(test)]
use alloc::vec;
use alloc::vec::Vec;

/// Resource bytes and a descriptor whose offset is relative to these bytes.
#[derive(Debug, PartialEq, Eq)]
pub struct EncodedResource {
    /// Serialized table, chunk framing, and data, excluding pipable blob headers.
    pub bytes: Vec<u8>,
    /// Resource descriptor; callers relocate its zero offset into their archive.
    pub header: ResourceHeader,
}
/// Framing retained after streamed chunks; chunk payloads are owned by the sink.
#[derive(Debug, PartialEq, Eq)]
pub struct StreamedResource {
    /// Ordinary chunk table or solid header/table, placed before payloads.
    pub prefix: Vec<u8>,
    /// Pipable chunk table, placed after payloads.
    pub suffix: Vec<u8>,
    /// Descriptor for the complete framing and payload, with zero archive offset.
    pub header: ResourceHeader,
}
/// Serialize a complete resource in upstream's ordinary, pipable, or solid format.
///
/// The callback returns compressed data or `None` to store the chunk verbatim.
/// Empty or nonshrinking compressed results also fall back to verbatim storage.
/// `None` compression and empty resources have no chunk table or compression flag.
/// This buffers the resource; streaming and archive blob headers are separate layers.
pub fn encode_resource<F>(
    input: &[u8],
    compression: Compression,
    chunk_size: u32,
    layout: ResourceLayout,
    encode: F,
) -> Result<EncodedResource, ParseError>
where
    F: FnMut(Compression, &[u8]) -> Result<Option<Vec<u8>>, ParseError>,
{
    encode_resource_with_progress(input, compression, chunk_size, layout, encode, |_, _, _| {
        Ok(())
    })
}

/// Encode resources with a completion hook after each stored chunk.
/// The hook receives stored bytes, uncompressed size, and final-chunk status;
/// returning an error cancels before encoding or appending later chunks.
pub fn encode_resource_with_progress<F, P>(
    input: &[u8],
    compression: Compression,
    chunk_size: u32,
    layout: ResourceLayout,
    encode: F,
    progress: P,
) -> Result<EncodedResource, ParseError>
where
    F: FnMut(Compression, &[u8]) -> Result<Option<Vec<u8>>, ParseError>,
    P: FnMut(&[u8], usize, bool) -> Result<(), ParseError>,
{
    encode_resource_from_chunks(
        input.len(),
        compression,
        chunk_size,
        layout,
        |range| Ok(alloc::borrow::Cow::Borrowed(&input[range])),
        encode,
        progress,
    )
}
/// Encode lazily supplied chunks, stopping reads and compression on cancellation.
/// The reader must return the exact requested range length. Completion hooks
/// run after storing each actual encoded chunk and before reading the next.
pub fn encode_resource_from_chunks<'a, R, F, P>(
    input_size: usize,
    compression: Compression,
    chunk_size: u32,
    layout: ResourceLayout,
    read: R,
    encode: F,
    mut progress: P,
) -> Result<EncodedResource, ParseError>
where
    R: FnMut(core::ops::Range<usize>) -> Result<alloc::borrow::Cow<'a, [u8]>, ParseError>,
    F: FnMut(Compression, &[u8]) -> Result<Option<Vec<u8>>, ParseError>,
    P: FnMut(&[u8], usize, bool) -> Result<(), ParseError>,
{
    let mut data = Vec::new();
    let framing = stream_resource_from_chunks(
        input_size,
        compression,
        chunk_size,
        layout,
        read,
        encode,
        |stored, size, last| {
            if compression != Compression::None
                && input_size != 0
                && layout == ResourceLayout::Pipable
            {
                append(&mut data, &(stored.len() as u32).to_le_bytes())?;
            }
            append(&mut data, stored)?;
            progress(stored, size, last)
        },
    )?;
    let mut bytes = framing.prefix;
    append(&mut bytes, &data)?;
    append(&mut bytes, &framing.suffix)?;
    Ok(EncodedResource {
        bytes,
        header: framing.header,
    })
}

/// Encode into a chunk sink while retaining only the framing table.
/// The sink must store each chunk (and its u32 size prefix for pipable compressed
/// resources), then run completion callbacks before returning. On success the
/// caller patches `prefix` and appends `suffix`. Errors stop before later reads.
pub fn stream_resource_from_chunks<'a, R, F, P>(
    input_size: usize,
    compression: Compression,
    chunk_size: u32,
    layout: ResourceLayout,
    mut read: R,
    mut encode: F,
    mut store: P,
) -> Result<StreamedResource, ParseError>
where
    R: FnMut(core::ops::Range<usize>) -> Result<alloc::borrow::Cow<'a, [u8]>, ParseError>,
    F: FnMut(Compression, &[u8]) -> Result<Option<Vec<u8>>, ParseError>,
    P: FnMut(&[u8], usize, bool) -> Result<(), ParseError>,
{
    if compression == Compression::None || input_size == 0 {
        for start in (0..input_size).step_by(32768) {
            let end = (start + 32768).min(input_size);
            let chunk = read(start..end)?;
            if chunk.len() != end - start {
                return Err(ParseError::InvalidParam);
            }
            store(&chunk, chunk.len(), end == input_size)?;
        }
        return finish_stream(Vec::new(), Vec::new(), input_size as u64, input_size, 0);
    }
    if !chunk_size.is_power_of_two() {
        return Err(ParseError::InvalidChunkSize);
    }
    let chunk_size = usize::try_from(chunk_size).map_err(|_| ParseError::InvalidChunkSize)?;
    let count = input_size.div_ceil(chunk_size);
    let mut table = Vec::new();
    let width = if input_size as u64 <= u64::from(u32::MAX) || layout == ResourceLayout::Solid {
        4usize
    } else {
        8
    };
    let entries = if layout == ResourceLayout::Solid {
        count
    } else {
        count - 1
    };
    table
        .try_reserve_exact(entries.checked_mul(width).ok_or(ParseError::Nomem)?)
        .map_err(|_| ParseError::Nomem)?;
    let mut offset = 0u64;
    for i in 0..count {
        let start = i * chunk_size;
        let end = (start + chunk_size).min(input_size);
        let chunk = read(start..end)?;
        if chunk.len() != end - start {
            return Err(ParseError::InvalidParam);
        }
        let compressed = encode(compression, &chunk)?;
        let stored = compressed
            .as_deref()
            .filter(|bytes| !bytes.is_empty() && bytes.len() < chunk.len())
            .unwrap_or(&chunk);
        let size = u32::try_from(stored.len()).map_err(|_| ParseError::InvalidChunkSize)?;
        store(stored, chunk.len(), i + 1 == count)?;
        offset = offset
            .checked_add(u64::from(size))
            .ok_or(ParseError::Nomem)?;
        if layout == ResourceLayout::Solid {
            append(&mut table, &size.to_le_bytes())?;
        } else if i + 1 < count {
            if width == 4 {
                append(&mut table, &(offset as u32).to_le_bytes())?;
            } else {
                append(&mut table, &offset.to_le_bytes())?;
            }
        }
    }
    let mut prefix = Vec::new();
    if layout == ResourceLayout::Solid {
        append(&mut prefix, &(input_size as u64).to_le_bytes())?;
        append(&mut prefix, &(chunk_size as u32).to_le_bytes())?;
        append(&mut prefix, &(compression as u32).to_le_bytes())?;
    }
    let suffix = if layout == ResourceLayout::Pipable {
        offset = offset
            .checked_add((count as u64).checked_mul(4).ok_or(ParseError::Nomem)?)
            .ok_or(ParseError::Nomem)?;
        table
    } else {
        append(&mut prefix, &table)?;
        Vec::new()
    };
    finish_stream(
        prefix,
        suffix,
        offset,
        input_size,
        if layout == ResourceLayout::Solid {
            16
        } else {
            4
        },
    )
}
fn append(out: &mut Vec<u8>, bytes: &[u8]) -> Result<(), ParseError> {
    out.try_reserve(bytes.len())
        .map_err(|_| ParseError::Nomem)?;
    out.extend_from_slice(bytes);
    Ok(())
}
fn finish_stream(
    prefix: Vec<u8>,
    suffix: Vec<u8>,
    payload: u64,
    uncompressed: usize,
    flags: u8,
) -> Result<StreamedResource, ParseError> {
    let size = payload
        .checked_add(prefix.len() as u64)
        .and_then(|size| size.checked_add(suffix.len() as u64))
        .ok_or(ParseError::Nomem)?;
    if size >= 1u64 << 56 {
        return Err(ParseError::InvalidParam);
    }
    Ok(StreamedResource {
        header: ResourceHeader {
            size_in_wim: size,
            flags,
            offset_in_wim: 0,
            uncompressed_size: uncompressed as u64,
        },
        prefix,
        suffix,
    })
}

#[cfg(test)]
mod progress_tests {
    use super::*;
    #[test]
    fn streamed_framing_matches_independent_tables_and_fallback_chunks() {
        let input = b"abcdefghij";
        for layout in [
            ResourceLayout::Ordinary,
            ResourceLayout::Pipable,
            ResourceLayout::Solid,
        ] {
            let mut payload = Vec::new();
            let framing = stream_resource_from_chunks(
                input.len(),
                Compression::Xpress,
                4,
                layout,
                |range| Ok(alloc::borrow::Cow::Borrowed(&input[range])),
                |_, chunk| {
                    Ok(Some(if chunk[0] == b'a' {
                        vec![1, 2]
                    } else {
                        chunk.to_vec()
                    }))
                },
                |stored, _, _| {
                    if layout == ResourceLayout::Pipable {
                        payload.extend_from_slice(&(stored.len() as u32).to_le_bytes());
                    }
                    payload.extend_from_slice(stored);
                    Ok(())
                },
            )
            .unwrap();
            let cumulative = [2u32, 6]
                .into_iter()
                .flat_map(u32::to_le_bytes)
                .collect::<Vec<_>>();
            if layout == ResourceLayout::Pipable {
                assert!(framing.prefix.is_empty());
                assert_eq!(framing.suffix, cumulative);
                assert_eq!(
                    payload,
                    [
                        2, 0, 0, 0, 1, 2, 4, 0, 0, 0, b'e', b'f', b'g', b'h', 2, 0, 0, 0, b'i',
                        b'j'
                    ]
                );
            } else {
                assert!(framing.suffix.is_empty());
                assert_eq!(payload, [1, 2, b'e', b'f', b'g', b'h', b'i', b'j']);
                let expected = if layout == ResourceLayout::Solid {
                    let mut prefix = 10u64.to_le_bytes().to_vec();
                    prefix.extend_from_slice(&4u32.to_le_bytes());
                    prefix.extend_from_slice(&(Compression::Xpress as u32).to_le_bytes());
                    prefix.extend([2u32, 4, 2].into_iter().flat_map(u32::to_le_bytes));
                    prefix
                } else {
                    cumulative
                };
                assert_eq!(framing.prefix, expected);
            }
            assert_eq!(
                framing.header.size_in_wim as usize,
                framing.prefix.len() + payload.len() + framing.suffix.len()
            );
        }
    }

    #[test]
    fn failed_chunk_sink_stops_later_reads_and_compression() {
        let mut reads = 0;
        let mut encodes = 0;
        let result = stream_resource_from_chunks(
            12,
            Compression::Xpress,
            4,
            ResourceLayout::Solid,
            |_| {
                reads += 1;
                Ok(alloc::borrow::Cow::Owned(vec![0; 4]))
            },
            |_, _| {
                encodes += 1;
                Ok(None)
            },
            |_, _, _| Err(ParseError::AbortedByProgress),
        );
        assert_eq!(result, Err(ParseError::AbortedByProgress));
        assert_eq!((reads, encodes), (1, 1));
    }

    #[test]
    fn cancellation_stops_before_later_chunks_are_compressed() {
        let input = vec![42; 98304];
        let mut encoded = 0;
        let mut completed = 0;
        let result = encode_resource_with_progress(
            &input,
            Compression::Xpress,
            32768,
            ResourceLayout::Ordinary,
            |_, _| {
                encoded += 1;
                Ok(Some(vec![7; 10]))
            },
            |stored, size, last| {
                assert_eq!(stored.len(), 10);
                assert_eq!(size, 32768);
                assert!(!last);
                completed += 1;
                Err(ParseError::AbortedByProgress)
            },
        );
        assert_eq!(result, Err(ParseError::AbortedByProgress));
        assert_eq!((encoded, completed), (1, 1));
    }
    #[test]
    fn raw_resources_report_real_buffer_boundaries_and_terminal_chunk() {
        let input = vec![42; 65537];
        let mut events = Vec::new();
        let encoded = encode_resource_with_progress(
            &input,
            Compression::None,
            0,
            ResourceLayout::Ordinary,
            |_, _| panic!("raw resource must not compress"),
            |stored, size, last| {
                events.push((stored.len(), size, last));
                Ok(())
            },
        )
        .unwrap();
        assert_eq!(encoded.bytes, input);
        assert_eq!(
            events,
            [(32768, 32768, false), (32768, 32768, false), (1, 1, true)]
        );
    }
}
