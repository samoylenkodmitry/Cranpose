use cranpose_core::collections::map::HashMap;
use std::hash::Hash;
use std::num::NonZeroUsize;

struct CacheEntry<V> {
    value: V,
    last_used: u64,
}

/// Small bounded LRU cache used by renderer hot-path caches.
///
/// Hits update recency in place, so the common path is a single hash lookup.
/// Eviction scans the bounded table and happens only when inserting into a full
/// cache.
pub struct BoundedLruCache<K, V> {
    entries: HashMap<K, CacheEntry<V>>,
    cap: NonZeroUsize,
    clock: u64,
}

impl<K, V> BoundedLruCache<K, V>
where
    K: Clone + Eq + Hash,
{
    pub fn new(cap: NonZeroUsize) -> Self {
        Self {
            entries: HashMap::with_capacity(cap.get()),
            cap,
            clock: 0,
        }
    }

    pub fn with_capacity_at_least_one(cap: usize) -> Self {
        let cap = NonZeroUsize::new(cap).unwrap_or(NonZeroUsize::MIN);
        Self::new(cap)
    }

    pub fn len(&self) -> usize {
        self.entries.len()
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    pub fn cap(&self) -> NonZeroUsize {
        self.cap
    }

    pub fn contains(&self, key: &K) -> bool {
        self.entries.contains_key(key)
    }

    pub fn get(&mut self, key: &K) -> Option<&V> {
        let tick = self.next_tick();
        let entry = self.entries.get_mut(key)?;
        entry.last_used = tick;
        Some(&entry.value)
    }

    pub fn peek(&self, key: &K) -> Option<&V> {
        self.entries.get(key).map(|entry| &entry.value)
    }

    pub fn get_mut(&mut self, key: &K) -> Option<&mut V> {
        let tick = self.next_tick();
        let entry = self.entries.get_mut(key)?;
        entry.last_used = tick;
        Some(&mut entry.value)
    }

    pub fn push(&mut self, key: K, value: V) -> Option<(K, V)> {
        let tick = self.next_tick();
        if let Some(entry) = self.entries.get_mut(&key) {
            entry.last_used = tick;
            let old_value = std::mem::replace(&mut entry.value, value);
            return Some((key, old_value));
        }

        let evicted = if self.entries.len() == self.cap.get() {
            self.pop_lru()
        } else {
            None
        };
        self.entries.insert(
            key,
            CacheEntry {
                value,
                last_used: tick,
            },
        );
        evicted
    }

    pub fn put(&mut self, key: K, value: V) -> Option<V> {
        self.push(key, value).map(|(_, value)| value)
    }

    pub fn pop_lru(&mut self) -> Option<(K, V)> {
        let key = self
            .entries
            .iter()
            .min_by_key(|(_, entry)| entry.last_used)
            .map(|(key, _)| key.clone())?;
        self.entries
            .remove_entry(&key)
            .map(|(key, entry)| (key, entry.value))
    }

    pub fn pop(&mut self, key: &K) -> Option<V> {
        self.entries.remove(key).map(|entry| entry.value)
    }

    pub fn iter(&self) -> impl Iterator<Item = (&K, &V)> {
        let mut entries = self.entries.iter().collect::<Vec<_>>();
        entries.sort_by_key(|(_, entry)| std::cmp::Reverse(entry.last_used));
        entries.into_iter().map(|(key, entry)| (key, &entry.value))
    }

    fn next_tick(&mut self) -> u64 {
        self.clock = self.clock.saturating_add(1);
        self.clock
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
