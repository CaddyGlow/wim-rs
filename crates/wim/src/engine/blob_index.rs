// SPDX-License-Identifier: LGPL-2.1-or-later
//! Retained original blob-table bucket and insertion history.
use std::cell::RefCell;
use wim_format::ParseError;

/// Actual storage that owns a hashed content descriptor.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BlobOwner {
    /// Parsed descriptor in the retained input lookup table.
    Stored,
    /// Independently retained data in the handle's owned blob map.
    Owned,
    /// Hashed source bound to an authoritative pending capture graph.
    Captured,
}
#[derive(Debug, Clone, Copy)]
struct Link {
    hash: [u8; 20],
    owner: BlobOwner,
    next: usize,
    live: bool,
}
const EMPTY: Link = Link {
    hash: [0; 20],
    owner: BlobOwner::Stored,
    next: 0,
    live: false,
};
#[derive(Debug)]
struct State {
    heads: Vec<usize>,
    links: Vec<Link>,
    used: usize,
    count: usize,
    free: usize,
}
impl State {
    fn position(&self, hash: &[u8; 20]) -> Option<usize> {
        let mut link = self.heads[short_hash(hash) & (self.heads.len() - 1)];
        while link != 0 {
            let index = link - 1;
            let node = self.links[index];
            if node.hash == *hash {
                return Some(index);
            }
            link = node.next;
        }
        None
    }
}
/// Globally allocated index preserving upstream head chains and rehash order.
#[derive(Debug)]
pub struct BlobIndex(RefCell<State>);
fn short_hash(hash: &[u8; 20]) -> usize {
    let mut word = [0u8; std::mem::size_of::<usize>()];
    let length = word.len();
    word.copy_from_slice(&hash[..length]);
    usize::from_ne_bytes(word)
}
impl BlobIndex {
    /// Allocate real bucket heads, rounding zero to one and otherwise up to a
    /// power of two as upstream's roundup_pow_of_2 does.
    pub fn new(capacity: usize) -> Result<Self, ParseError> {
        let capacity = capacity
            .max(1)
            .checked_next_power_of_two()
            .ok_or(ParseError::Nomem)?;
        Ok(Self(RefCell::new(State {
            heads: crate::engine::collections::filled(capacity, 0)
                .map_err(|_| ParseError::Nomem)?,
            links: crate::engine::collections::filled(0, EMPTY).map_err(|_| ParseError::Nomem)?,
            used: 0,
            count: 0,
            free: 0,
        })))
    }
    /// Current bucket capacity, retained across unlink operations.
    pub fn capacity(&self) -> usize {
        self.0.borrow().heads.len()
    }
    /// Number of live hashed descriptors.
    pub fn len(&self) -> usize {
        self.0.borrow().count
    }
    /// Whether no hashed descriptors are retained.
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }
    /// Find actual descriptor ownership without changing insertion history.
    pub fn owner(&self, hash: &[u8; 20]) -> Option<BlobOwner> {
        let state = self.0.borrow();
        state.position(hash).map(|index| state.links[index].owner)
    }
    fn reserve_link(&self) -> Result<(), ParseError> {
        loop {
            let (required, capacity) = {
                let state = self.0.borrow();
                if state.free != 0 || state.used < state.links.len() {
                    return Ok(());
                }
                (
                    state.used.checked_add(1).ok_or(ParseError::Nomem)?,
                    state.links.len(),
                )
            };
            let capacity = required.max(capacity.checked_mul(2).ok_or(ParseError::Nomem)?);
            let mut replacement = crate::engine::collections::filled(capacity, EMPTY)
                .map_err(|_| ParseError::Nomem)?;
            let previous = {
                let mut state = self.0.borrow_mut();
                if state.links.len() >= capacity {
                    None
                } else {
                    replacement[..state.links.len()].copy_from_slice(&state.links);
                    Some(std::mem::replace(&mut state.links, replacement))
                }
            };
            // Release the replaced storage after ending the interior state borrow.
            drop(previous);
        }
    }
    /// Insert at bucket head. Existing hashes keep their original position.
    /// Link allocation failure leaves the index unchanged. Failed bucket growth
    /// retains the successfully inserted old table, matching upstream.
    pub fn insert(&self, hash: [u8; 20], owner: BlobOwner) -> Result<bool, ParseError> {
        if self.owner(&hash).is_some() {
            return Ok(false);
        }
        self.reserve_link()?;
        let grow = {
            let mut state = self.0.borrow_mut();
            if state.position(&hash).is_some() {
                return Ok(false);
            }
            let bucket = short_hash(&hash) & (state.heads.len() - 1);
            let node = Link {
                hash,
                owner,
                next: state.heads[bucket],
                live: true,
            };
            let index = if state.free == 0 {
                let index = state.used;
                state.used += 1;
                index
            } else {
                let index = state.free - 1;
                state.free = state.links[index].next;
                index
            };
            state.links[index] = node;
            state.heads[bucket] = index + 1;
            let grow = state.count > state.heads.len() - 1;
            state.count += 1;
            grow
        };
        if grow {
            self.grow();
        }
        Ok(true)
    }
    fn grow(&self) {
        let capacity = {
            let state = self.0.borrow();
            let Some(capacity) = state.heads.len().checked_mul(2) else {
                return;
            };
            capacity
        };
        // Preserve the committed insertion if bucket growth cannot allocate.
        let Ok(mut heads) = crate::engine::collections::filled(capacity, 0) else {
            return;
        };
        let previous = {
            let mut state = self.0.borrow_mut();
            if state.heads.len() >= capacity {
                None
            } else {
                for bucket in 0..state.heads.len() {
                    let mut link = state.heads[bucket];
                    while link != 0 {
                        let index = link - 1;
                        let node = &mut state.links[index];
                        let next = node.next;
                        let bucket = short_hash(&node.hash) & (capacity - 1);
                        node.next = heads[bucket];
                        heads[bucket] = link;
                        link = next;
                    }
                }
                Some(std::mem::replace(&mut state.heads, heads))
            }
        };
        drop(previous);
    }
    /// Update real descriptor ownership without unlinking and reinserting it.
    pub fn set_owner(&self, hash: &[u8; 20], owner: BlobOwner) -> bool {
        let mut state = self.0.borrow_mut();
        let Some(index) = state.position(hash) else {
            return false;
        };
        state.links[index].owner = owner;
        true
    }
    /// Unlink without shrinking buckets. Later reinsertion adds at the head.
    pub fn unlink(&self, hash: &[u8; 20]) -> bool {
        let mut state = self.0.borrow_mut();
        let bucket = short_hash(hash) & (state.heads.len() - 1);
        let mut link = state.heads[bucket];
        let mut previous = 0;
        while link != 0 {
            let index = link - 1;
            let node = state.links[index];
            if node.hash == *hash {
                if previous == 0 {
                    state.heads[bucket] = node.next;
                } else {
                    state.links[previous - 1].next = node.next;
                }
                state.links[index].live = false;
                state.links[index].next = state.free;
                state.free = link;
                state.count -= 1;
                return true;
            }
            previous = link;
            link = node.next;
        }
        false
    }
    /// Clone bucket/link ownership for a real rollback checkpoint.
    pub fn try_clone(&self) -> Result<Self, ParseError> {
        let (capacity, links) = {
            let state = self.0.borrow();
            (state.heads.len(), state.links.len())
        };
        let mut heads =
            crate::engine::collections::filled(capacity, 0).map_err(|_| ParseError::Nomem)?;
        let mut links =
            crate::engine::collections::filled(links, EMPTY).map_err(|_| ParseError::Nomem)?;
        let state = self.0.borrow();
        heads.copy_from_slice(&state.heads);
        links.copy_from_slice(&state.links);
        Ok(Self(RefCell::new(State {
            heads,
            links,
            used: state.used,
            count: state.count,
            free: state.free,
        })))
    }
    /// Copy the next descriptor in bucket/head-chain order. No state or handle
    /// borrow remains live after this call, including at a C callback boundary.
    pub fn next(&self, cursor: &mut BlobCursor) -> Option<([u8; 20], BlobOwner)> {
        let capacity = self.capacity();
        let state = self.0.borrow();
        while cursor.link == 0 {
            if cursor.bucket >= capacity {
                return None;
            }
            cursor.link = state.heads[cursor.bucket];
            cursor.bucket += 1;
        }
        let node = state.links[cursor.link - 1];
        debug_assert!(node.live);
        cursor.link = node.next;
        Some((node.hash, node.owner))
    }
}
/// Value cursor borrowing no archive, descriptor or callback context.
#[derive(Debug, Default, Clone, Copy)]
pub struct BlobCursor {
    bucket: usize,
    link: usize,
}
#[cfg(test)]
mod tests {
    use super::*;
    fn hash(value: usize) -> [u8; 20] {
        let mut hash = [0; 20];
        hash[..std::mem::size_of::<usize>()].copy_from_slice(&value.to_ne_bytes());
        hash[19] = 1;
        hash
    }
    fn order(index: &BlobIndex) -> Vec<[u8; 20]> {
        let mut cursor = BlobCursor::default();
        std::iter::from_fn(|| index.next(&mut cursor).map(|entry| entry.0)).collect()
    }
    #[test]
    fn growth_reinserts_old_head_chains_instead_of_sorting_hashes() {
        let index = BlobIndex::new(4).unwrap();
        for value in [0, 4, 8, 12] {
            index.insert(hash(value), BlobOwner::Stored).unwrap();
        }
        assert_eq!(index.capacity(), 4);
        assert_eq!(order(&index), [12, 8, 4, 0].map(hash));
        index.insert(hash(16), BlobOwner::Owned).unwrap();
        assert_eq!(index.capacity(), 8);
        assert_eq!(order(&index), [0, 8, 16, 4, 12].map(hash));
    }
    #[test]
    fn unlink_reinsert_and_checkpoint_preserve_actual_history() {
        let index = BlobIndex::new(64).unwrap();
        for value in [0, 64, 128] {
            index.insert(hash(value), BlobOwner::Captured).unwrap();
        }
        let checkpoint = index.try_clone().unwrap();
        assert!(index.unlink(&hash(64)));
        index.insert(hash(64), BlobOwner::Owned).unwrap();
        assert_eq!(order(&index), [64, 128, 0].map(hash));
        assert_eq!(order(&checkpoint), [128, 64, 0].map(hash));
        assert!(!index.insert(hash(128), BlobOwner::Stored).unwrap());
        assert_eq!(index.owner(&hash(128)), Some(BlobOwner::Captured));
    }
    #[test]
    #[cfg(target_pointer_width = "64")]
    fn original_c_growth_and_unlink_chains_match() {
        let original = include_str!("../../tests/fixtures/evidence/buckets-original.txt");
        assert_eq!(original.lines().next(), Some("word 8 disk 50"));
        let check = |index: &BlobIndex, label: &str| {
            let expected = original
                .lines()
                .find(|line| line.starts_with(&format!("{label} ")))
                .unwrap();
            let mut actual = label.to_owned();
            for digest in order(index) {
                let word = usize::from_ne_bytes(digest[..8].try_into().unwrap());
                use std::fmt::Write;
                write!(&mut actual, " {}", word / 256).unwrap();
            }
            use std::fmt::Write;
            write!(
                &mut actual,
                " rc0 cap{} count{}",
                index.capacity(),
                index.len()
            )
            .unwrap();
            assert_eq!(actual, expected);
        };
        let index = BlobIndex::new(64).unwrap();
        for value in 0..130 {
            index
                .insert(hash(value * 256), BlobOwner::Captured)
                .unwrap();
            let label = match value {
                63 => Some("at64"),
                64 => Some("at65"),
                65 => Some("at66"),
                127 => Some("at128"),
                128 => Some("at129"),
                129 => Some("at130"),
                _ => None,
            };
            if let Some(label) = label {
                check(&index, label);
            }
        }
        index.unlink(&hash(4 * 256));
        index.insert(hash(4 * 256), BlobOwner::Owned).unwrap();
        check(&index, "reinsert4");
    }
}
