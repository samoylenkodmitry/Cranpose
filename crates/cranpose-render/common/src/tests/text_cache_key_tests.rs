use std::hash::{BuildHasher, DefaultHasher};

use super::*;
use crate::bounded_lru_cache::BoundedLruCache;

fn hash_of<T: Hash + ?Sized>(value: &T) -> u64 {
    std::hash::BuildHasherDefault::<DefaultHasher>::default().hash_one(value)
}

#[test]
fn a_borrowed_probe_hashes_and_compares_like_the_stored_key() {
    let text = String::from("the same words");
    let probe = TextProbe::new(text.as_str(), (14u32, 3u64));
    let stored = probe.to_owned_key();

    assert_eq!(hash_of(probe.key()), hash_of(&stored));
    assert!(probe.key() == Borrow::<dyn TextKey<(u32, u64)>>::borrow(&stored));
    assert!(
        TextProbe::new("other words", (14u32, 3u64)).key()
            != Borrow::<dyn TextKey<(u32, u64)>>::borrow(&stored)
    );
    assert!(
        TextProbe::new(text.as_str(), (15u32, 3u64)).key()
            != Borrow::<dyn TextKey<(u32, u64)>>::borrow(&stored)
    );
}

#[test]
fn a_cache_finds_an_entry_by_a_probe_of_the_same_text() {
    let mut cache = BoundedLruCache::with_capacity_at_least_one(4);
    let first = String::from("measured once");
    cache.put(TextProbe::new(first.as_str(), 1u64).to_owned_key(), 10);

    let again = String::from("measured once");
    assert_eq!(
        cache.get(TextProbe::new(again.as_str(), 1u64).key()),
        Some(&10)
    );
    assert_eq!(cache.get(TextProbe::new(again.as_str(), 2u64).key()), None);
    assert!(cache.contains(TextProbe::new(again.as_str(), 1u64).key()));
}
