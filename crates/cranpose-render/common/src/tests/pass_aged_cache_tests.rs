use super::*;

fn passes(cache: &mut PassAgedCache<u32, &'static str>, count: u64) -> Vec<&'static str> {
    let mut dropped = Vec::new();
    for _ in 0..count {
        cache.begin_pass(|value| dropped.push(value));
    }
    dropped
}

#[test]
fn an_entry_leaves_after_idle_passes_without_a_lookup() {
    let mut cache = PassAgedCache::with_capacity_at_least_one(8);
    assert!(cache.push(1, "one").is_none());

    assert!(passes(&mut cache, IDLE_PASSES).is_empty());
    assert_eq!(cache.get(&1), Some(&"one"), "idle for exactly the limit");

    assert!(passes(&mut cache, IDLE_PASSES).is_empty());
    assert_eq!(passes(&mut cache, 1), vec!["one"]);
    assert_eq!(cache.len(), 0);
}

#[test]
fn a_lookup_keeps_an_entry_and_the_idle_ones_leave_oldest_first() {
    let mut cache = PassAgedCache::with_capacity_at_least_one(8);
    cache.push(1, "kept");
    cache.push(2, "old");
    cache.push(3, "older-use");
    let _ = cache.get(&3);
    passes(&mut cache, IDLE_PASSES / 2);
    let _ = cache.get(&1);

    let dropped = passes(&mut cache, IDLE_PASSES / 2 + 1);

    assert_eq!(dropped, vec!["old", "older-use"]);
    assert_eq!(cache.get(&1), Some(&"kept"));
    assert_eq!(cache.len(), 1);
}

#[test]
fn the_bound_still_pushes_out_the_least_recently_used() {
    let mut cache = PassAgedCache::with_capacity_at_least_one(2);
    cache.push(1, "one");
    cache.push(2, "two");
    let _ = cache.get(&1);

    assert_eq!(cache.push(3, "three"), Some((2, "two")));
    assert_eq!(
        cache.push(3, "again"),
        Some((3, "three")),
        "a replaced value"
    );
    assert_eq!(cache.pop_lru(), Some((1, "one")));
    assert_eq!(cache.len(), 1);
}
