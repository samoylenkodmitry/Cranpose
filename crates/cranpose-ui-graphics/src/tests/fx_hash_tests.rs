use std::hash::Hash;

use super::*;

fn hash_of<T: Hash>(value: &T) -> u64 {
    let mut hasher = FxHasher::default();
    value.hash(&mut hasher);
    hasher.finish()
}

#[test]
fn single_words_never_collide() {
    let mut seen = std::collections::HashSet::new();
    for value in 0u64..200_000 {
        assert!(seen.insert(hash_of(&value)), "collision at {value}");
    }
}

#[test]
fn adjacent_f32_bit_patterns_stay_distinct() {
    let mut seen = std::collections::HashSet::new();
    for step in 0u32..200_000 {
        let value = f32::from_bits(0x3f80_0000 + step);
        assert!(
            seen.insert(hash_of(&value.to_bits())),
            "collision at {step}"
        );
    }
}

#[test]
fn word_order_matters() {
    assert_ne!(hash_of(&(1u64, 2u64)), hash_of(&(2u64, 1u64)));
}

#[test]
fn byte_tail_length_matters() {
    let mut short = FxHasher::default();
    short.write(&[1]);
    let mut long = FxHasher::default();
    long.write(&[1, 0]);
    assert_ne!(short.finish(), long.finish());
}

#[test]
fn strings_of_every_length_stay_distinct() {
    let mut seen = std::collections::HashSet::new();
    for len in 0..512usize {
        let value = "x".repeat(len);
        assert!(seen.insert(hash_of(&value)), "collision at len {len}");
    }
}

#[test]
fn flipping_any_single_bit_of_a_pair_changes_the_hash() {
    let base = hash_of(&(0x1234_5678_9abc_def0u64, 0u64));
    for bit in 0..64 {
        let flipped = hash_of(&(0x1234_5678_9abc_def0u64 ^ (1u64 << bit), 0u64));
        assert_ne!(base, flipped, "bit {bit} did not change the hash");
    }
}

#[test]
fn low_bits_avalanche_enough_for_hash_map_bucketing() {
    let mut buckets = [0usize; 64];
    for value in 0u64..64_000 {
        buckets[(hash_of(&value) & 63) as usize] += 1;
    }
    let expected = 64_000 / 64;
    for (index, count) in buckets.iter().enumerate() {
        assert!(
            count * 4 > expected && *count < expected * 4,
            "bucket {index} held {count} of {expected} expected"
        );
    }
}
