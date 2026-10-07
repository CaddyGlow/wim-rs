// SPDX-License-Identifier: LGPL-2.1-or-later
//! WIM information layout, masked header edits and output compression settings.
use crate::engine::handles::{WimHandle, can_modify, handle_mut, handle_ref};
use std::ffi::c_int;
use wim_format::{Compression, PIPABLE_MAGIC};

/// Original `wimlib_wim_info` ABI. Flags encode the C bitfields in declaration order.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct WimInfo {
    /// WIM globally unique identifier.
    pub guid: [u8; 16],
    /// Number of images.
    pub image_count: u32,
    /// One-based boot image, or zero.
    pub boot_index: u32,
    /// Format version.
    pub wim_version: u32,
    /// Input resource compression chunk size.
    pub chunk_size: u32,
    /// One-based split part index.
    pub part_number: u16,
    /// Total split parts.
    pub total_parts: u16,
    /// Input compression type.
    pub compression_type: i32,
    /// Root XML TOTALBYTES, or zero if absent/invalid.
    pub total_bytes: u64,
    /// Bitfields: integrity, opened, readonly, rpfix, marked-readonly, spanned,
    /// write-in-progress, metadata-only, resource-only, pipable.
    pub flags: u32,
    /// Reserved fields are zeroed by information queries.
    pub reserved: [u32; 9],
}

/// Populate the unchanged public header's information structure.
///
/// # Safety
/// `handle` must be a live native WIM handle; `output` must be writable for a
/// complete information structure and must not overlap the handle.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn wimlib_get_wim_info(
    handle: *mut WimHandle,
    output: *mut WimInfo,
) -> c_int {
    if output.is_null() {
        return 24;
    }
    // SAFETY: Caller supplies the live handle required by the ABI.
    let Some(wim) = (unsafe { handle_ref(handle) }) else {
        return 24;
    };
    let info = info(wim);
    // SAFETY: Output is writable according to the caller contract.
    unsafe {
        output.write(info);
    }
    0
}

pub(crate) fn info(wim: &WimHandle) -> WimInfo {
    let header = &wim.header;
    let mut flags = u32::from(header.integrity_table.offset_in_wim != 0)
        | (u32::from(wim.filename.is_some()) << 1)
        | (u32::from(!can_modify(wim)) << 2)
        | (u32::from(header.magic == PIPABLE_MAGIC) << 9);
    for (header_bit, info_bit) in [(0x80, 3), (4, 4), (8, 5), (0x40, 6), (0x20, 7), (0x10, 8)] {
        flags |= u32::from(header.flags & header_bit != 0) << info_bit;
    }
    WimInfo {
        guid: header.guid,
        image_count: header.image_count,
        boot_index: header.boot_index,
        wim_version: header.version,
        chunk_size: header.chunk_size,
        part_number: header.part_number,
        total_parts: header.total_parts,
        compression_type: header.validate_compression().map_or(0, Compression::as_i32),
        total_bytes: wim.xml.total_bytes(),
        flags,
        reserved: [0; 9],
    }
}

/// Apply exactly the selected GUID, readonly, boot-index and rpfix fields.
///
/// # Safety
/// `handle` must be live and uniquely used. `info` must point to a readable
/// structure, disjoint from the handle, when a supported nonzero mask is used.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn wimlib_set_wim_info(
    handle: *mut WimHandle,
    info: *const WimInfo,
    which: c_int,
) -> c_int {
    if which & !15 != 0 {
        return 24;
    }
    // SAFETY: Caller supplies the live exclusively used handle.
    let Some(wim) = (unsafe { handle_mut(handle) }) else {
        return 24;
    };
    if which == 0 {
        return 0;
    }
    if info.is_null() {
        return 24;
    }
    // SAFETY: A readable structure is required for selected mutations.
    let info = unsafe { &*info };
    set_info(wim, info, which).map_or_else(|error| error, |()| 0)
}
pub(crate) fn set_info(wim: &mut WimHandle, info: &WimInfo, which: c_int) -> Result<(), c_int> {
    if which & !15 != 0 {
        return Err(24);
    }
    if which & 4 != 0 && info.boot_index > wim.header.image_count {
        return Err(18);
    }
    if which & 1 != 0 {
        set_flag(&mut wim.header.flags, 4, info.flags & (1 << 4) != 0);
    }
    if which & 2 != 0 {
        wim.header.guid = info.guid;
    }
    if which & 4 != 0 {
        wim.header.boot_index = info.boot_index;
    }
    if which & 8 != 0 {
        set_flag(&mut wim.header.flags, 0x80, info.flags & (1 << 3) != 0);
    }
    Ok(())
}
fn set_flag(flags: &mut u32, bit: u32, enabled: bool) {
    if enabled {
        *flags |= bit;
    } else {
        *flags &= !bit;
    }
}
fn default_chunk(codec: Compression, solid: bool) -> u32 {
    match codec {
        Compression::None => 0,
        Compression::Xpress | Compression::Lzx => 32768,
        Compression::Lzms => {
            if solid {
                67108864
            } else {
                131072
            }
        }
    }
}

