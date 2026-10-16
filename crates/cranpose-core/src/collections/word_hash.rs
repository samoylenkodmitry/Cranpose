use std::hash::Hasher;

/// The multiplier each word is folded in with: 2^32 over the golden ratio,
/// odd, so each step is a bijection of the state.
const K: u32 = 0x9e37_79b9;

/// The multiplier of [`WordHasher::finish`]'s mix, from the first half of
/// Chris Wellons' `lowbias32` finalizer.
const MIX: u32 = 0x7feb_352d;

/// A hasher that folds a key one 32-bit word at a time, with one add and one
/// 32-bit multiply a word, then mixes the high bits into the low ones, by
/// which a table picks its bucket. The framework's maps use it on 32-bit targets,
/// where foldhash's folded 64-bit multiply costs a chip without a 64-bit
/// multiplier about twenty instructions a word. It is deterministic and is
/// not suited to hashing untrusted input.
#[derive(Clone, Copy, Default)]
pub struct WordHasher {
    hash: u32,
}

impl WordHasher {
    #[inline]
    fn add(&mut self, word: u32) {
        self.hash = self.hash.wrapping_add(word).wrapping_mul(K);
    }
}

impl Hasher for WordHasher {
    #[inline]
    fn write(&mut self, bytes: &[u8]) {
        let (words, tail) = bytes.as_chunks::<4>();
        for word in words {
            self.add(u32::from_le_bytes(*word));
        }
        if !tail.is_empty() {
            let mut word = [0; 4];
            word[..tail.len()].copy_from_slice(tail);
            self.add(u32::from_le_bytes(word));
        }
    }

    #[inline]
    fn write_u8(&mut self, value: u8) {
        self.add(u32::from(value));
    }

    #[inline]
    fn write_u16(&mut self, value: u16) {
        self.add(u32::from(value));
    }

    #[inline]
    fn write_u32(&mut self, value: u32) {
        self.add(value);
    }

    #[inline]
    fn write_u64(&mut self, value: u64) {
        self.add(value as u32);
        self.add((value >> 32) as u32);
    }

    #[inline]
    fn write_u128(&mut self, value: u128) {
        self.write_u64(value as u64);
        self.write_u64((value >> 64) as u64);
    }

    #[inline]
    fn write_usize(&mut self, value: usize) {
        // One word where `usize` is one: the maps this hasher serves key most
        // entries by node ids.
        if usize::BITS == u32::BITS {
            self.add(value as u32);
        } else {
            self.write_u64(value as u64);
        }
    }

    #[inline]
    fn finish(&self) -> u64 {
        // A multiply leaves a word's low bits depending only on the key's low
        // bits: keys that share them, as aligned addresses and ids do, would
        // share buckets without the mix.
        let mut hash = self.hash;
        hash ^= hash >> 16;
        hash = hash.wrapping_mul(MIX);
        hash ^= hash >> 15;
        u64::from(hash)
    }
}

#[cfg(test)]
#[path = "tests/word_hash_tests.rs"]
mod tests;
