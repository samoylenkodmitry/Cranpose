use std::hash::{BuildHasher, Hash, Hasher};

use foldhash::fast::FixedState;

use crate::shaper::ShapedGlyph;

// Shaped words kept from one shaping to the next. A direct-mapped table of
// slots finds a word with one hash and one compare; the words' text and
// glyphs live in two rings of fixed size, so keeping a word allocates
// nothing: a slot names where its word was written, and the word is gone
// once the ring has wrapped over it. A word is kept the second time it is
// shaped, as TinyLFU's doorkeeper admits, so words seen once, such as a
// number that changes every frame, do not push kept words out.
pub(crate) struct WordCache {
    // The slots and rings are made on the first word kept, so a shaper that
    // keeps none takes no room.
    slots: Vec<Option<Word>>,
    texts: Ring<u8>,
    glyphs: Ring<ShapedGlyph>,
    // One bit per hash of a word shaped once and not kept; cleared after as
    // many words as it has bits, so it forgets old words.
    seen: [u64; SEEN_WORDS],
    seen_count: usize,
}

struct Word {
    plan: u64,
    text: Written,
    // Clusters count from the word's start.
    glyphs: Written,
}

// Where a value was written in a ring: its position counted from the ring's
// first write ever, and its length.
#[derive(Clone, Copy)]
struct Written {
    at: u64,
    len: usize,
}

// A ring of values written in contiguous stretches.
struct Ring<T> {
    values: Vec<T>,
    // How many values were ever written, the gaps left at the end of a lap
    // included.
    written: u64,
}

const WORD_SLOTS: usize = 512;
const SLOT_MASK: u64 = 511;
// 64 words of 64 bits: 4,096 words remembered.
const SEEN_WORDS: usize = 64;
const TEXT_RING: usize = 8 * 1024;
const GLYPH_RING: usize = 2048;
const HASH_STATE: FixedState = FixedState::with_seed(0);

impl Default for WordCache {
    fn default() -> Self {
        Self {
            slots: Vec::new(),
            texts: Ring::default(),
            glyphs: Ring::default(),
            seen: [0; SEEN_WORDS],
            seen_count: 0,
        }
    }
}

impl WordCache {
    // The glyphs of `text` shaped with `plan`, a hash of the shaping face and
    // plan, clusters counting from the word's start.
    pub(crate) fn get(&self, plan: u64, text: &str) -> Option<&[ShapedGlyph]> {
        let word = self.slots.get(slot(plan, text))?.as_ref()?;
        let kept = self.texts.read(word.text, TEXT_RING)?;
        if word.plan != plan || kept != text.as_bytes() {
            return None;
        }
        self.glyphs.read(word.glyphs, GLYPH_RING)
    }

    // Whether the shapes of `text` with `plan` are worth keeping: it was
    // shaped before since the doorkeeper last forgot.
    pub(crate) fn admits(&mut self, plan: u64, text: &str) -> bool {
        let bit = hash(plan, text) >> 52;
        let (word, mask) = (index(bit >> 6), 1 << (bit & 63));
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
    // of `text` with `plan`, in place of the word in the same slot. A word
    // longer than a ring is not kept.
    pub(crate) fn put(&mut self, plan: u64, text: &str, glyphs: &[ShapedGlyph]) {
        if text.len() > TEXT_RING || glyphs.len() > GLYPH_RING {
            return;
        }
        if self.slots.is_empty() {
            self.slots.resize_with(WORD_SLOTS, || None);
        }
        let at = slot(plan, text);
        self.slots[at] = Some(Word {
            plan,
            text: self.texts.write(text.as_bytes(), TEXT_RING),
            glyphs: self.glyphs.write(glyphs, GLYPH_RING),
        });
    }
}

impl<T> Default for Ring<T> {
    fn default() -> Self {
        Self {
            values: Vec::new(),
            written: 0,
        }
    }
}

impl<T: Copy + Default> Ring<T> {
    // Writes `values` in one stretch of a ring of `size`, starting the next
    // lap when they do not fit before its end.
    fn write(&mut self, values: &[T], size: usize) -> Written {
        if self.values.is_empty() {
            self.values.resize(size, T::default());
        }
        let ring = to_u64(size);
        let mut start = index(self.written % ring);
        if start + values.len() > size {
            self.written += to_u64(size - start);
            start = 0;
        }
        self.values[start..start + values.len()].copy_from_slice(values);
        let at = self.written;
        self.written += to_u64(values.len());
        Written {
            at,
            len: values.len(),
        }
    }

    // What was written at `written`, unless the ring has wrapped over it.
    fn read(&self, written: Written, size: usize) -> Option<&[T]> {
        if self.written > written.at + to_u64(size) {
            return None;
        }
        let start = index(written.at % to_u64(size));
        self.values.get(start..start + written.len)
    }
}

fn slot(plan: u64, text: &str) -> usize {
    index(hash(plan, text) & SLOT_MASK)
}

fn hash(plan: u64, text: &str) -> u64 {
    let mut state = HASH_STATE.build_hasher();
    plan.hash(&mut state);
    text.hash(&mut state);
    state.finish()
}

fn index(value: u64) -> usize {
    usize::try_from(value).unwrap_or(0)
}

fn to_u64(value: usize) -> u64 {
    u64::try_from(value).unwrap_or(u64::MAX)
}
