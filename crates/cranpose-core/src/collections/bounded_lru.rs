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
    newer: Option<usize>,
    older: Option<usize>,
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
/// Each key lives in its slot. The hash index holds slot numbers and borrows
/// their keys for comparisons and rehashing, keeping large keys out of the
/// index without raw pointers or a second owner.
///
/// The index and slots grow with the entries rather than reserving the bound:
/// most caches of a process never come near it, and a table sized for
/// thousands of entries each is megabytes a small screen never touches.
pub struct BoundedLruCache<K, V> {
    index: HashTable<usize>,
    hash_builder: RandomState,
    slots: Vec<Option<CacheSlot<K, V>>>,
    free: Vec<usize>,
    newest: Option<usize>,
    oldest: Option<usize>,
    cap: NonZeroUsize,
}

impl<K, V> BoundedLruCache<K, V>
where
    K: Eq + Hash,
{
    /// Creates an empty cache that grows up to `cap` entries.
    pub fn new(cap: NonZeroUsize) -> Self {
        Self {
            index: HashTable::new(),
            hash_builder: RandomState::default(),
            slots: Vec::new(),
            free: Vec::new(),
            newest: None,
            oldest: None,
            cap,
        }
    }

    /// Creates an empty cache, treating a zero capacity as one.
    pub fn with_capacity_at_least_one(cap: usize) -> Self {
        let cap = NonZeroUsize::new(cap).unwrap_or(NonZeroUsize::MIN);
        Self::new(cap)
    }

    /// Returns the number of cached entries.
    pub fn len(&self) -> usize {
        self.index.len()
    }

    /// Returns whether the cache contains no entries.
    pub fn is_empty(&self) -> bool {
        self.index.is_empty()
    }

    /// Returns the maximum number of cached entries.
    pub fn cap(&self) -> NonZeroUsize {
        self.cap
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
        self.find_slot(key).is_some()
    }

    /// Looks up a value and marks its entry most recently used.
    pub fn get<Q>(&mut self, key: &Q) -> Option<&V>
    where
        K: Borrow<Q>,
        Q: Hash + Eq + ?Sized,
    {
        let slot = self.find_slot(key)?;
        self.promote(slot);
        Some(&self.slot(slot).value)
    }

    /// Looks up a value without changing its recency.
    pub fn peek<Q>(&self, key: &Q) -> Option<&V>
    where
        K: Borrow<Q>,
        Q: Hash + Eq + ?Sized,
    {
        let slot = self.find_slot(key)?;
        Some(&self.slot(slot).value)
    }

    /// Looks up a mutable value and marks its entry most recently used.
    pub fn get_mut<Q>(&mut self, key: &Q) -> Option<&mut V>
    where
        K: Borrow<Q>,
        Q: Hash + Eq + ?Sized,
    {
        let slot = self.find_slot(key)?;
        self.promote(slot);
        Some(&mut self.slot_mut(slot).value)
    }

    /// Inserts a most recently used entry, returning any displaced entry.
    ///
    /// An equal stored key keeps its identity: only its value is replaced,
    /// and the incoming key is returned with the previous value.
    pub fn push(&mut self, key: K, value: V) -> Option<(K, V)> {
        let hash = self.hash_builder.hash_one(&key);
        if let Some(&slot) = self.index.find(hash, |&slot| self.slot(slot).key == key) {
            self.promote(slot);
            let old_value = std::mem::replace(&mut self.slot_mut(slot).value, value);
            return Some((key, old_value));
        }

        let evicted = if self.index.len() == self.cap.get() {
            self.pop_lru()
        } else {
            None
        };

        let slot = self.claim_slot(key, value);
        self.index.insert_unique(hash, slot, |&slot| {
            let entry = self.slots[slot]
                .as_ref()
                .expect("an indexed cache slot is always occupied");
            self.hash_builder.hash_one(&entry.key)
        });
        self.link_newest(slot);
        evicted
    }

    /// Inserts a most recently used entry, returning any displaced value.
    pub fn put(&mut self, key: K, value: V) -> Option<V> {
        self.push(key, value).map(|(_, value)| value)
    }

    /// The least recently used entry, without touching its recency.
    pub fn peek_lru(&self) -> Option<(&K, &V)> {
        let entry = self.slot(self.oldest?);
        Some((&entry.key, &entry.value))
    }

    /// Removes and returns the least recently used entry.
    pub fn pop_lru(&mut self) -> Option<(K, V)> {
        let slot = self.oldest?;
        let hash = self.hash_builder.hash_one(&self.slot(slot).key);
        self.index
            .find_entry(hash, |&index| index == slot)
            .expect("a linked cache slot is always indexed")
            .remove();
        self.unlink(slot);
        let entry = self.release_slot(slot);
        Some((entry.key, entry.value))
    }

    /// Removes the entry under `key` and returns its value.
    pub fn pop(&mut self, key: &K) -> Option<V> {
        let hash = self.hash_builder.hash_one(key);
        let (slot, _) = self
            .index
            .find_entry(hash, |&slot| {
                self.slots[slot]
                    .as_ref()
                    .is_some_and(|entry| &entry.key == key)
            })
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

    fn find_slot<Q>(&self, key: &Q) -> Option<usize>
    where
        K: Borrow<Q>,
        Q: Hash + Eq + ?Sized,
    {
        self.index
            .find(self.hash_builder.hash_one(key), |&slot| {
                self.slot(slot).key.borrow() == key
            })
            .copied()
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
#[path = "tests/bounded_lru_tests.rs"]
mod tests;
