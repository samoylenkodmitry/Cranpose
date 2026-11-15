use std::{borrow::Borrow, hash::Hash, num::NonZeroUsize};

use super::linked_slots::{LinkedSlots, Order};

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
    slots: LinkedSlots<K, V>,
    order: Order,
    cap: NonZeroUsize,
}

impl<K, V> BoundedLruCache<K, V>
where
    K: Eq + Hash,
{
    /// Creates an empty cache that holds at most `cap` entries.
    pub fn new(cap: NonZeroUsize) -> Self {
        Self {
            slots: LinkedSlots::new(),
            order: Order::default(),
            cap,
        }
    }

    /// Creates an empty cache bounded to `cap` entries, or to one entry when `cap` is zero.
    pub fn with_capacity_at_least_one(cap: usize) -> Self {
        let cap = NonZeroUsize::new(cap).unwrap_or(NonZeroUsize::MIN);
        Self::new(cap)
    }

    /// Returns the number of entries the cache holds.
    pub fn len(&self) -> usize {
        self.slots.len()
    }

    /// Returns `true` if the cache holds no entries.
    pub fn is_empty(&self) -> bool {
        self.slots.is_empty()
    }

    /// Returns the largest number of entries the cache holds.
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
        self.slots.find(key).is_some()
    }

    /// Returns the value under `key`, marking the entry most recently used.
    pub fn get<Q>(&mut self, key: &Q) -> Option<&V>
    where
        K: Borrow<Q>,
        Q: Hash + Eq + ?Sized,
    {
        let slot = self.slots.find(key)?;
        self.slots.promote(slot, &mut self.order);
        Some(&self.slots.slot(slot).value)
    }

    /// Returns the value under `key` without changing its recency.
    pub fn peek<Q>(&self, key: &Q) -> Option<&V>
    where
        K: Borrow<Q>,
        Q: Hash + Eq + ?Sized,
    {
        let slot = self.slots.find(key)?;
        Some(&self.slots.slot(slot).value)
    }

    /// Returns the value under `key` for change, marking the entry most recently used.
    pub fn get_mut<Q>(&mut self, key: &Q) -> Option<&mut V>
    where
        K: Borrow<Q>,
        Q: Hash + Eq + ?Sized,
    {
        let slot = self.slots.find(key)?;
        self.slots.promote(slot, &mut self.order);
        Some(&mut self.slots.slot_mut(slot).value)
    }

    /// Returns the value under `key`, marking the entry most recently used,
    /// after storing the value `value` makes under it when the cache has none,
    /// which pushes the least recently used entry out of a full cache. A hit
    /// takes one hash lookup.
    pub fn find_or_insert_with(&mut self, key: K, value: impl FnOnce() -> V) -> &V {
        let slot = match self.slots.find(&key) {
            Some(slot) => {
                self.slots.promote(slot, &mut self.order);
                slot
            }
            None => self.insert_new(key, value()).0,
        };
        &self.slots.slot(slot).value
    }

    /// Stores `value` under `key` and marks it most recently used. Returns the entry it
    /// replaced or, when the cache was full, the least recently used entry it pushed out.
    pub fn push(&mut self, key: K, value: V) -> Option<(K, V)> {
        if let Some(slot) = self.slots.find(&key) {
            self.slots.promote(slot, &mut self.order);
            let old_value = std::mem::replace(&mut self.slots.slot_mut(slot).value, value);
            return Some((key, old_value));
        }
        self.insert_new(key, value).1
    }

    // Stores `value` under `key`, which the cache lacks, as the most recently
    // used entry: its slot, and the least recently used entry it pushed out
    // of a full cache. Out of line, as a lookup mostly finds its entry.
    #[inline(never)]
    fn insert_new(&mut self, key: K, value: V) -> (u32, Option<(K, V)>) {
        let evicted = if self.slots.len() == self.cap.get() {
            self.pop_lru()
        } else {
            None
        };

        let hash = self.slots.hash(&key);
        let slot = self.slots.insert(hash, (key, value), self.cap);
        self.slots.link_newest(slot, &mut self.order);
        (slot, evicted)
    }

    /// Stores `value` under `key` and returns the value it replaced, or the value of the least
    /// recently used entry it pushed out. Returns `None` when nothing was removed.
    pub fn put(&mut self, key: K, value: V) -> Option<V> {
        self.push(key, value).map(|(_, value)| value)
    }

    /// The least recently used entry, without touching its recency.
    pub fn peek_lru(&self) -> Option<(&K, &V)> {
        let entry = self.slots.slot(self.order.oldest?);
        Some((&entry.key, &entry.value))
    }

    /// Removes and returns the least recently used entry, or `None` if the cache is empty.
    pub fn pop_lru(&mut self) -> Option<(K, V)> {
        let slot = self.order.oldest?;
        let (_, entry) = self.slots.remove(slot, &mut self.order);
        Some((entry.key, entry.value))
    }

    /// Removes the entry under `key` and returns its value, or `None` if there is none.
    pub fn pop(&mut self, key: &K) -> Option<V> {
        self.slots
            .take(key, &mut self.order)
            .map(|entry| entry.value)
    }

    /// Entries most recently used first.
    pub fn iter(&self) -> impl Iterator<Item = (&K, &V)> {
        self.slots.iter(self.order)
    }
}

#[cfg(test)]
#[path = "tests/bounded_lru_tests.rs"]
mod tests;
