// 64 words of 64 bits: 4,096 hashes remembered.
const WORDS: usize = 64;

/// A set of hashes, one bit per hash, which forgets every hash once as many
/// have gone in as it has bits: TinyLFU's doorkeeper, by which a cache
/// keeps only what comes back.
#[derive(Default)]
pub struct HashBits {
    bits: Vec<u64>,
    count: usize,
}

impl HashBits {
    /// Whether `hash` went in since the set last forgot.
    pub fn contains(&self, hash: u64) -> bool {
        let (word, mask) = bit(hash);
        self.bits.get(word).is_some_and(|bits| bits & mask != 0)
    }

    /// Puts `hash` in, forgetting every hash first when as many went in as
    /// the set has bits.
    pub fn insert(&mut self, hash: u64) {
        if self.bits.is_empty() || self.count == WORDS * 64 {
            self.bits = vec![0; WORDS];
            self.count = 0;
        }
        let (word, mask) = bit(hash);
        self.bits[word] |= mask;
        self.count += 1;
    }
}

// The word that holds `hash` and the bit in it.
fn bit(hash: u64) -> (usize, u64) {
    let bit = hash >> 52;
    (usize::try_from(bit >> 6).unwrap_or(0), 1 << (bit & 63))
}
