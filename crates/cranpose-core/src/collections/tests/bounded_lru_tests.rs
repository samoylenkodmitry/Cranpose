use super::BoundedLruCache;

#[derive(Clone, PartialEq, Eq, Hash)]
struct TimedTextKey {
    text: std::rc::Rc<str>,
    parameters: [u64; 3],
}

fn time_cache<K: Clone + Eq + std::hash::Hash>(
    name: &str,
    keys: &[K],
    capacity: usize,
    mode: &str,
    iterations: usize,
) {
    use std::hint::black_box;

    let mut cache = cache(capacity);
    for (index, key) in keys.iter().take(capacity).enumerate() {
        cache.put(key.clone(), [index as u64; 5]);
    }
    let start = web_time::Instant::now();
    let mut misses = 0;
    for step in 0..iterations {
        let index = match mode {
            "hits" => step * 73 % capacity,
            "mixed" if step % 32 == 0 => capacity + (step / 32) % capacity,
            "mixed" => step * 73 % (capacity * 3 / 4),
            "churn" => step % keys.len(),
            _ => unreachable!(),
        };
        let key = black_box(&keys[index]);
        if let Some(value) = cache.get(key) {
            black_box(value);
        } else {
            misses += 1;
            black_box(cache.put(key.clone(), [index as u64; 5]));
        }
    }
    let elapsed = start.elapsed();
    eprintln!(
        "CACHE_LOOKUP {name}_{capacity}_{mode} iterations={iterations} ns_per_call={} misses={misses}",
        elapsed.as_nanos() as f64 / iterations as f64
    );
    black_box(cache);
}

#[test]
#[ignore]
fn bounded_cache_timing() {
    let iterations = std::env::var("CACHE_LOOKUP_ITERATIONS")
        .ok()
        .and_then(|value| value.parse().ok())
        .unwrap_or(2_000_000);
    for capacity in [128, 8192] {
        let small: Vec<_> = (0..capacity as u64 * 2).collect();
        let text: Vec<_> = (0..capacity * 2)
            .map(|index| TimedTextKey {
                text: std::rc::Rc::from(format!("A cached text label number {index}")),
                parameters: [16, 0, 450],
            })
            .collect();
        for mode in ["hits", "mixed", "churn"] {
            time_cache("small", &small, capacity, mode, iterations);
            time_cache("text", &text, capacity, mode, iterations);
        }
    }
}

#[test]
fn each_cached_key_has_one_owner() {
    use std::rc::Rc;

    let key: Rc<str> = Rc::from("key");
    let mut cache = BoundedLruCache::with_capacity_at_least_one(2);
    cache.put(Rc::clone(&key), 7);
    assert_eq!(Rc::strong_count(&key), 2, "the cache stores the key once");
    assert_eq!(cache.get(&key), Some(&7));
    assert_eq!(cache.pop(&key), Some(7));
    assert_eq!(Rc::strong_count(&key), 1, "removal releases the stored key");
}

fn cache<K, V>(cap: usize) -> BoundedLruCache<K, V>
where
    K: Eq + std::hash::Hash,
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

#[test]
fn a_cache_reserves_room_only_for_the_entries_it_holds() {
    let mut cache: BoundedLruCache<u64, [u8; 64]> =
        BoundedLruCache::with_capacity_at_least_one(8192);
    assert_eq!(
        (cache.index.capacity(), cache.slots.capacity()),
        (0, 0),
        "a cache no text reaches must not hold room for its whole bound"
    );
    for key in 0..10 {
        cache.put(key, [0; 64]);
    }
    assert!(cache.slots.capacity() < 64, "{}", cache.slots.capacity());
    assert_eq!(cache.cap().get(), 8192);
}

#[test]
fn peek_lru_reads_the_oldest_entry_without_refreshing_it() {
    let mut cache = BoundedLruCache::with_capacity_at_least_one(3);
    cache.put(1, "one");
    cache.put(2, "two");
    cache.put(3, "three");
    assert_eq!(cache.peek_lru(), Some((&1, &"one")));
    assert_eq!(
        cache.peek_lru(),
        Some((&1, &"one")),
        "peeking keeps it oldest"
    );
    assert_eq!(cache.get(&1), Some(&"one"));
    assert_eq!(cache.peek_lru(), Some((&2, &"two")));
    assert_eq!(
        BoundedLruCache::<u32, u32>::with_capacity_at_least_one(1).peek_lru(),
        None
    );
}

#[test]
fn clear_drops_every_entry_and_keeps_the_bound() {
    let mut lru = cache::<u32, &str>(2);
    lru.put(1, "one");
    lru.put(2, "two");

    lru.clear();

    assert!(lru.is_empty());
    assert!(lru.get(&1).is_none());
    assert_eq!(lru.cap().get(), 2);
    lru.put(3, "three");
    lru.put(4, "four");
    assert_eq!(lru.push(5, "five"), Some((3, "three")));
}

#[test]
fn borrowed_text_lookups_preserve_recency_and_allow_mutation() {
    let mut cache = cache(2);
    cache.put(String::from("first"), 1);
    cache.put(String::from("second"), 2);
    assert!(cache.contains("first"));
    assert_eq!(cache.peek("first"), Some(&1));
    assert_eq!(cache.peek_lru().map(|(key, _)| key.as_str()), Some("first"));
    *cache.get_mut("first").expect("stored text") = 3;
    assert_eq!(cache.get("first"), Some(&3));
    assert_eq!(
        cache.push(String::from("third"), 4),
        Some((String::from("second"), 2))
    );
    assert!(!cache.contains("missing"));
    assert_eq!(cache.peek("missing"), None);
    assert_eq!(cache.get_mut("missing"), None);
    assert_eq!(cache.get("missing"), None);
}

