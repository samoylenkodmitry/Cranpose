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
