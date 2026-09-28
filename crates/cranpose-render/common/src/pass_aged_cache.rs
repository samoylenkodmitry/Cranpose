//! Caches that forget what layout stopped asking for.

use std::{borrow::Borrow, hash::Hash};

use crate::bounded_lru_cache::BoundedLruCache;

/// Layout passes an entry may go without a lookup before it leaves. Text on
/// screen is measured again only when its layout changes, so this is how long
/// a list may keep an item scrolled out before measuring it anew costs a miss.
pub(crate) const IDLE_PASSES: u64 = 120;

/// A value and the layout pass that last looked it up.
struct Used<V> {
    value: V,
    pass: u64,
}

/// A bounded LRU cache whose entries also leave once [`IDLE_PASSES`] layout
/// passes go by without a lookup of them. A scrolling list then keeps the
/// measurements of the texts it shows, not of every text it has shown up to
/// the bound.
pub(crate) struct PassAgedCache<K, V> {
    entries: BoundedLruCache<K, Used<V>>,
    pass: u64,
}

impl<K, V> PassAgedCache<K, V>
where
    K: Clone + Eq + Hash,
{
    pub(crate) fn with_capacity_at_least_one(capacity: usize) -> Self {
        Self {
            entries: BoundedLruCache::with_capacity_at_least_one(capacity),
            pass: 0,
        }
    }

    pub(crate) fn len(&self) -> usize {
        self.entries.len()
    }

    /// The value under `key`, marked used in the current pass.
    pub(crate) fn get<Q>(&mut self, key: &Q) -> Option<&V>
    where
        K: Borrow<Q>,
        Q: Hash + Eq + ?Sized,
    {
        let pass = self.pass;
        let used = self.entries.get_mut(key)?;
        used.pass = pass;
        Some(&used.value)
    }

    /// Stores `value` under `key`, returning the entry it replaced or the
    /// least recently used one it pushed out.
    pub(crate) fn push(&mut self, key: K, value: V) -> Option<(K, V)> {
        let pass = self.pass;
        self.entries
            .push(key, Used { value, pass })
            .map(|(key, used)| (key, used.value))
    }

    pub(crate) fn pop_lru(&mut self) -> Option<(K, V)> {
        self.entries.pop_lru().map(|(key, used)| (key, used.value))
    }

    /// Starts a layout pass: drops the entries no lookup asked for in the
    /// last [`IDLE_PASSES`] passes, least recently used first, handing each
    /// dropped value to `dropped`. It stops at the first entry still in use,
    /// so it costs one comparison when nothing has gone idle.
    pub(crate) fn begin_pass(&mut self, mut dropped: impl FnMut(V)) {
        self.pass += 1;
        let pass = self.pass;
        while self
            .entries
            .peek_lru()
            .is_some_and(|(_, used)| pass - used.pass > IDLE_PASSES)
        {
            let Some((_, used)) = self.entries.pop_lru() else {
                break;
            };
            dropped(used.value);
        }
    }
}

#[cfg(test)]
#[path = "tests/pass_aged_cache_tests.rs"]
mod tests;
