// SPDX-License-Identifier: LGPL-2.1-or-later
//! Optional original test helpers; never compiled into standard builds.
//! Source: test_support.c at cd5e231c348c255ae5088873b5a66ee0eb96fa07.
use crate::engine::handles::{WimHandle, image_metadata_bytes};
/// Source-derived optional generated graphs and the feature-only capture route.
pub mod generate;
/// Bounded source-derived metadata and data generators.
pub mod primitives;
/// Source-derived random state for optional generated metadata.
pub mod random;
use wim_format::{
    ParseError,
    metadata::{Dentry, Metadata, StreamType},
};

/// Seed the original test generator's 48-bit linear congruential state.
#[unsafe(no_mangle)]
pub extern "C" fn wimlib_seed_random(seed: u64) {
    random::seed_global(seed);
}

fn timestamp_equal(a: u64, b: u64, flags: i32) -> bool {
    const EPOCH: i128 = 116_444_736_000_000_000;
    let valid = |value: u64| {
        let seconds = (i128::from(value) - EPOCH).div_euclid(10_000_000);
        (-0x8000_0000..0x3_8000_0000).contains(&seconds)
    };
    a == b || flags & 8 != 0 && (!valid(a) || !valid(b))
}
fn symlink(entry: &Dentry<'_>) -> bool {
    matches!(
        entry.reparse_fields(),
        Some((0xa000_000c | 0xa000_0003, _, _))
    )
}
fn attributes_equal(a: &Dentry<'_>, b: &Dentry<'_>, flags: i32) -> bool {
    let changed = a.attributes ^ b.attributes;
    let cleared = a.attributes & !b.attributes;
    let unix = flags & 1 != 0;
    !(b.attributes & 0x80 != 0 && b.attributes & !0x80 != 0
        || changed & 0x10 != 0 && !(unix && symlink(a))
        || changed & 0x400 != 0 && !(cleared & 0x400 != 0 && unix && !symlink(a))
        || changed & 0x200 != 0 && cleared & 0x200 == 0
        || changed & 0x800 != 0 && flags & 3 == 0
        || changed & !(0x80 | 0x10 | 0x400 | 0x200 | 0x800) != 0 && !unix)
}
fn folded_name(name: &[u8]) -> impl Iterator<Item = u16> + '_ {
    name.chunks_exact(2)
        .map(|unit| wim_format::ntfs_upcase::uppercase(u16::from_le_bytes([unit[0], unit[1]])))
}
fn inode_equal(a: &Dentry<'_>, b: &Dentry<'_>, flags: i32) -> bool {
    if !attributes_equal(a, b, flags)
        || flags & 1 == 0 && !timestamp_equal(a.creation_time, b.creation_time, flags)
        || !timestamp_equal(a.last_write_time, b.last_write_time, flags)
        || flags & 4 == 0 && !timestamp_equal(a.last_access_time, b.last_access_time, flags)
    {
        return false;
    }
    for stream in &a.streams {
        if stream.kind == StreamType::Unknown
            || stream.kind == StreamType::ReparsePoint && flags & 1 != 0 && !symlink(a)
        {
            continue;
        }
        match b
            .streams
            .iter()
            .find(|other| other.kind == stream.kind && other.name == stream.name)
        {
            Some(other) if other.hash == stream.hash => {}
            None if !stream.name.is_empty() && flags & 1 != 0 => {}
            _ => return false,
        }
    }
    let object_a = a.tagged_item(1, 16);
    let object_b = b.tagged_item(1, 16);
    if object_a != object_b && !(object_a.is_some() && object_b.is_none() && flags & 1 != 0) {
        return false;
    }
    let unix_a = a.tagged_item(0x337d_d873, 16).map(|bytes| &bytes[..16]);
    let unix_b = b.tagged_item(0x337d_d873, 16).map(|bytes| &bytes[..16]);
    if unix_a != unix_b
        && !((unix_a.is_some() && unix_b.is_none() && flags & 6 != 0)
            || (unix_a.is_none() && unix_b.is_some() && flags & 1 != 0))
    {
        return false;
    }
    true
}
type Xattrs<'a> = Option<Vec<(&'a [u8], &'a [u8])>>;
fn xattrs<'a>(entry: &Dentry<'a>) -> Result<Xattrs<'a>, ParseError> {
    let (mut bytes, old) = if let Some(bytes) = entry.tagged_item(2, 6) {
        (bytes, false)
    } else if let Some(bytes) = entry.tagged_item(0x337d_d874, 12) {
        (bytes, true)
    } else {
        return Ok(None);
    };
    let mut result = Vec::new();
    result
        .try_reserve_exact(64)
        .map_err(|_| ParseError::Nomem)?;
    while !bytes.is_empty() {
        let header = if old { 8 } else { 4 };
        if bytes.len() < header || result.len() == 64 {
            return Err(ParseError::InvalidXattr);
        }
        let name_len = if old {
            usize::from(u16::from_le_bytes([bytes[0], bytes[1]]))
        } else {
            usize::from(bytes[2])
        };
        let value_len = if old {
            u32::from_le_bytes([bytes[4], bytes[5], bytes[6], bytes[7]]) as usize
        } else {
            usize::from(u16::from_le_bytes([bytes[0], bytes[1]]))
        };
        let value_offset = header + name_len + usize::from(!old);
        let size = value_offset
            .checked_add(value_len)
            .ok_or(ParseError::InvalidXattr)?;
        let padded = if old {
            size.checked_add(3).ok_or(ParseError::InvalidXattr)? & !3
        } else {
            size
        };
        if name_len == 0
            || name_len > 255
            || value_len > 65535
            || padded > bytes.len()
            || bytes[header..header + name_len].contains(&0)
            || !old && bytes[header + name_len] != 0
        {
            return Err(ParseError::InvalidXattr);
        }
        result.push((
            &bytes[header..header + name_len],
            &bytes[value_offset..size],
        ));
        bytes = &bytes[padded..];
    }
    if result.is_empty() {
        return Err(ParseError::InvalidXattr);
    }
    result.sort_by(|a, b| a.0.len().cmp(&b.0.len()).then_with(|| a.0.cmp(b.0)));
    if result.windows(2).any(|pair| pair[0].0 == pair[1].0) {
        return Err(ParseError::InvalidXattr);
    }
    Ok(Some(result))
}
fn compare(a: &Metadata<'_>, b: &Metadata<'_>, flags: i32) -> Result<bool, ParseError> {
    if a.nodes.len() != b.nodes.len() {
        return Ok(false);
    }
    let mut pairs = Vec::new();
    pairs
        .try_reserve_exact(a.nodes.len())
        .map_err(|_| ParseError::Nomem)?;
    if !a.nodes.is_empty() {
        pairs.push((0, 0));
    }
    let mut cursor = 0;
    while cursor < pairs.len() {
        let (ai, bi) = pairs[cursor];
        cursor += 1;
        let an = &a.nodes[ai];
        let bn = &b.nodes[bi];
        if an.entry.name != bn.entry.name
            || !(folded_name(an.entry.short_name).eq(folded_name(bn.entry.short_name))
                || bn.entry.short_name.is_empty() && flags & 1 != 0)
            || an.children.len() != bn.children.len()
        {
            return Ok(false);
        }
        let mut ac = Vec::new();
        let mut bc = Vec::new();
        ac.try_reserve_exact(an.children.len())
            .map_err(|_| ParseError::Nomem)?;
        bc.try_reserve_exact(bn.children.len())
            .map_err(|_| ParseError::Nomem)?;
        ac.extend(an.children.iter().copied());
        bc.extend(bn.children.iter().copied());
        ac.sort_by(|x, y| {
            wim_format::ntfs_upcase::compare_names(a.nodes[*x].entry.name, a.nodes[*y].entry.name)
        });
        bc.sort_by(|x, y| {
            wim_format::ntfs_upcase::compare_names(b.nodes[*x].entry.name, b.nodes[*y].entry.name)
        });
        pairs.extend(ac.into_iter().zip(bc));
    }
    let mut forward = Vec::new();
    let mut backward = Vec::new();
    forward
        .try_reserve_exact(a.nodes.len())
        .map_err(|_| ParseError::Nomem)?;
    backward
        .try_reserve_exact(b.nodes.len())
        .map_err(|_| ParseError::Nomem)?;
    forward.resize(a.nodes.len(), None);
    backward.resize(b.nodes.len(), None);
    for &(ai, bi) in &pairs {
        let inode_a = a.nodes[ai].inode;
        let inode_b = b.nodes[bi].inode;
        if forward[inode_a].is_some_and(|other| other != inode_b)
            || backward[inode_b].is_some_and(|other| other != inode_a)
        {
            return Ok(false);
        }
        forward[inode_a] = Some(inode_b);
        backward[inode_b] = Some(inode_a);
        let ae = a
            .inode_entry(ai)
            .ok_or(ParseError::InvalidMetadataResource)?;
        let be = b
            .inode_entry(bi)
            .ok_or(ParseError::InvalidMetadataResource)?;
        if !inode_equal(ae, be, flags) {
            return Ok(false);
        }
        if flags & 4 == 0
            && let Some(sd) = a.security_descriptor(ai)
        {
            match b.security_descriptor(bi) {
                Some(other) if sd == other => {}
                None if flags & 1 != 0 => {}
                _ => return Ok(false),
            }
        }
        let xa = xattrs(ae)?;
        let xb = xattrs(be)?;
        if xa != xb && !(xa.is_some() && xb.is_none() && flags & 2 != 0) {
            return Ok(false);
        }
    }
    Ok(true)
}
/// Compare loaded image trees using original asymmetric metadata policies.
/// # Safety
/// Both handles must be live; neither may be freed during this operation.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn wimlib_compare_images(
    a: *mut WimHandle,
    image_a: i32,
    b: *mut WimHandle,
    image_b: i32,
    flags: i32,
) -> i32 {
    let run = || -> Result<bool, ParseError> {
        let a = unsafe { a.as_ref() }.ok_or(ParseError::InvalidParam)?;
        if image_a < 1 {
            return Err(ParseError::InvalidImage);
        }
        let ab = image_metadata_bytes(a, image_a as usize - 1)?;
        let am = Metadata::parse(&ab)?;
        let b = unsafe { b.as_ref() }.ok_or(ParseError::InvalidParam)?;
        if image_b < 1 {
            return Err(ParseError::InvalidImage);
        }
        let bb = image_metadata_bytes(b, image_b as usize - 1)?;
        let bm = Metadata::parse(&bb)?;
        compare(&am, &bm, flags)
    };
    match run() {
        Ok(true) => 0,
        Ok(false) => 200,
        Err(error) => error as i32,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use wim_format::metadata_write::{OwnedDentry, OwnedMetadata};
    fn tree() -> OwnedMetadata {
        let mut root = OwnedDentry::new(Vec::new(), 0x10);
        root.children.push(1);
        OwnedMetadata {
            security_descriptors: Vec::new(),
            nodes: vec![root, OwnedDentry::new(vec![b'a', 0], 0)],
        }
    }
    #[test]
    fn compares_stream_hashes_and_platform_timestamp_policies() {
        let a = tree();
        let mut b = a.clone();
        b.nodes[1].creation_time = 100;
        let aa = a.encode().unwrap();
        let bb = b.encode().unwrap();
        let am = Metadata::parse(&aa).unwrap();
        let bm = Metadata::parse(&bb).unwrap();
        assert!(!compare(&am, &bm, 0).unwrap());
        assert!(compare(&am, &bm, 1).unwrap());
        b.nodes[1].main_hash = [1; 20];
        let bb = b.encode().unwrap();
        let bm = Metadata::parse(&bb).unwrap();
        assert!(!compare(&am, &bm, 1).unwrap());
    }
    #[test]
    fn rejects_changed_hardlink_equivalence_even_with_identical_data() {
        let mut a = tree();
        a.nodes[0].children.push(2);
        let mut alias = a.nodes[1].clone();
        alias.name = vec![b'b', 0];
        a.nodes[1].inode_union = 42;
        alias.inode_union = 42;
        a.nodes.push(alias);
        let mut b = a.clone();
        b.nodes[2].inode_union = 43;
        let aa = a.encode().unwrap();
        let bb = b.encode().unwrap();
        assert!(
            !compare(
                &Metadata::parse(&aa).unwrap(),
                &Metadata::parse(&bb).unwrap(),
                0
            )
            .unwrap()
        );
    }
}
