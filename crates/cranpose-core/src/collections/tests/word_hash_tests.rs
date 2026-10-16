use std::hash::{BuildHasher, BuildHasherDefault};

use super::WordHasher;

fn hash(key: impl std::hash::Hash) -> u32 {
    BuildHasherDefault::<WordHasher>::default().hash_one(key) as u32
}

fn distinct(values: impl Iterator<Item = u32>) -> usize {
    let mut values: Vec<u32> = values.collect();
    values.sort_unstable();
    values.dedup();
    values.len()
}

#[test]
fn aligned_keys_spread_over_buckets() {
    // Addresses and ids that are multiples of a power of two share their low
    // bits; a table of 1024 buckets picks its bucket by the hash's low bits.
    // Random hashes would fill about 647 of them.
    for stride in [1u32, 3, 8, 64, 4096] {
        let buckets = distinct((0..1024u32).map(|key| hash(key * stride) & 1023));
        assert!(buckets > 560, "stride {stride}: {buckets} buckets of 1024");
    }
}

#[test]
fn keys_spread_over_control_tags() {
    // A 32-bit table tags each entry with the top seven bits of the hash.
    let tags = distinct((0..1024u32).map(|key| hash(key) >> 25));
    assert!(tags > 100, "{tags} tags of 128");
}

#[test]
fn every_word_of_a_key_counts() {
    assert_ne!(hash(1u64), hash(1u64 << 32));
    assert_ne!(hash((1u32, 2u32)), hash((2u32, 1u32)));
    assert_ne!(hash("abcde"), hash("abcdf"));
}
