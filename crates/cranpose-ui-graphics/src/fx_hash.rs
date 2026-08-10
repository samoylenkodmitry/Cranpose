use std::hash::{BuildHasherDefault, Hasher};

/// Multiplier from the FxHash family: odd, so every `wrapping_mul` step is a
/// bijection on `u64` and folding a word can never lose accumulator state.
const FOLD: u64 = 0x517c_c1b7_2722_0a95;

/// Rotation applied before each fold so that a word's bits reach every lane of
/// the accumulator across successive words.
const FOLD_ROTATE: u32 = 5;

const FINALIZE_A: u64 = 0xbf58_476d_1ce4_e5b9;
const FINALIZE_B: u64 = 0x94d0_49bb_1331_11eb;

/// Non-cryptographic hasher for in-memory frame-to-frame change detection.
///
/// Every step of the fold — rotate, xor with the next word, multiply by an odd
/// constant — is a bijection on `u64`, so equal-length inputs never collide
/// structurally; only the unavoidable 64-bit pigeonhole remains. [`finish`]
/// applies the SplitMix64 finalizer, itself a bijection, which adds no
/// collisions and gives full avalanche so the result is safe to use directly as
/// a `HashMap` key hash.
///
/// [`finish`]: Hasher::finish
#[derive(Clone, Copy, Default)]
pub struct FxHasher {
    hash: u64,
}

/// [`std::hash::BuildHasher`] for [`FxHasher`], for `HashMap`/`HashSet` on the
/// per-frame path.
pub type FxBuildHasher = BuildHasherDefault<FxHasher>;

impl FxHasher {
    #[inline]
    fn fold(&mut self, word: u64) {
        self.hash = (self.hash.rotate_left(FOLD_ROTATE) ^ word).wrapping_mul(FOLD);
    }
}

impl Hasher for FxHasher {
    #[inline]
    fn write(&mut self, bytes: &[u8]) {
        let mut chunks = bytes.chunks_exact(8);
        for chunk in &mut chunks {
            let mut word = [0u8; 8];
            word.copy_from_slice(chunk);
            self.fold(u64::from_le_bytes(word));
        }
        // The tail is folded together with its own length, so `[1]` and `[1, 0]`
        // stay distinct even though both pad to the same word.
        let tail = chunks.remainder();
        if !tail.is_empty() {
            let mut word = [0u8; 8];
            word[..tail.len()].copy_from_slice(tail);
            self.fold(u64::from_le_bytes(word) ^ ((tail.len() as u64) << 56));
        }
    }

    #[inline]
    fn write_u8(&mut self, value: u8) {
        self.fold(u64::from(value));
    }

    #[inline]
    fn write_u16(&mut self, value: u16) {
        self.fold(u64::from(value));
    }

    #[inline]
    fn write_u32(&mut self, value: u32) {
        self.fold(u64::from(value));
    }

    #[inline]
    fn write_u64(&mut self, value: u64) {
        self.fold(value);
    }

    #[inline]
    fn write_u128(&mut self, value: u128) {
        self.fold(value as u64);
        self.fold((value >> 64) as u64);
    }

    #[inline]
    fn write_usize(&mut self, value: usize) {
        self.fold(value as u64);
    }

    #[inline]
    fn finish(&self) -> u64 {
        let mut hash = self.hash;
        hash ^= hash >> 30;
        hash = hash.wrapping_mul(FINALIZE_A);
        hash ^= hash >> 27;
        hash = hash.wrapping_mul(FINALIZE_B);
        hash ^ (hash >> 31)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::hash::Hash;

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
        // hashbrown indexes buckets with the low bits; a raw FxHash fold leaves
        // those nearly unmixed, so this guards the finalizer specifically.
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
}
