use super::*;

fn passes(cache: &mut PassAgedCache<u32, &'static str>, count: u64) -> Vec<&'static str> {
    let mut dropped = Vec::new();
    for _ in 0..count {
        cache.begin_pass(IDLE_PASSES, |value| dropped.push(value));
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
    assert_eq!(cache.len(), 2);
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

#[test]
fn a_key_a_pass_asks_for_again_outlasts_the_keys_it_asks_for_once() {
    let mut cache = PassAgedCache::with_capacity_at_least_one(4);
    cache.push(0, "asked for twice");
    assert!(cache.get(&0).is_some());

    for key in 1..100 {
        cache.push(key, "asked for once");
    }

    assert_eq!(cache.get(&0), Some(&"asked for twice"));
    assert_eq!(cache.len(), 4);
}

#[test]
fn a_key_that_comes_back_after_it_left_for_room_outlasts_the_keys_seen_once() {
    let mut cache = PassAgedCache::with_capacity_at_least_one(4);
    for key in 0..5 {
        cache.push(key, "seen once");
    }
    assert!(cache.get(&0).is_none(), "the oldest key left for room");

    cache.push(0, "came back");
    for key in 5..100 {
        cache.push(key, "seen once");
    }

    assert_eq!(cache.get(&0), Some(&"came back"));
}

#[test]
fn a_new_pass_ends_the_standing_of_the_keys_the_last_one_asked_for_again() {
    let mut cache = PassAgedCache::with_capacity_at_least_one(2);
    cache.push(1, "asked for twice");
    assert!(cache.get(&1).is_some());
    cache.push(2, "asked for once");

    cache.begin_pass(IDLE_PASSES, drop);
    cache.push(3, "new");
    cache.push(4, "newer");

    assert!(cache.get(&1).is_none(), "the least recently used leaves");
    assert_eq!(cache.get(&3), Some(&"new"));
    assert_eq!(cache.get(&4), Some(&"newer"));
}
