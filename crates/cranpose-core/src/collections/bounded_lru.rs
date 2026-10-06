use std::{
    borrow::Borrow,
    hash::{BuildHasher, Hash},
    num::NonZeroUsize,
};

use hashbrown::HashTable;

use crate::collections::map::RandomState;

struct CacheSlot<K, V> {
    key: K,
    value: V,
    newer: Option<u32>,
    older: Option<u32>,
}

/// A small bounded LRU cache for hot-path caches.
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
/// The index holds slot numbers only and compares a probe against the key in
/// its slot, so each key is stored once: a full text cache's index takes
/// five bytes an entry instead of a second copy of every key.
///
/// The index and slots grow with the entries rather than reserving the bound:
/// most caches of a process never come near it, and a table sized for
/// thousands of entries each is megabytes a small screen never touches.
pub struct BoundedLruCache<K, V> {
    index: HashTable<u32>,
    hasher: RandomState,
    slots: Vec<Option<CacheSlot<K, V>>>,
    free: Vec<u32>,
    newest: Option<u32>,
    oldest: Option<u32>,
    cap: NonZeroUsize,
}

/// The occupied slot `slot` of `slots`.
fn occupied<K, V>(slots: &[Option<CacheSlot<K, V>>], slot: u32) -> &CacheSlot<K, V> {
    slots[slot as usize]
        .as_ref()
        .expect("an indexed or linked cache slot is always occupied")
}

impl<K, V> BoundedLruCache<K, V>
where
    K: Eq + Hash,
{
    pub fn new(cap: NonZeroUsize) -> Self {
        Self {
            index: HashTable::new(),
            hasher: RandomState::default(),
            slots: Vec::new(),
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

    /// Sets the bound, evicting the least recently used entries past it.
    pub fn set_cap(&mut self, cap: NonZeroUsize) {
        self.cap = cap;
        while self.len() > cap.get() {
            self.pop_lru();
        }
    }

    /// Drops every entry and gives back the storage they took.
    pub fn clear(&mut self) {
        *self = Self::new(self.cap);
    }

    /// The lookups take any form of the key the stored key borrows as, so a
    /// caller can probe with a borrowed view instead of building an owned key.
    pub fn contains<Q>(&self, key: &Q) -> bool
    where
        K: Borrow<Q>,
        Q: Hash + Eq + ?Sized,
    {
        self.find(key).is_some()
    }

    pub fn get<Q>(&mut self, key: &Q) -> Option<&V>
    where
        K: Borrow<Q>,
        Q: Hash + Eq + ?Sized,
    {
        let slot = self.find(key)?;
        self.promote(slot);
        Some(&self.slot(slot).value)
    }

    pub fn peek<Q>(&self, key: &Q) -> Option<&V>
    where
        K: Borrow<Q>,
        Q: Hash + Eq + ?Sized,
    {
        let slot = self.find(key)?;
        Some(&self.slot(slot).value)
    }

    pub fn get_mut<Q>(&mut self, key: &Q) -> Option<&mut V>
    where
        K: Borrow<Q>,
        Q: Hash + Eq + ?Sized,
    {
        let slot = self.find(key)?;
        self.promote(slot);
        Some(&mut self.slot_mut(slot).value)
    }

    pub fn push(&mut self, key: K, value: V) -> Option<(K, V)> {
        if let Some(slot) = self.find(&key) {
            self.promote(slot);
            let old_value = std::mem::replace(&mut self.slot_mut(slot).value, value);
            return Some((key, old_value));
        }

        let evicted = if self.index.len() == self.cap.get() {
            self.pop_lru()
        } else {
            None
        };

        let hash = self.hasher.hash_one(&key);
        let slot = self.claim_slot(key, value);
        let (slots, hasher) = (&self.slots, &self.hasher);
        self.index.insert_unique(hash, slot, |&slot| {
            hasher.hash_one(&occupied(slots, slot).key)
        });
        self.link_newest(slot);
        evicted
    }

    pub fn put(&mut self, key: K, value: V) -> Option<V> {
        self.push(key, value).map(|(_, value)| value)
    }

    /// The least recently used entry, without touching its recency.
    pub fn peek_lru(&self) -> Option<(&K, &V)> {
        let entry = self.slot(self.oldest?);
        Some((&entry.key, &entry.value))
    }

    pub fn pop_lru(&mut self) -> Option<(K, V)> {
        let slot = self.oldest?;
        let hash = self.hasher.hash_one(&self.slot(slot).key);
        if let Ok(entry) = self.index.find_entry(hash, |&indexed| indexed == slot) {
            entry.remove();
        }
        self.unlink(slot);
        let entry = self.release_slot(slot);
        Some((entry.key, entry.value))
    }

    pub fn pop(&mut self, key: &K) -> Option<V> {
        let hash = self.hasher.hash_one(key);
        let slots = &self.slots;
        let (slot, _) = self
            .index
            .find_entry(hash, |&slot| occupied(slots, slot).key == *key)
            .ok()?
            .remove();
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

    fn find<Q>(&self, key: &Q) -> Option<u32>
    where
        K: Borrow<Q>,
        Q: Hash + Eq + ?Sized,
    {
        let slots = &self.slots;
        self.index
            .find(self.hasher.hash_one(key), |&slot| {
                occupied(slots, slot).key.borrow() == key
            })
            .copied()
    }

    fn slot(&self, slot: u32) -> &CacheSlot<K, V> {
        occupied(&self.slots, slot)
    }

    fn slot_mut(&mut self, slot: u32) -> &mut CacheSlot<K, V> {
        self.slots[slot as usize]
            .as_mut()
            .expect("a linked cache slot is always occupied")
    }

    fn promote(&mut self, slot: u32) {
        if self.newest == Some(slot) {
            return;
        }
        self.unlink(slot);
        self.link_newest(slot);
    }

    fn link_newest(&mut self, slot: u32) {
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

    fn unlink(&mut self, slot: u32) {
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

    fn claim_slot(&mut self, key: K, value: V) -> u32 {
        let entry = CacheSlot {
            key,
            value,
            newer: None,
            older: None,
        };
        match self.free.pop() {
            Some(slot) => {
                self.slots[slot as usize] = Some(entry);
                slot
            }
            None => {
                let slot =
                    u32::try_from(self.slots.len()).expect("a cache holds fewer than 2^32 entries");
                self.slots.push(Some(entry));
                slot
            }
        }
    }

    fn release_slot(&mut self, slot: u32) -> CacheSlot<K, V> {
        let entry = self.slots[slot as usize]
            .take()
            .expect("a slot being released is always occupied");
        self.free.push(slot);
        entry
    }
}

#[cfg(test)]
#[path = "tests/bounded_lru_tests.rs"]
mod tests;
