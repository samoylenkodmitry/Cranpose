use super::*;

#[test]
fn an_inserted_entry_is_found_by_its_key_only() {
    let mut cache = DirectMappedCache::with_slots_log2(4);
    assert_eq!(cache.get(&7u64), None);
    cache.insert(7u64, 1.5f32);
    assert_eq!(cache.get(&7), Some(1.5));
    assert_eq!(cache.get(&8), None, "another key misses, whatever its slot");
    cache.insert(7, 2.5);
    assert_eq!(
        cache.get(&7),
        Some(2.5),
        "a key's newer value replaces its older"
    );
}

#[test]
fn a_colliding_key_takes_the_slot() {
    let mut cache = DirectMappedCache::with_slots_log2(0);
    cache.insert(1u32, 'a');
    cache.insert(2u32, 'b');
    assert_eq!(cache.get(&2), Some('b'));
    assert_eq!(
        cache.get(&1),
        None,
        "with one slot the second key evicted the first"
    );
}

#[test]
fn every_key_of_a_full_cache_is_found_while_their_slots_differ() {
    let mut cache = DirectMappedCache::with_slots_log2(10);
    for key in 0..64u64 {
        cache.insert(key, key * 3);
    }
    let found = (0..64u64)
        .filter(|key| cache.get(key) == Some(key * 3))
        .count();
    assert!(
        found >= 60,
        "64 keys in 1024 slots almost never collide, found {found}"
    );
}

#[test]
fn a_cache_takes_its_slots_with_its_first_entry() {
    let mut cache = DirectMappedCache::with_slots_log2(10);
    assert_eq!(cache.get(&3u64), None);
    assert!(cache.slots.is_empty(), "a lookup allocates nothing");
    cache.insert(3u64, 9u64);
    assert_eq!(cache.slots.len(), 1024);
    assert_eq!(cache.get(&3), Some(9));
}
