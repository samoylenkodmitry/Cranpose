//! Caches that forget what layout stopped asking for.

use std::{borrow::Borrow, hash::Hash, num::NonZeroUsize};

use super::{bounded_lru::BoundedLruCache, map::RandomState};

/// Layout passes an entry may go without a lookup before it leaves. Text on
/// screen is measured again only when its layout changes, so this is how long
/// a list may keep an item scrolled out before measuring it anew costs a miss.
pub const IDLE_PASSES: u64 = 120;

/// A value and the layout pass that last looked it up.
struct Used<V> {
    value: V,
    pass: u64,
}

/// Share of a cache's bound held by entries no lookup has hit yet.
const PROBATION_SHARE: usize = 4;

/// A bounded LRU cache whose entries also leave once [`IDLE_PASSES`] layout
/// passes go by without a lookup of them. A scrolling list then keeps the
/// measurements of the texts it shows, not of every text it has shown up to
/// the bound.
///
/// An entry waits in a probation segment, a quarter of the bound, until its
/// first lookup. A text measured once and replaced the next pass, such as a
/// ticking price, leaves from there: a stream of them holds a quarter of the
/// bound instead of all of it, and pushes out none of the entries lookups
/// keep hitting.
pub struct PassAgedCache<K, V> {
    /// Entries a lookup has hit since they were stored.
    entries: BoundedLruCache<K, Used<V>>,
    /// Entries no lookup has hit yet.
    probation: BoundedLruCache<K, Used<V>>,
    pass: u64,
}

impl<K, V> PassAgedCache<K, V>
where
    K: Eq + Hash,
{
    pub fn with_capacity_at_least_one(capacity: usize) -> Self {
        let probation = (capacity / PROBATION_SHARE).max(1);
        let bound = |cap| NonZeroUsize::new(cap).unwrap_or(NonZeroUsize::MIN);
        // One hasher for both segments, so a lookup hashes its key once.
        let hasher = RandomState::default();
        Self {
            entries: BoundedLruCache::with_hasher(
                bound(capacity.saturating_sub(probation)),
                hasher.clone(),
            ),
            probation: BoundedLruCache::with_hasher(bound(probation), hasher),
            pass: 0,
        }
    }

    pub fn len(&self) -> usize {
        self.entries.len() + self.probation.len()
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty() && self.probation.is_empty()
    }

    /// Drops every entry and gives back the storage they took.
    pub fn clear(&mut self) {
        self.entries.clear();
        self.probation.clear();
    }

    /// The value under `key`, marked used in the current pass. A first
    /// lookup moves it out of probation.
    pub fn get<Q>(&mut self, key: &Q) -> Option<&V>
    where
        K: Borrow<Q>,
        Q: Hash + Eq + ?Sized,
    {
        let hash = self.entries.hash(key);
        let slot = match self.entries.find_hashed(hash, key) {
            Some(slot) => slot,
            None => {
                let (key, used) = self.probation.remove_hashed(hash, key)?;
                self.entries.insert_absent_hashed(hash, key, used).0
            }
        };
        let used = self.entries.touch_mut(slot);
        used.pass = self.pass;
        Some(&used.value)
    }

    /// Stores `value` under `key`, returning the entry it replaced or the
    /// least recently used one it pushed out.
    pub fn push(&mut self, key: K, value: V) -> Option<(K, V)> {
        let used = Used {
            value,
            pass: self.pass,
        };
        let hash = self.entries.hash(&key);
        let segment = if self.entries.find_hashed(hash, &key).is_some() {
            &mut self.entries
        } else {
            &mut self.probation
        };
        segment
            .push_hashed(hash, key, used)
            .map(|(key, used)| (key, used.value))
    }

    /// Starts a layout pass: drops the entries no lookup asked for in the
    /// last [`IDLE_PASSES`] passes, least recently used first, handing each
    /// dropped value to `dropped`. It stops at the first entry still in use,
    /// so it costs one comparison a segment when nothing has gone idle.
    pub fn begin_pass(&mut self, mut dropped: impl FnMut(V)) {
        self.pass += 1;
        let pass = self.pass;
        for segment in [&mut self.probation, &mut self.entries] {
            while segment
                .peek_lru()
                .is_some_and(|(_, used)| pass - used.pass > IDLE_PASSES)
            {
                let Some((_, used)) = segment.pop_lru() else {
                    break;
                };
                dropped(used.value);
            }
        }
    }
}

#[cfg(test)]
#[path = "tests/pass_aged_tests.rs"]
mod tests;
