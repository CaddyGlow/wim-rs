// SPDX-License-Identifier: LGPL-2.1-or-later
//! Compatibility memory estimates for the original x86-64 compressor profile.
//! These values reproduce the original allocation formula; they do not yet
//! describe the temporary allocations made by the native fixed-strategy encoders.

#![cfg(target_pointer_width = "64")]

use ms_compress::context::{Codec, CompressionConfig};

// Original x86-64 structure sizes and offsets are recorded by the layout
// oracle in scripts/wimlib/probe-compression-memory-layout.c. No C code is used
// at runtime. The outer original compressor contributes four pointer-size words.
fn needed_memory(config: CompressionConfig) -> u64 {
    let n = config.maximum as u64;
    let destructive_copy = if config.destructive { 0 } else { n };
    let private = match config.codec {
        Codec::Xpress if config.level < 60 => 4632 + 196608 + 2 * n + 8 * n,
        Codec::Xpress => 6700 + 262144 + 4 * n + 8 * (n + 1) + 8 * (9 * n + 65538),
        Codec::Lzx => {
            let (prefix, table, positions) = match (config.level <= 34, n <= 32768) {
                (true, true) => (445832, 196608, 2),
                (true, false) => (445832, 393216, 4),
                (false, true) => (5281236, 270336, 4),
                (false, false) => (5281236, 540672, 8),
            };
            prefix + table + positions * n + destructive_copy
        }
        Codec::Lzms => {
            let interval_width = if n <= 1 << 26 { 4 } else { 8 };
            978368 + destructive_copy + 4 * (n + 5).max(65792) + interval_width * (n + 5)
        }
    };
    private + 32
}

/// Reproduce the original x86-64 baseline compressor allocation estimate.
///
/// Returns zero for invalid codecs, block sizes or explicit levels. Zero levels
/// resolve through the same defaults as `wimlib_create_compressor`. The returned
/// compatibility estimate is not yet a bound on native codec scratch allocations;
/// native codec memory budgeting remains an incomplete replacement gate.
#[unsafe(no_mangle)]
pub extern "C" fn wimlib_get_compressor_needed_memory(
    codec: std::ffi::c_int,
    maximum: usize,
    level: u32,
) -> u64 {
    let codec_value = match Codec::from_wimlib(codec) {
        Ok(value) => value,
        Err(_) => return 0,
    };
    let destructive = level & 0x8000_0000 != 0;
    let mut resolved_level = level & !0x8000_0000;
    let limit = match codec_value {
        Codec::Xpress => 65536,
        Codec::Lzx => 1 << 21,
        Codec::Lzms => 1 << 30,
    };
    if maximum == 0 || maximum > limit || resolved_level > 0x00ff_ffff {
        return 0;
    }
    if resolved_level == 0 {
        resolved_level = crate::engine::compress::default_compression_level(codec);
    }
    if resolved_level == 0 {
        resolved_level = 50;
    }
    needed_memory(CompressionConfig {
        codec: codec_value,
        maximum,
        level: resolved_level,
        destructive,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    fn config(codec: Codec, maximum: usize, level: u32, destructive: bool) -> CompressionConfig {
        CompressionConfig {
            codec,
            maximum,
            level,
            destructive,
        }
    }
    #[test]
    fn invalid_query_parameters_return_zero_without_allocating() {
        for codec in [-1, 0, 4, i32::MAX] {
            assert_eq!(wimlib_get_compressor_needed_memory(codec, 32768, 50), 0);
        }
        for (codec, limit) in [(1, 65536), (2, 1 << 21), (3, 1 << 30)] {
            assert_eq!(wimlib_get_compressor_needed_memory(codec, 0, 50), 0);
            assert_eq!(wimlib_get_compressor_needed_memory(codec, limit + 1, 50), 0);
            assert_eq!(
                wimlib_get_compressor_needed_memory(codec, limit, 0x0100_0000),
                0
            );
            assert!(wimlib_get_compressor_needed_memory(codec, limit, 50) > 0);
        }
    }
    #[test]
    fn xpress_estimate_changes_at_near_optimal_threshold() {
        assert_eq!(
            needed_memory(config(Codec::Xpress, 32768, 59, false)),
            528952
        );
        assert_eq!(
            needed_memory(config(Codec::Xpress, 32768, 60, false)),
            3545700
        );
    }
    #[test]
    fn lzx_estimate_changes_at_fast_level_and_position_width_thresholds() {
        assert_eq!(needed_memory(config(Codec::Lzx, 32768, 34, false)), 740776);
        assert_eq!(needed_memory(config(Codec::Lzx, 32769, 34, false)), 1002925);
        assert_eq!(needed_memory(config(Codec::Lzx, 32768, 35, true)), 5682676);
    }
    #[test]
    fn lzms_estimate_changes_at_suffix_array_and_interval_width_thresholds() {
        assert_eq!(needed_memory(config(Codec::Lzms, 1, 50, false)), 1241593);
        assert_eq!(
            needed_memory(config(Codec::Lzms, 67108864, 50, true)),
            537849352
        );
        assert_eq!(
            needed_memory(config(Codec::Lzms, 67108865, 50, true)),
            806284840
        );
    }
}