#[test]
fn replacing_an_equal_key_preserves_the_stored_owner() {
    use std::rc::Rc;

    let original: Rc<str> = Rc::from("equal");
    let incoming: Rc<str> = Rc::from("equal");
    let mut cache = cache(1);
    cache.put(Rc::clone(&original), 1);
    let (returned, value) = cache.push(Rc::clone(&incoming), 2).expect("replacement");
    assert_eq!(value, 1);
    assert!(Rc::ptr_eq(&returned, &incoming));
    assert!(Rc::ptr_eq(
        cache.peek_lru().expect("cached key").0,
        &original
    ));
    drop(returned);
    assert_eq!(Rc::strong_count(&incoming), 1);
    assert_eq!(Rc::strong_count(&original), 2);
    let evicted = cache.push(Rc::from("other"), 3).expect("eviction");
    assert!(Rc::ptr_eq(&evicted.0, &original));
    drop(evicted);
    assert_eq!(Rc::strong_count(&original), 1);
    cache.put(Rc::clone(&original), 4);
    cache.clear();
    assert_eq!(Rc::strong_count(&original), 1);
    assert_eq!(cache.index.capacity(), 0);
    assert_eq!(cache.slots.capacity(), 0);
}

#[derive(Debug, PartialEq, Eq)]
struct CollidingKey(u32);

struct HashCountedKey {
    value: u32,
    hashes: std::rc::Rc<std::cell::Cell<usize>>,
}

impl PartialEq for HashCountedKey {
    fn eq(&self, other: &Self) -> bool {
        self.value == other.value
    }
}

impl Eq for HashCountedKey {}

impl std::hash::Hash for HashCountedKey {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        self.hashes.set(self.hashes.get() + 1);
        state.write_u32(self.value);
    }
}

#[test]
fn growth_and_eviction_reuse_the_stored_hashes() {
    let hashes = std::rc::Rc::new(std::cell::Cell::new(0));
    let mut cache = cache(256);
    for value in 0..512 {
        cache.put(
            HashCountedKey {
                value,
                hashes: std::rc::Rc::clone(&hashes),
            },
            value,
        );
    }
    assert_eq!(hashes.get(), 512, "each incoming key is hashed once");
    while cache.pop_lru().is_some() {}
    assert_eq!(
        hashes.get(),
        512,
        "eviction does not hash stored keys again"
    );
}

impl std::hash::Hash for CollidingKey {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        state.write_u8(0);
    }
}

#[test]
fn collisions_growth_and_slot_reuse_match_a_reference_lru() {
    for capacity in [1, 17, 129] {
        let mut cache = cache(capacity);
        let mut reference: Vec<(u32, u32)> = Vec::new();
        for key in 0..capacity as u32 {
            cache.put(CollidingKey(key), key);
            reference.insert(0, (key, key));
        }
        let mut state = 0x1234_5678_9abc_def0u64;
        for step in 0..4_000u32 {
            state = state
                .wrapping_mul(6_364_136_223_846_793_005)
                .wrapping_add(1);
            let key = ((state >> 32) % 257) as u32;
            let position = reference.iter().position(|entry| entry.0 == key);
            match (state >> 16) % 8 {
                0..=2 => {
                    let evicted = if let Some(position) = position {
                        Some(reference.remove(position))
                    } else if reference.len() == capacity {
                        reference.pop()
                    } else {
                        None
                    };
                    reference.insert(0, (key, step));
                    assert_eq!(
                        cache
                            .push(CollidingKey(key), step)
                            .map(|(key, value)| (key.0, value)),
                        evicted
                    );
                }
                3 | 4 => {
                    let change = (state >> 16) % 8 == 4;
                    let expected = position.map(|position| {
                        let mut entry = reference.remove(position);
                        if change {
                            entry.1 ^= step;
                        }
                        reference.insert(0, entry);
                        entry.1
                    });
                    let actual = if change {
                        cache.get_mut(&CollidingKey(key)).map(|value| {
                            *value ^= step;
                            *value
                        })
                    } else {
                        cache.get(&CollidingKey(key)).copied()
                    };
                    assert_eq!(actual, expected);
                }
                5 => {
                    let expected = position.map(|position| reference.remove(position).1);
                    assert_eq!(cache.pop(&CollidingKey(key)), expected);
                }
                6 => assert_eq!(
                    cache.pop_lru().map(|(key, value)| (key.0, value)),
                    reference.pop()
                ),
                _ => {
                    assert_eq!(cache.contains(&CollidingKey(key)), position.is_some());
                    assert_eq!(
                        cache.peek(&CollidingKey(key)).copied(),
                        position.map(|position| reference[position].1)
                    );
                }
            }
            assert_eq!(cache.len(), reference.len());
            assert_eq!(cache.is_empty(), reference.is_empty());
            assert_eq!(
                cache.peek_lru().map(|(key, value)| (key.0, *value)),
                reference.last().copied()
            );
            assert!(
                cache
                    .iter()
                    .map(|(key, value)| (key.0, *value))
                    .eq(reference.iter().copied())
            );
        }
    }
}

#[test]
fn pass_aged_cache_accepts_keys_without_clone() {
    use crate::collections::pass_aged::{IDLE_PASSES, PassAgedCache};

    let mut cache = PassAgedCache::with_capacity_at_least_one(1);
    assert_eq!(cache.push(CollidingKey(1), 7), None);
    assert_eq!(cache.get(&CollidingKey(1)), Some(&7));
    let mut dropped = Vec::new();
    for _ in 0..=IDLE_PASSES {
        cache.begin_pass(|value| dropped.push(value));
    }
    assert_eq!(dropped, vec![7]);
    assert!(cache.is_empty());
}
