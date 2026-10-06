use std::hash::{BuildHasherDefault, Hasher};

/// Multiplier from the FxHash family: odd, so every `wrapping_mul` step is a
/// bijection on `u64` and folding a word can never lose accumulator state.
const FOLD: u64 = 0x517c_c1b7_2722_0a95;

/// Rotation applied before each fold so that a word's bits reach every lane of
/// the accumulator across successive words.
const FOLD_ROTATE: u32 = 5;

/// The bytes a write needs before [`FxHasher::write`] folds it in four lanes.
/// Each fold waits on the multiply before it, so one chain takes a multiply's
/// latency per word; four chains overlap their multiplies, and a recording's
/// columns hash in about a quarter of the time. Below this, the lanes' own
/// folds into the hash cost what they save.
const LANE_BYTES: usize = 64;

/// Starting offsets that set the lanes apart, so the same word in two lanes
/// folds differently.
const LANE_SEEDS: [u64; 3] = [
    0x9e37_79b9_7f4a_7c15,
    0xc2b2_ae3d_27d4_eb4f,
    0x1656_67b1_9e37_79f9,
];

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
        self.hash = fold(self.hash, word);
    }

    /// Folds `blocks` into four lanes, word `i` of each block into lane `i`,
    /// then the lanes into the hash in order. Every lane fold is a bijection,
    /// so a change to any one word still changes the hash.
    fn fold_blocks(&mut self, blocks: &[[u8; 32]]) {
        let mut lanes = [
            self.hash,
            self.hash ^ LANE_SEEDS[0],
            self.hash ^ LANE_SEEDS[1],
            self.hash ^ LANE_SEEDS[2],
        ];
        for block in blocks {
            for (lane, word) in lanes.iter_mut().zip(block.as_chunks::<8>().0) {
                *lane = fold(*lane, u64::from_le_bytes(*word));
            }
        }
        for lane in lanes {
            self.fold(lane);
        }
    }
}

#[inline]
fn fold(hash: u64, word: u64) -> u64 {
    (hash.rotate_left(FOLD_ROTATE) ^ word).wrapping_mul(FOLD)
}

impl Hasher for FxHasher {
    #[inline]
    fn write(&mut self, bytes: &[u8]) {
        let bytes = if bytes.len() >= LANE_BYTES {
            let (blocks, rest) = bytes.as_chunks::<32>();
            self.fold_blocks(blocks);
            rest
        } else {
            bytes
        };
        let (chunks, tail) = bytes.as_chunks::<8>();
        for chunk in chunks {
            self.fold(u64::from_le_bytes(*chunk));
        }
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
#[path = "tests/fx_hash_tests.rs"]
mod tests;
