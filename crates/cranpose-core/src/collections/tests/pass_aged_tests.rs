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
    let mut cache = PassAgedCache::with_capacity_at_least_one(16);
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
    let mut cache = PassAgedCache::with_capacity_at_least_one(8);
    cache.push(1, "one");
    cache.push(2, "two");
    let _ = cache.get(&1);
    cache.push(3, "three");

    assert_eq!(cache.push(4, "four"), Some((2, "two")));
    assert_eq!(
        cache.push(4, "again"),
        Some((4, "four")),
        "a replaced value"
    );
    assert_eq!(cache.get(&1), Some(&"one"));
    assert_eq!(cache.len(), 3);
}

#[test]
fn a_stream_of_entries_used_once_leaves_the_hit_ones_in_place() {
    let mut cache = PassAgedCache::with_capacity_at_least_one(8);
    cache.push(0, "kept");
    let _ = cache.get(&0);

    for price in 1..=100 {
        cache.push(price, "price");
        let _ = passes(&mut cache, 1);
    }

    assert_eq!(cache.get(&0), Some(&"kept"));
    assert_eq!(cache.len(), 3, "a quarter of the bound holds the stream");
}

#[test]
fn clear_drops_every_entry() {
    let mut cache = PassAgedCache::with_capacity_at_least_one(4);
    cache.push(1, "one");
    cache.push(2, "two");

    assert!(!cache.is_empty());
    cache.clear();

    assert!(cache.is_empty());
    assert!(cache.get(&1).is_none());
    assert!(passes(&mut cache, IDLE_PASSES + 1).is_empty());
}
