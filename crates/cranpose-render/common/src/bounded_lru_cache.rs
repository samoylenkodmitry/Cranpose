use std::{hash::Hash, num::NonZeroUsize};

use cranpose_core::collections::map::HashMap;

struct CacheSlot<K, V> {
    key: K,
    value: V,
    newer: Option<usize>,
    older: Option<usize>,
}

/// Small bounded LRU cache used by renderer hot-path caches.
///
/// Hits update recency in place, so the common path is a single hash lookup.
/// Eviction unlinks the oldest entry, which costs the same whether the cache
/// holds ten entries or ten thousand.
///
/// The recency order is a linked list rather than a timestamp per entry
/// because a timestamp makes eviction a scan for the minimum. These caches are
/// large -- thousands of glyph masks -- and the workloads that need them most
/// are the ones that miss steadily: text whose size animates re-rasterises
/// every glyph of every frame, and every one of those inserts was walking the
/// whole table to decide what to drop.
///
/// A key is held twice, once in the index and once in its slot, so an eviction
/// can find the index entry to remove without searching for it. The keys these
/// caches use are small `Copy` structs, and the duplicate is what keeps the
/// links free of raw pointers.
pub struct BoundedLruCache<K, V> {
    index: HashMap<K, usize>,
    slots: Vec<Option<CacheSlot<K, V>>>,
    free: Vec<usize>,
    newest: Option<usize>,
    oldest: Option<usize>,
    cap: NonZeroUsize,
}

impl<K, V> BoundedLruCache<K, V>
where
    K: Clone + Eq + Hash,
{
    pub fn new(cap: NonZeroUsize) -> Self {
        Self {
            index: HashMap::with_capacity_and_hasher(cap.get(), Default::default()),
            slots: Vec::with_capacity(cap.get()),
            free: Vec::new(),
            newest: None,
            oldest: None,
            cap,
        }
    }

    pub fn with_capacity_at_least_one(cap: usize) -> Self {
        let cap = NonZeroUsize::new(cap).unwrap_or(NonZeroUsize::MIN);
        Self::new(cap)
    }

    pub fn len(&self) -> usize {
        self.index.len()
    }

    pub fn is_empty(&self) -> bool {
        self.index.is_empty()
    }

    pub fn cap(&self) -> NonZeroUsize {
        self.cap
    }

    pub fn contains(&self, key: &K) -> bool {
        self.index.contains_key(key)
    }

    pub fn get(&mut self, key: &K) -> Option<&V> {
        let slot = *self.index.get(key)?;
        self.promote(slot);
        Some(&self.slot(slot).value)
    }

    pub fn peek(&self, key: &K) -> Option<&V> {
        let slot = *self.index.get(key)?;
        Some(&self.slot(slot).value)
    }

    pub fn get_mut(&mut self, key: &K) -> Option<&mut V> {
        let slot = *self.index.get(key)?;
        self.promote(slot);
        Some(&mut self.slot_mut(slot).value)
    }

    pub fn push(&mut self, key: K, value: V) -> Option<(K, V)> {
        if let Some(&slot) = self.index.get(&key) {
            self.promote(slot);
            let old_value = std::mem::replace(&mut self.slot_mut(slot).value, value);
            return Some((key, old_value));
        }

        let evicted = if self.index.len() == self.cap.get() {
            self.pop_lru()
        } else {
            None
        };

        let slot = self.claim_slot(key.clone(), value);
        self.index.insert(key, slot);
        self.link_newest(slot);
        evicted
    }

    pub fn put(&mut self, key: K, value: V) -> Option<V> {
        self.push(key, value).map(|(_, value)| value)
    }

    pub fn pop_lru(&mut self) -> Option<(K, V)> {
        let slot = self.oldest?;
        self.unlink(slot);
        let entry = self.release_slot(slot);
        self.index.remove(&entry.key);
        Some((entry.key, entry.value))
    }

    pub fn pop(&mut self, key: &K) -> Option<V> {
        let slot = self.index.remove(key)?;
        self.unlink(slot);
        Some(self.release_slot(slot).value)
    }

    /// Entries most recently used first.
    pub fn iter(&self) -> impl Iterator<Item = (&K, &V)> {
        let mut next = self.newest;
        std::iter::from_fn(move || {
            let entry = self.slot(next?);
            next = entry.older;
            Some((&entry.key, &entry.value))
        })
    }

    fn slot(&self, slot: usize) -> &CacheSlot<K, V> {
        self.slots[slot]
            .as_ref()
            .expect("a linked cache slot is always occupied")
    }

    fn slot_mut(&mut self, slot: usize) -> &mut CacheSlot<K, V> {
        self.slots[slot]
            .as_mut()
            .expect("a linked cache slot is always occupied")
    }

    fn promote(&mut self, slot: usize) {
        if self.newest == Some(slot) {
            return;
        }
        self.unlink(slot);
        self.link_newest(slot);
    }

    fn link_newest(&mut self, slot: usize) {
        let previous_newest = self.newest;
        {
            let entry = self.slot_mut(slot);
            entry.newer = None;
            entry.older = previous_newest;
        }
        if let Some(previous) = previous_newest {
            self.slot_mut(previous).newer = Some(slot);
        }
        self.newest = Some(slot);
        if self.oldest.is_none() {
            self.oldest = Some(slot);
        }
    }

    fn unlink(&mut self, slot: usize) {
        let (newer, older) = {
            let entry = self.slot_mut(slot);
            (entry.newer.take(), entry.older.take())
        };
        match newer {
            Some(newer) => self.slot_mut(newer).older = older,
            None => self.newest = older,
        }
        match older {
            Some(older) => self.slot_mut(older).newer = newer,
            None => self.oldest = newer,
        }
    }

    fn claim_slot(&mut self, key: K, value: V) -> usize {
        let entry = CacheSlot {
            key,
            value,
            newer: None,
            older: None,
        };
        match self.free.pop() {
            Some(slot) => {
                self.slots[slot] = Some(entry);
                slot
            }
            None => {
                self.slots.push(Some(entry));
                self.slots.len() - 1
            }
        }
    }

    fn release_slot(&mut self, slot: usize) -> CacheSlot<K, V> {
        let entry = self.slots[slot]
            .take()
            .expect("a slot being released is always occupied");
        self.free.push(slot);
        entry
    }
}

#[cfg(test)]
#[path = "tests/bounded_lru_cache_tests.rs"]
mod tests;
