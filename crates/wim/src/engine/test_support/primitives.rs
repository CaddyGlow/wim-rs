// SPDX-License-Identifier: LGPL-2.1-or-later
//! Bounded source-derived building blocks of the optional generated tree.
//! Temporary vectors use the Rust global allocator.
use super::random::Random;
use wim_format::ParseError;

fn zeros(size: usize) -> Result<Vec<u8>, ParseError> {
    let mut bytes = Vec::new();
    bytes
        .try_reserve_exact(size)
        .map_err(|_| ParseError::Nomem)?;
    bytes.resize(size, 0);
    Ok(bytes)
}

/// Generate a timestamp with the original platform-compatible range rules.
pub fn timestamp(random: &mut Random) -> u64 {
    let value = if random.next_bool() {
        random.next_u64()
    } else {
        (random.next_u64() % (1 << 34) + 11_644_473_600) * 10_000_000
    };
    (value % (1 << 63)).max(1)
}

/// Generate a UTF-16LE filename without its terminating code unit.
pub fn filename(random: &mut Random, maximum: usize) -> Result<Vec<u8>, ParseError> {
    let size = match random.next_u32() % 8 {
        0 | 1 => 1 + random.next_u32() % 6,
        2..=4 => 7 + random.next_u32() % 8,
        5 | 6 => 15 + random.next_u32() % 15,
        _ => 30 + random.next_u32() % 90,
    };
    let mut name = zeros((size as usize).min(maximum) * 2)?;
    loop {
        for unit in name.chunks_exact_mut(2) {
            let value = loop {
                let value = random.next_u16();
                #[cfg(windows)]
                let valid = valid_windows_char(value);
                #[cfg(not(windows))]
                let valid = value != 0 && value != 47;
                if valid {
                    break value;
                }
            };
            unit.copy_from_slice(&value.to_le_bytes());
        }
        if name != [46, 0] && name != [46, 0, 46, 0] {
            return Ok(name);
        }
    }
}

/// Whether a UTF-16 code unit is accepted in the original Windows namespace.
pub fn valid_windows_char(value: u16) -> bool {
    value > 31 && ![47, 60, 62, 58, 34, 92, 124, 63, 42].contains(&value)
}

/// Generate an 8.3 name in the original alphabet and random draw order.
pub fn short_name(random: &mut Random) -> Result<Vec<u8>, ParseError> {
    const CHARS: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZ0123456789!#$%&'()-@^_`{}~";
    let base = 1 + random.next_u32() % 8;
    let extension = random.next_u32() % 4;
    let mut name = zeros((base + if extension == 0 { 0 } else { extension + 1 }) as usize * 2)?;
    let mut unit = 0;
    for _ in 0..base {
        name[unit] = CHARS[random.next_u32() as usize % CHARS.len()];
        unit += 2;
    }
    if extension != 0 {
        name[unit] = b'.';
        unit += 2;
        for _ in 0..extension {
            name[unit] = CHARS[random.next_u32() as usize % CHARS.len()];
            unit += 2;
        }
    }
    Ok(name)
}

/// Choose a stream length, consuming no draw in metadata-only mode.
pub fn stream_size(random: &mut Random, metadata_only: bool) -> usize {
    if metadata_only {
        return 0;
    }
    let bound = match random.next_u32() % 2048 {
        600..=799 => 64,
        800..=1319 => 4096,
        1320..=1799 => 32768,
        1800..=2046 => 262144,
        2047 => 134217728,
        _ => return 0,
    };
    (random.next_u32() % bound) as usize
}

/// Fill bytes using the original sparse fills, waves and zero regions.
/// Unlike a normal empty data stream, an empty reparse stream calls this and
/// consumes the initial byte-fill-count draw.
pub fn data(random: &mut Random, size: usize) -> Result<Vec<u8>, ParseError> {
    let mut bytes = zeros(size)?;
    let fills = random.next_u32() as usize % 256;
    if size == 0 {
        return Ok(bytes);
    }
    bytes.fill(random.next_u32() as u8);
    let mut mask = usize::MAX;
    for _ in 0..fills {
        let byte = random.next_u8();
        let count = ((size as f64 / fills as f64) * (f64::from(random.next_u32()) / 2e9)) as usize;
        let offset = random.next_u32() as usize & !mask;
        for _ in 0..count {
            bytes[(offset + (random.next_u32() as usize & mask)) % size] = byte;
        }
        if random.next_u32().is_multiple_of(4) {
            mask = usize::MAX << (random.next_u32() % 4);
        }
    }
    if random.next_u32().is_multiple_of(8) {
        let magnitude = f64::from(random.next_u32() % 128);
        let scale = 1.0 / f64::from(1 + random.next_u32() % 256);
        for (index, byte) in bytes.iter_mut().enumerate() {
            *byte = byte.wrapping_add((magnitude * (index as f64 * scale).cos()) as i32 as u8);
        }
    }
    if random.next_u32().is_multiple_of(4) {
        let holes = 1 + random.next_u32() % 16;
        for _ in 0..holes {
            let offset = random.next_u32() as usize % size;
            let len = (size - offset).min(size / (1 + random.next_u32() % 16) as usize);
            bytes[offset..offset + len].fill(0);
        }
    }
    Ok(bytes)
}

