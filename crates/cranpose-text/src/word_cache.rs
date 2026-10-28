use std::hash::{BuildHasher, Hash, Hasher};

use foldhash::fast::FixedState;

use crate::shaper::ShapedGlyph;

// Shaped words kept from one shaping to the next, in a direct-mapped table: a
// lookup is one hash and one compare, and the table's size is fixed. A word
// is keyed by its text and by the shaping face and plan it was shaped with.
// A word is kept the second time it is shaped, as TinyLFU's doorkeeper
// admits, so words seen once, such as a number that changes every frame,
// take no room and cost no allocation.
pub(crate) struct WordCache {
    // Made on the first word kept, so a shaper that keeps none takes no room.
    slots: Vec<Option<Word>>,
    // One bit per hash of a word shaped once and not kept; cleared after as
    // many words as it has bits, so it forgets old words.
    seen: [u64; SEEN_WORDS],
    seen_count: usize,
}

impl Default for WordCache {
    fn default() -> Self {
        Self {
            slots: Vec::new(),
            seen: [0; SEEN_WORDS],
            seen_count: 0,
        }
    }
}

struct Word {
    plan: u64,
    text: Box<str>,
    // Clusters count from the word's start.
    glyphs: Box<[ShapedGlyph]>,
}

const WORD_SLOTS: usize = 512;
const SLOT_MASK: u64 = 511;
// 64 words of 64 bits: 4,096 words remembered.
const SEEN_WORDS: usize = 64;
const HASH_STATE: FixedState = FixedState::with_seed(0);

impl WordCache {
    // The glyphs of `text` shaped with `plan`, a hash of the shaping face and
    // plan, clusters counting from the word's start.
    pub(crate) fn get(&self, plan: u64, text: &str) -> Option<&[ShapedGlyph]> {
        let word = self.slots.get(slot(plan, text))?.as_ref()?;
        (word.plan == plan && *word.text == *text).then_some(&*word.glyphs)
    }

    // Whether the shapes of `text` with `plan` are worth keeping: it was
    // shaped before since the doorkeeper last forgot.
    pub(crate) fn admits(&mut self, plan: u64, text: &str) -> bool {
        let bit = hash(plan, text) >> 52;
        let (word, mask) = (bit_index(bit >> 6), 1 << (bit & 63));
        if self.seen[word] & mask != 0 {
            return true;
        }
        self.seen[word] |= mask;
        self.seen_count += 1;
        if self.seen_count == SEEN_WORDS * 64 {
            self.seen = [0; SEEN_WORDS];
            self.seen_count = 0;
        }
        false
    }

    // Keeps `glyphs`, clusters counting from the word's start, as the shapes
    // of `text` with `plan`, in place of the word in the same slot.
    pub(crate) fn put(&mut self, plan: u64, text: &str, glyphs: &[ShapedGlyph]) {
        if self.slots.is_empty() {
            self.slots.resize_with(WORD_SLOTS, || None);
        }
        let at = slot(plan, text);
        self.slots[at] = Some(Word {
            plan,
            text: text.into(),
            glyphs: glyphs.into(),
        });
    }
}

fn slot(plan: u64, text: &str) -> usize {
    bit_index(hash(plan, text) & SLOT_MASK)
}

fn hash(plan: u64, text: &str) -> u64 {
    let mut state = HASH_STATE.build_hasher();
    plan.hash(&mut state);
    text.hash(&mut state);
    state.finish()
}

fn bit_index(value: u64) -> usize {
    usize::try_from(value).unwrap_or(0)
}
