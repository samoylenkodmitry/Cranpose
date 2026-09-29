//! A fixed-size cache with one entry per slot, found by the key's hash.
//!
//! A lookup is a hash and a read, and a new entry takes the place of whatever
//! held its slot. Meant for small keys looked up once per glyph, where a hit
//! must cost less than a hash map probe and an LRU's recency bookkeeping,
//! and where losing an entry to a collision only costs recomputing it.

use std::hash::{Hash, Hasher};

pub(crate) struct DirectMappedCache<K, V> {
    /// Empty until the first entry: a cache nothing is put in, such as the
    /// kerning of characters a face's ASCII table answers, takes no memory.
    slots: Box<[Option<(K, V)>]>,
    slots_log2: u32,
}

impl<K: Copy + Eq + Hash, V: Copy> DirectMappedCache<K, V> {
    /// A cache of `1 << slots_log2` slots, allocated with its first entry.
    pub(crate) fn with_slots_log2(slots_log2: u32) -> Self {
        Self {
            slots: Box::default(),
            slots_log2,
        }
    }

    pub(crate) fn get(&self, key: &K) -> Option<V> {
        if self.slots.is_empty() {
            return None;
        }
        match self.slots[self.slot(key)] {
            Some((stored, value)) if stored == *key => Some(value),
            _ => None,
        }
    }

    pub(crate) fn insert(&mut self, key: K, value: V) {
        if self.slots.is_empty() {
            self.slots = vec![None; 1usize << self.slots_log2].into_boxed_slice();
        }
        let slot = self.slot(&key);
        self.slots[slot] = Some((key, value));
    }

    fn slot(&self, key: &K) -> usize {
        let mut hasher = cranpose_core::hash::default::new();
        key.hash(&mut hasher);
        // Truncation keeps the hash's low bits, which the mask selects.
        (hasher.finish() as usize) & (self.slots.len() - 1)
    }
}

#[cfg(test)]
#[path = "tests/direct_mapped_cache_tests.rs"]
mod tests;