fn sid(random: &mut Random) -> Vec<u8> {
    const COMMON: &[(u64, &[u32])] = &[
        (0, &[0]),
        (1, &[0]),
        (2, &[0]),
        (3, &[0]),
        (3, &[1]),
        (3, &[2]),
        (3, &[3]),
        (5, &[1]),
        (5, &[2]),
        (5, &[3]),
        (5, &[4]),
        (5, &[6]),
        (5, &[7]),
        (5, &[8]),
        (5, &[9]),
        (5, &[10]),
        (5, &[11]),
        (5, &[12]),
        (5, &[13]),
        (5, &[18]),
        (5, &[19]),
        (5, &[20]),
        (
            80,
            &[956008885, 3418522649, 1831038044, 1853292631, 2271478464],
        ),
        (5, &[32, 544]),
    ];
    let draw = random.next_u32();
    let mut bytes = vec![1, 0];
    if draw & 1 != 0 {
        let (authority, subs) = COMMON[(draw >> 1) as usize % COMMON.len()];
        bytes[1] = subs.len() as u8;
        bytes.extend_from_slice(&authority.to_be_bytes()[2..]);
        for sub in subs {
            bytes.extend_from_slice(&sub.to_le_bytes());
        }
    } else {
        let count = 1 + ((draw >> 1) % 15);
        bytes[1] = count as u8;
        for _ in 0..6 {
            bytes.push(random.next_u8());
        }
        for _ in 0..count {
            bytes.extend_from_slice(&random.next_u32().to_le_bytes());
        }
    }
    bytes
}
fn acl(random: &mut Random, dacl: bool) -> Vec<u8> {
    let count = random.next_u32() as u16 % 16;
    let mut bytes = vec![2, 0, 0, 0];
    bytes.extend_from_slice(&count.to_le_bytes());
    bytes.extend_from_slice(&[0, 0]);
    for _ in 0..count {
        let start = bytes.len();
        bytes.push(if dacl {
            (random.next_u32() % 2) as u8
        } else {
            2
        });
        bytes.push(random.next_u8());
        bytes.extend_from_slice(&[0, 0]);
        bytes.extend_from_slice(&(random.next_u32() & 0x001f01ff).to_le_bytes());
        bytes.extend_from_slice(&sid(random));
        let size = (bytes.len() - start) as u16;
        bytes[start + 2..start + 4].copy_from_slice(&size.to_le_bytes());
    }
    let size = bytes.len() as u16;
    bytes[2..4].copy_from_slice(&size.to_le_bytes());
    bytes
}

/// Generate the original self-relative owner/group/DACL/SACL descriptor.
pub fn security_descriptor(random: &mut Random) -> Result<Vec<u8>, ParseError> {
    // The original stack buffer is8192 bytes. Reserve this bound before draws
    // so failure never exposes a partially built security descriptor.
    let mut bytes = zeros(20)?;
    bytes
        .try_reserve_exact(8192 - 20)
        .map_err(|_| ParseError::Nomem)?;
    bytes[0] = 1;
    bytes[2..4].copy_from_slice(&((random.next_u16() & 0x0c00) | 0x8014).to_le_bytes());
    for field in [4, 8] {
        let offset = bytes.len() as u32;
        bytes[field..field + 4].copy_from_slice(&offset.to_le_bytes());
        bytes.extend_from_slice(&sid(random));
    }
    for (field, dacl) in [(16, true), (12, false)] {
        if random.next_bool() {
            let offset = bytes.len() as u32;
            bytes[field..field + 4].copy_from_slice(&offset.to_le_bytes());
            bytes.extend_from_slice(&acl(random, dacl));
        }
    }
    Ok(bytes)
}