/// Select compression for ordinary output resources and repair an invalid chunk size.
///
/// # Safety
/// `handle` must be a live exclusively used native WIM handle.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn wimlib_set_output_compression_type(
    handle: *mut WimHandle,
    codec: c_int,
) -> c_int {
    // SAFETY: Same live-handle contract as the exported function.
    unsafe { set_compression(handle, codec, false) }
}
/// Select compression for solid output resources. Uncompressed solid output is invalid.
///
/// # Safety
/// `handle` must be a live exclusively used native WIM handle.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn wimlib_set_output_pack_compression_type(
    handle: *mut WimHandle,
    codec: c_int,
) -> c_int {
    // SAFETY: Same live-handle contract as the exported function.
    unsafe { set_compression(handle, codec, true) }
}
unsafe fn set_compression(handle: *mut WimHandle, codec: c_int, solid: bool) -> c_int {
    let Ok(codec) = Compression::from_i32(codec) else {
        return 16;
    };
    if solid && codec == Compression::None {
        return 16;
    }
    // SAFETY: Caller supplies a live exclusively used handle.
    let Some(wim) = (unsafe { handle_mut(handle) }) else {
        return 24;
    };
    let (compression, chunk) = if solid {
        (
            &mut wim.output_solid_compression,
            &mut wim.output_solid_chunk_size,
        )
    } else {
        (&mut wim.output_compression, &mut wim.output_chunk_size)
    };
    *compression = codec;
    if codec.validate_chunk_size(*chunk).is_err() {
        *chunk = default_chunk(codec, solid);
    }
    0
}
/// Set the ordinary output chunk size; zero restores the codec's default.
///
/// # Safety
/// `handle` must be a live exclusively used native WIM handle.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn wimlib_set_output_chunk_size(handle: *mut WimHandle, chunk: u32) -> c_int {
    // SAFETY: Same live-handle contract as the exported function.
    unsafe { set_chunk(handle, chunk, false) }
}
/// Set the solid output chunk size; zero restores the codec's default.
///
/// # Safety
/// `handle` must be a live exclusively used native WIM handle.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn wimlib_set_output_pack_chunk_size(
    handle: *mut WimHandle,
    chunk: u32,
) -> c_int {
    // SAFETY: Same live-handle contract as the exported function.
    unsafe { set_chunk(handle, chunk, true) }
}
unsafe fn set_chunk(handle: *mut WimHandle, chunk: u32, solid: bool) -> c_int {
    // SAFETY: Caller supplies a live exclusively used handle.
    let Some(wim) = (unsafe { handle_mut(handle) }) else {
        return 24;
    };
    let (codec, selected) = if solid {
        (
            wim.output_solid_compression,
            &mut wim.output_solid_chunk_size,
        )
    } else {
        (wim.output_compression, &mut wim.output_chunk_size)
    };
    if chunk == 0 {
        *selected = default_chunk(codec, solid);
        return 0;
    }
    if codec.validate_chunk_size(chunk).is_err() {
        return 15;
    }
    *selected = chunk;
    0
}
