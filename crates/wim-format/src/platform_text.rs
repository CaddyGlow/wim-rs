//! Lossless wimlib platform-text conversions, including unpaired surrogates.
//! Upstream encoding.c accepts WTF-8 rather than strict Unicode UTF-8.
use crate::ParseError;
use crate::allocation::*;
use alloc::vec::Vec;
/// Encode UTF-16 code units as WTF-8, preserving every unpaired surrogate.
pub fn utf16_to_wtf8(input: &[u16]) -> Result<Vec<u8>, ParseError> {
    let mut output = Vec::new();
    output
        .try_reserve(input.len().checked_mul(3).ok_or(ParseError::Nomem)?)
        .map_err(|_| ParseError::Nomem)?;
    encode_utf16(input.iter().copied(), |bytes| {
        output.extend_from_slice(bytes);
        Ok(())
    })?;
    Ok(output)
}
fn encode_utf16(
    input: impl Iterator<Item = u16>,
    mut emit: impl FnMut(&[u8]) -> Result<(), ParseError>,
) -> Result<(), ParseError> {
    let mut input = input.peekable();
    while let Some(unit) = input.next() {
        let mut code = u32::from(unit);
        if (0xd800..0xdc00).contains(&code)
            && let Some(&low) = input.peek()
            && (0xdc00..0xe000).contains(&low)
        {
            code = 0x10000 + ((code - 0xd800) << 10) + u32::from(low - 0xdc00);
            input.next();
        }
        if code < 0x80 {
            emit(&[code as u8])?;
        } else if code < 0x800 {
            emit(&[(0xc0 | (code >> 6)) as u8, (0x80 | (code & 63)) as u8])?;
        } else if code < 0x10000 {
            emit(&[
                (0xe0 | (code >> 12)) as u8,
                (0x80 | ((code >> 6) & 63)) as u8,
                (0x80 | (code & 63)) as u8,
            ])?;
        } else {
            emit(&[
                (0xf0 | (code >> 18)) as u8,
                (0x80 | ((code >> 12) & 63)) as u8,
                (0x80 | ((code >> 6) & 63)) as u8,
                (0x80 | (code & 63)) as u8,
            ])?;
        }
    }
    Ok(())
}
fn convert_utf16le(input: &[u8], terminated: bool) -> Result<Vec<u8>, ParseError> {
    if !input.len().is_multiple_of(2) {
        return Err(ParseError::InvalidUtf16String);
    }
    let units = || {
        input
            .chunks_exact(2)
            .map(|bytes| u16::from_le_bytes([bytes[0], bytes[1]]))
    };
    let mut length = usize::from(terminated);
    encode_utf16(units(), |bytes| {
        length = length.checked_add(bytes.len()).ok_or(ParseError::Nomem)?;
        Ok(())
    })?;
    let mut output = Vec::new();
    output.try_reserve(length).map_err(|_| ParseError::Nomem)?;
    encode_utf16(units(), |bytes| {
        output
            .try_extend_from_slice(bytes)
            .map_err(|_| ParseError::Nomem)
    })?;
    if terminated {
        output.try_push(0).map_err(|_| ParseError::Nomem)?;
    }
    Ok(output)
}
/// Convert little-endian UTF-16 directly to allocated WTF-8 bytes.
/// Unpaired surrogates are preserved; intermediate unit vectors are unnecessary.
pub fn utf16le_to_wtf8(input: &[u8]) -> Result<Vec<u8>, ParseError> {
    convert_utf16le(input, false)
}
/// Convert little-endian UTF-16 to an allocated terminated WTF-8 string.
pub fn utf16le_to_wtf8z(input: &[u8]) -> Result<Vec<u8>, ParseError> {
    convert_utf16le(input, true)
}
/// Decode original-library WTF-8, rejecting overlong/truncated/out-of-range sequences.
pub fn wtf8_to_utf16(input: &[u8]) -> Result<Vec<u16>, ParseError> {
    let mut output = Vec::new();
    output
        .try_reserve(input.len())
        .map_err(|_| ParseError::Nomem)?;
    decode_wtf8(input, |unit| {
        output.push(unit);
        Ok(())
    })?;
    Ok(output)
}
fn decode_wtf8(
    input: &[u8],
    mut emit: impl FnMut(u16) -> Result<(), ParseError>,
) -> Result<(), ParseError> {
    let mut index = 0;
    while index < input.len() {
        let first = input[index];
        let (count, mut code, minimum) = match first {
            0..=0x7f => (1, u32::from(first), 0),
            0xc2..=0xdf => (2, u32::from(first & 31), 0x80),
            0xe0..=0xef => (3, u32::from(first & 15), 0x800),
            0xf0..=0xf7 => (4, u32::from(first & 7), 0x10000),
            _ => return Err(ParseError::InvalidUtf8String),
        };
        let sequence = input
            .get(index..index + count)
            .ok_or(ParseError::InvalidUtf8String)?;
        for &tail in &sequence[1..] {
            if tail & 0xc0 != 0x80 {
                return Err(ParseError::InvalidUtf8String);
            }
            code = (code << 6) | u32::from(tail & 63);
        }
        if code < minimum || code > 0x10ffff {
            return Err(ParseError::InvalidUtf8String);
        }
        if code < 0x10000 {
            emit(code as u16)?
        } else {
            let code = code - 0x10000;
            emit(0xd800 + (code >> 10) as u16)?;
            emit(0xdc00 + (code & 1023) as u16)?;
        }
        index += count;
    }
    Ok(())
}

/// Convert validated WTF-8 to an owned terminated UTF-16 platform string.
/// Validation and exact size computation precede the actual fallible allocation.
pub fn wtf8_to_utf16z(input: &[u8]) -> Result<Vec<u16>, ParseError> {
    let mut length = 1usize;
    decode_wtf8(input, |_| {
        length = length.checked_add(1).ok_or(ParseError::Nomem)?;
        Ok(())
    })?;
    let mut output = Vec::new();
    output.try_reserve(length).map_err(|_| ParseError::Nomem)?;
    decode_wtf8(input, |unit| {
        output.try_push(unit).map_err(|_| ParseError::Nomem)
    })?;
    output.try_push(0).map_err(|_| ParseError::Nomem)?;
    Ok(output)
}
