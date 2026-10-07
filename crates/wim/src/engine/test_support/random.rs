// SPDX-License-Identifier: LGPL-2.1-or-later
//! Source-exact random state for the optional original test generator.
//!
//! Source: test_support.c, rand32 through rand64, at
//! cd5e231c348c255ae5088873b5a66ee0eb96fa07. This module does not enable
//! fabricated-source capture; the tree generator must be implemented first.
use std::sync::atomic::{AtomicU64, Ordering};

static GLOBAL_STATE: AtomicU64 = AtomicU64::new(0);
const MASK: u64 = (1 << 48) - 1;

fn advance(state: u64) -> u64 {
    state.wrapping_mul(25_214_903_917).wrapping_add(11) & MASK
}

/// Seed the shared state consumed by the optional tree generator.
pub fn seed_global(seed: u64) {
    GLOBAL_STATE.store(seed, Ordering::Relaxed);
}

/// A generator using either a reproducible local state or the C helper's state.
///
/// Individual shared draws are atomic, so allocation or progress callbacks do
/// not run with a random-state mutex held. Concurrent generation does not have
/// a deterministic interleaving, just as the original helper does not promise it.
pub enum Random {
    /// Independent state for source-derived unit tests and generation plans.
    Local(u64),
    /// The state selected by `wimlib_seed_random`.
    Global,
}

impl Random {
    /// Draw the upper 32 bits after one 48-bit LCG transition.
    pub fn next_u32(&mut self) -> u32 {
        let state = match self {
            Self::Local(state) => {
                *state = advance(*state);
                *state
            }
            Self::Global => {
                let previous = GLOBAL_STATE
                    .try_update(Ordering::Relaxed, Ordering::Relaxed, |state| {
                        Some(advance(state))
                    })
                    .unwrap_or_else(|state| state);
                advance(previous)
            }
        };
        (state >> 16) as u32
    }

    /// Draw a boolean using the original remainder operation.
    pub fn next_bool(&mut self) -> bool {
        !self.next_u32().is_multiple_of(2)
    }

    /// Draw a byte by truncating a complete 32-bit draw.
    pub fn next_u8(&mut self) -> u8 {
        self.next_u32() as u8
    }

    /// Draw a UTF-16 code unit by truncating a complete 32-bit draw.
    pub fn next_u16(&mut self) -> u16 {
        self.next_u32() as u16
    }

    /// Draw high bits first, consuming two transitions as the original does.
    pub fn next_u64(&mut self) -> u64 {
        (u64::from(self.next_u32()) << 32) | u64::from(self.next_u32())
    }
}

#[cfg(test)]
mod tests {
    use super::Random;

    #[test]
    fn zero_seed_matches_original_lcg_transitions() {
        let mut random = Random::Local(0);
        let observed: Vec<_> = (0..6).map(|_| random.next_u32()).collect();
        assert_eq!(
            observed,
            [
                0,
                0x0040_942d,
                0x0aa8_544e,
                0x2d38_73c4,
                0x5d56_92ac,
                0x1761_7168
            ]
        );
    }

    #[test]
    fn wide_draw_consumes_high_half_before_low_half() {
        let mut random = Random::Local(0);
        assert_eq!(random.next_u64(), 0x0040_942d);
        assert_eq!(random.next_u32(), 0x0aa8_544e);
    }

    #[test]
    fn overflowing_seed_uses_wrapping_original_arithmetic() {
        let mut random = Random::Local(u64::MAX);
        assert_eq!(random.next_u32(), 0xfffa_2113);
    }
}
