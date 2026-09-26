//! A fixed-size cache with one entry per slot, found by the key's hash.
//!
//! A lookup is a hash and a read, and a new entry takes the place of whatever
//! held its slot. Meant for small keys looked up once per glyph, where a hit
//! must cost less than a hash map probe and an LRU's recency bookkeeping,
//! and where losing an entry to a collision only costs recomputing it.

use std::hash::{Hash, Hasher};

pub(crate) struct DirectMappedCache<K, V> {
    slots: Box<[Option<(K, V)>]>,
    mask: usize,
}

impl<K: Copy + Eq + Hash, V: Copy> DirectMappedCache<K, V> {
    /// A cache of `1 << slots_log2` slots.
    pub(crate) fn with_slots_log2(slots_log2: u32) -> Self {
        let slots = 1usize << slots_log2;
        Self {
            slots: vec![None; slots].into_boxed_slice(),
            mask: slots - 1,
        }
    }

    pub(crate) fn get(&self, key: &K) -> Option<V> {
        match self.slots[self.slot(key)] {
            Some((stored, value)) if stored == *key => Some(value),
            _ => None,
        }
    }

    pub(crate) fn insert(&mut self, key: K, value: V) {
        let slot = self.slot(&key);
        self.slots[slot] = Some((key, value));
    }

    fn slot(&self, key: &K) -> usize {
        let mut hasher = cranpose_core::hash::default::new();
        key.hash(&mut hasher);
        // Truncation keeps the hash's low bits, which the mask selects.
        (hasher.finish() as usize) & self.mask
    }
}

#[cfg(test)]
#[path = "tests/direct_mapped_cache_tests.rs"]
mod tests;
