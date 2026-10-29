use std::hash::{BuildHasher, Hash, Hasher};

use cranpose_ui_graphics::MemoryHeadroom;
use foldhash::fast::FixedState;

use crate::shaper::ShapedGlyph;

// Shaped words kept from one shaping to the next. A direct-mapped table of
// slots finds a word with one hash and one compare; the words' text and
// glyphs live in two rings, so keeping a word allocates nothing: a slot
// names where its word was written, and the word is gone once the ring has
// wrapped over it. A word is kept the second time it is shaped, as
// TinyLFU's doorkeeper admits, so words seen once, such as a number that
// changes every frame, do not push kept words out.
//
// The cache starts with room for the fewest words and doubles when it
// misses a word it kept and lost for room, which a set of the words pushed
// out of their slots tells, as ARC's ghost entries do, up to the most words
// while memory is ample: a screen of a few labels keeps a few kilobytes, a
// page of prose the whole cache. Tight memory takes it back to the fewest.
pub(crate) struct WordCache {
    // The slots and rings are made on the first word kept, so a shaper that
    // keeps none takes no room.
    slots: Vec<Option<Word>>,
    texts: Ring<u8>,
    glyphs: Ring<ShapedGlyph>,
    // The words shaped once and not kept.
    seen: HashBits,
    // The words kept and then pushed out of their slots by others.
    gone: HashBits,
    // The words there is room for: a power of two from `FEWEST_WORDS` to
    // `MOST_WORDS`.
    room: usize,
    // Whether a word kept was missed for room since the room last changed.
    outgrown: bool,
    headroom: MemoryHeadroom,
}

struct Word {
    // The hash of the plan and text, which tells the word the rings wrapped
    // over from another word in its slot.
    hash: u64,
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

const FEWEST_WORDS: usize = 64;
const MOST_WORDS: usize = 512;
// The text bytes and glyphs the rings hold for a word.
const TEXT_PER_WORD: usize = 16;
const GLYPHS_PER_WORD: usize = 4;
// 64 words of 64 bits: 4,096 words remembered.
const HASH_BIT_WORDS: usize = 64;

// A set of word hashes, one bit per hash, which forgets every hash after as
// many have gone in as it has bits.
#[derive(Default)]
struct HashBits {
    bits: Vec<u64>,
    count: usize,
}
const HASH_STATE: FixedState = FixedState::with_seed(0);

impl Default for WordCache {
    fn default() -> Self {
        Self {
            slots: Vec::new(),
            texts: Ring::default(),
            glyphs: Ring::default(),
            seen: HashBits::default(),
            gone: HashBits::default(),
            room: FEWEST_WORDS,
            outgrown: false,
            headroom: MemoryHeadroom::Ample,
        }
    }
}

impl WordCache {
    // The glyphs of `text` shaped with `plan`, a hash of the shaping face and
    // plan, clusters counting from the word's start.
    pub(crate) fn get(&mut self, plan: u64, text: &str) -> Option<&[ShapedGlyph]> {
        let hash = hash(plan, text);
        let Some(word) = self
            .slots
            .get(slot(hash, self.room))
            .and_then(Option::as_ref)
            .filter(|word| word.hash == hash)
        else {
            self.outgrown |= self.gone.contains(hash);
            return None;
        };
        let Some(kept) = self.texts.read(word.text, self.room * TEXT_PER_WORD) else {
            self.outgrown = true;
            return None;
        };
        if word.plan != plan || kept != text.as_bytes() {
            return None;
        }
        let glyphs = self.glyphs.read(word.glyphs, self.room * GLYPHS_PER_WORD);
        self.outgrown |= glyphs.is_none();
        glyphs
    }

    // Takes the room back to the fewest words when memory turns tight.
    pub(crate) fn set_headroom(&mut self, headroom: MemoryHeadroom) {
        if headroom == self.headroom {
            return;
        }
        self.headroom = headroom;
        if headroom == MemoryHeadroom::Tight && self.room > FEWEST_WORDS {
            self.resize(FEWEST_WORDS);
        }
    }

    // Drops every word kept and makes room for `room` words when the next
    // is kept.
    fn resize(&mut self, room: usize) {
        self.room = room;
        self.outgrown = false;
        self.gone = HashBits::default();
        self.slots = Vec::new();
        self.texts = Ring::default();
        self.glyphs = Ring::default();
    }

    // Whether the shapes of `text` with `plan` are worth keeping: it was
    // shaped before since the doorkeeper last forgot.
    pub(crate) fn admits(&mut self, plan: u64, text: &str) -> bool {
        let hash = hash(plan, text);
        if self.seen.contains(hash) {
            return true;
        }
        self.seen.insert(hash);
        false
    }

    // Keeps `glyphs`, clusters counting from the word's start, as the shapes
    // of `text` with `plan`, in place of the word in the same slot. A word
    // longer than a ring is not kept.
    pub(crate) fn put(&mut self, plan: u64, text: &str, glyphs: &[ShapedGlyph]) {
        if self.outgrown && self.headroom == MemoryHeadroom::Ample && self.room < MOST_WORDS {
            self.resize(self.room * 2);
        }
        let (text_ring, glyph_ring) = (self.room * TEXT_PER_WORD, self.room * GLYPHS_PER_WORD);
        if text.len() > text_ring || glyphs.len() > glyph_ring {
            return;
        }
        if self.slots.is_empty() {
            self.slots.resize_with(self.room, || None);
        }
        let hash = hash(plan, text);
        let at = slot(hash, self.room);
        if let Some(pushed) = &self.slots[at]
            && pushed.hash != hash
        {
            self.gone.insert(pushed.hash);
        }
        self.slots[at] = Some(Word {
            hash,
            plan,
            text: self.texts.write(text.as_bytes(), text_ring),
            glyphs: self.glyphs.write(glyphs, glyph_ring),
        });
    }
}

impl HashBits {
    fn contains(&self, hash: u64) -> bool {
        let (word, mask) = bit(hash);
        self.bits.get(word).is_some_and(|bits| bits & mask != 0)
    }

    fn insert(&mut self, hash: u64) {
        if self.bits.is_empty() || self.count == HASH_BIT_WORDS * 64 {
            self.bits = vec![0; HASH_BIT_WORDS];
            self.count = 0;
        }
        let (word, mask) = bit(hash);
        self.bits[word] |= mask;
        self.count += 1;
    }
}

// The word of a `HashBits` that holds `hash` and the bit in it.
fn bit(hash: u64) -> (usize, u64) {
    let bit = hash >> 52;
    (index(bit >> 6), 1 << (bit & 63))
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

// The slot of the word of `hash` in a table of `room` slots, a power of two.
fn slot(hash: u64, room: usize) -> usize {
    index(hash & (to_u64(room) - 1))
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
