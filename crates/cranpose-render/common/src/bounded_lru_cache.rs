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
            index: HashMap::with_capacity(cap.get()),
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
mod tests {
    use super::BoundedLruCache;

    fn cache<K, V>(cap: usize) -> BoundedLruCache<K, V>
    where
        K: Clone + Eq + std::hash::Hash,
    {
        BoundedLruCache::with_capacity_at_least_one(cap)
    }

    #[test]
    fn clamped_constructor_uses_minimum_nonzero_capacity() {
        let mut cache = BoundedLruCache::with_capacity_at_least_one(0);
        assert_eq!(cache.cap().get(), 1);
        assert_eq!(cache.push("a", 1), None);
        assert_eq!(cache.push("b", 2), Some(("a", 1)));
        assert_eq!(cache.get(&"b"), Some(&2));
    }

    #[test]
    fn get_promotes_entry_and_push_evicts_lru() {
        let mut cache = cache(2);
        assert_eq!(cache.push("a", 1), None);
        assert_eq!(cache.push("b", 2), None);

        assert_eq!(cache.get(&"a"), Some(&1));
        assert_eq!(cache.push("c", 3), Some(("b", 2)));

        assert!(cache.contains(&"a"));
        assert!(cache.contains(&"c"));
        assert!(!cache.contains(&"b"));
    }

    #[test]
    fn push_existing_replaces_value_and_keeps_capacity() {
        let mut cache = cache(2);
        cache.push("a", 1);
        cache.push("b", 2);

        assert_eq!(cache.push("a", 3), Some(("a", 1)));
        assert_eq!(cache.len(), 2);
        assert_eq!(cache.get(&"a"), Some(&3));
    }

    #[test]
    fn pop_removes_requested_entry_and_preserves_lru_order() {
        let mut cache = cache(3);
        cache.push("a", 1);
        cache.push("b", 2);
        cache.push("c", 3);

        assert_eq!(cache.pop(&"b"), Some(2));
        assert_eq!(cache.get(&"a"), Some(&1));
        assert_eq!(cache.pop_lru(), Some(("c", 3)));
        assert_eq!(cache.len(), 1);
    }

    #[test]
    fn a_full_cache_that_only_misses_keeps_evicting_in_order() {
        let mut cache = cache(4);
        for step in 0..4 {
            assert_eq!(cache.push(step, step * 10), None);
        }

        for step in 4..64 {
            let evicted = cache.push(step, step * 10);
            assert_eq!(
                evicted,
                Some((step - 4, (step - 4) * 10)),
                "insert {step} must evict the oldest entry"
            );
            assert_eq!(cache.len(), 4);
        }

        let live: Vec<_> = cache.iter().map(|(key, value)| (*key, *value)).collect();
        assert_eq!(live, vec![(63, 630), (62, 620), (61, 610), (60, 600)]);
    }

    #[test]
    fn reused_slots_do_not_resurrect_the_entries_that_vacated_them() {
        let mut cache = cache(3);
        cache.push("a", 1);
        cache.push("b", 2);
        cache.push("c", 3);

        assert_eq!(cache.pop(&"b"), Some(2));
        assert_eq!(cache.push("d", 4), None);

        assert!(!cache.contains(&"b"));
        assert_eq!(cache.peek(&"d"), Some(&4));
        assert_eq!(cache.len(), 3);
        assert_eq!(cache.pop_lru(), Some(("a", 1)));
        assert_eq!(cache.pop_lru(), Some(("c", 3)));
        assert_eq!(cache.pop_lru(), Some(("d", 4)));
        assert_eq!(cache.pop_lru(), None);
        assert!(cache.is_empty());
    }

    #[test]
    fn a_promoted_entry_survives_the_next_eviction() {
        let mut cache = cache(3);
        cache.push("a", 1);
        cache.push("b", 2);
        cache.push("c", 3);

        assert_eq!(cache.get(&"a"), Some(&1));
        assert_eq!(cache.get_mut(&"b").map(|value| *value), Some(2));

        assert_eq!(cache.push("d", 4), Some(("c", 3)));
        assert!(cache.contains(&"a"));
        assert!(cache.contains(&"b"));
    }

    #[test]
    fn peek_reads_without_promoting_entry() {
        let mut cache = cache(2);
        cache.push("a", 1);
        cache.push("b", 2);

        assert_eq!(cache.peek(&"a"), Some(&1));
        assert_eq!(cache.push("c", 3), Some(("a", 1)));
    }

    #[test]
    fn iter_reports_mru_to_lru_entries() {
        let mut cache = cache(3);
        cache.push("a", 1);
        cache.push("b", 2);
        cache.get(&"a");

        let entries: Vec<_> = cache.iter().map(|(key, value)| (*key, *value)).collect();
        assert_eq!(entries, vec![("a", 1), ("b", 2)]);
    }
}
