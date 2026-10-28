//! A map from strings to values whose keys are all known when the program
//! is built. [`PerfectMap::new`] runs in a `const` context and finds a hash
//! that gives every key a slot of its own, so a lookup hashes the key once,
//! reads one slot and compares one key. The search follows PTHash: the keys
//! fall into buckets, and each bucket, the fullest first, takes the first
//! pilot that moves all its keys to free slots. The hash folds the key in
//! 8-byte lanes, one multiply a lane.

// The buckets the keys fall into, and the slots they are placed in.
const BUCKETS: usize = 32;
const SLOTS: usize = 128;
// A slot no key took.
const EMPTY: u8 = u8::MAX;
// Seeds tried before a set of keys is refused.
const SEEDS: u64 = 64;

/// The most keys a [`PerfectMap`] holds: three of every four slots.
pub const MAX_KEYS: usize = SLOTS * 3 / 4;

/// A map from the strings of a fixed set to values, built at compile time.
///
/// ```
/// use cranpose_core::collections::perfect_map::PerfectMap;
///
/// const DIGITS: PerfectMap<u8, 3> = PerfectMap::new([("zero", 0), ("one", 1), ("two", 2)]);
/// assert_eq!(DIGITS.get("one"), Some(&1));
/// assert_eq!(DIGITS.get("three"), None);
/// ```
pub struct PerfectMap<V: Copy + 'static, const N: usize> {
    entries: [(&'static str, V); N],
    seed: u64,
    pilots: [u8; BUCKETS],
    slots: [u8; SLOTS],
}

impl<V: Copy + 'static, const N: usize> PerfectMap<V, N> {
    /// The map of `entries`. Called in a `const`, it fails the build when two
    /// keys are equal, when there are more than [`MAX_KEYS`], or when no seed
    /// places every key.
    pub const fn new(entries: [(&'static str, V); N]) -> Self {
        assert!(N <= MAX_KEYS, "a PerfectMap holds at most MAX_KEYS keys");
        let mut seed = 0;
        while seed < SEEDS {
            seed += 1;
            if let Some((pilots, slots)) = place(&entries, seed) {
                return Self {
                    entries,
                    seed,
                    pilots,
                    slots,
                };
            }
        }
        panic!("no seed places these keys: is one of them there twice?");
    }

    /// The value of `key`, if it is one of the map's keys.
    #[inline]
    pub fn get(&self, key: &str) -> Option<&V> {
        let hash = lane_hash(self.seed, key.as_bytes());
        let slot = self.slots[slot_of(hash, self.pilots[bucket_of(hash)])];
        let (name, value) = self.entries.get(usize::from(slot))?;
        (*name == key).then_some(value)
    }
}

// Folds `bytes` into the seed 8 bytes at a time, each lane with one
// multiply, then mixes the high bits into the low ones the bucket reads.
const fn lane_hash(seed: u64, bytes: &[u8]) -> u64 {
    let mut hash = seed ^ (bytes.len() as u64).wrapping_mul(0x9e37_79b9_7f4a_7c15);
    let mut at = 0;
    while at < bytes.len() {
        let mut word = 0u64;
        let mut index = 0;
        while index < 8 && at + index < bytes.len() {
            word |= (bytes[at + index] as u64) << (8 * index);
            index += 1;
        }
        hash = (hash ^ word).wrapping_mul(0xbf58_476d_1ce4_e5b9);
        hash ^= hash >> 31;
        at += 8;
    }
    // A multiply carries a difference only to higher bits: keys that differ
    // in their last byte, as "key 94" and "key 95", left the low bits alike
    // and 96 such keys found no seed. One more multiply between two shifts
    // spreads them.
    hash = (hash ^ (hash >> 32)).wrapping_mul(0x94d0_49bb_1331_11eb);
    hash ^ (hash >> 29)
}

// The hash modulo BUCKETS, read from its low byte: BUCKETS divides 256.
const fn bucket_of(hash: u64) -> usize {
    hash.to_le_bytes()[0] as usize % BUCKETS
}
const _: () = assert!(256 % BUCKETS == 0, "a hash's low byte holds its bucket");

// The slot a key of `hash` takes with `pilot`: the top seven bits of the
// hash mixed with the pilot.
const fn slot_of(hash: u64, pilot: u8) -> usize {
    let mixed = (hash ^ (pilot as u64).wrapping_mul(0x94d0_49bb_1331_11eb))
        .wrapping_mul(0x9e37_79b9_7f4a_7c15);
    // The top bits, read from the top byte: SLOTS is at most 256.
    (mixed.to_be_bytes()[0] >> (8 - SLOTS.trailing_zeros())) as usize
}
const _: () = assert!(
    SLOTS.is_power_of_two() && SLOTS <= 256,
    "a hash's top byte holds its slot"
);
const _: () = assert!(MAX_KEYS < 256, "a key's index fits its slot's byte");

// Every key's slot under `seed`: each bucket, the fullest first, takes the
// first pilot that gives its keys free slots. `None` when a bucket finds
// none.
const fn place<V: Copy, const N: usize>(
    entries: &[(&'static str, V); N],
    seed: u64,
) -> Option<([u8; BUCKETS], [u8; SLOTS])> {
    let mut hashes = [0u64; N];
    let mut sizes = [0usize; BUCKETS];
    let mut key = 0;
    while key < N {
        hashes[key] = lane_hash(seed, entries[key].0.as_bytes());
        sizes[bucket_of(hashes[key])] += 1;
        key += 1;
    }
    let mut pilots = [0u8; BUCKETS];
    let mut slots = [EMPTY; SLOTS];
    let mut size = N;
    while size > 0 {
        let mut bucket = 0;
        while bucket < BUCKETS {
            if sizes[bucket] == size {
                match place_bucket(&hashes, bucket, &mut slots) {
                    Some(pilot) => pilots[bucket] = pilot,
                    None => return None,
                }
            }
            bucket += 1;
        }
        size -= 1;
    }
    Some((pilots, slots))
}

// Puts the keys of `bucket` in `slots` with the first pilot that finds each
// a free slot, and returns that pilot.
const fn place_bucket<const N: usize>(
    hashes: &[u64; N],
    bucket: usize,
    slots: &mut [u8; SLOTS],
) -> Option<u8> {
    let mut pilot = 0u8;
    loop {
        if bucket_fits(hashes, bucket, slots, pilot) {
            let mut key = 0;
            while key < N {
                if bucket_of(hashes[key]) == bucket {
                    // A key's index is its low byte: N is at most MAX_KEYS, below 256.
                    slots[slot_of(hashes[key], pilot)] = key.to_le_bytes()[0];
                }
                key += 1;
            }
            return Some(pilot);
        }
        if pilot == u8::MAX {
            return None;
        }
        pilot += 1;
    }
}

// Whether every key of `bucket` finds a free slot with `pilot`, no two the
// same.
const fn bucket_fits<const N: usize>(
    hashes: &[u64; N],
    bucket: usize,
    slots: &[u8; SLOTS],
    pilot: u8,
) -> bool {
    let mut taken = [false; SLOTS];
    let mut key = 0;
    while key < N {
        if bucket_of(hashes[key]) == bucket {
            let slot = slot_of(hashes[key], pilot);
            if slots[slot] != EMPTY || taken[slot] {
                return false;
            }
            taken[slot] = true;
        }
        key += 1;
    }
    true
}

#[cfg(test)]
#[path = "tests/perfect_map_tests.rs"]
mod tests;
