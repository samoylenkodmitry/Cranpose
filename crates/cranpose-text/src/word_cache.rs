use std::hash::{BuildHasher, Hash, Hasher};

use foldhash::fast::FixedState;

use crate::shaper::ShapedGlyph;

// Shaped words kept from one shaping to the next, in a direct-mapped table: a
// lookup is one hash and one compare, and the table's size is fixed. A word
// is keyed by its text and by the shaping face and plan it was shaped with.
#[derive(Default)]
pub(crate) struct WordCache {
    // Made on the first word kept, so a shaper that keeps none takes no room.
    slots: Vec<Option<Word>>,
}

struct Word {
    plan: u64,
    text: Box<str>,
    // Clusters count from the word's start.
    glyphs: Box<[ShapedGlyph]>,
}

const WORD_SLOTS: usize = 512;
const SLOT_MASK: u64 = 511;
const HASH_STATE: FixedState = FixedState::with_seed(0);

impl WordCache {
    // The glyphs of `text` shaped with `plan`, a hash of the shaping face and
    // plan, clusters counting from the word's start.
    pub(crate) fn get(&self, plan: u64, text: &str) -> Option<&[ShapedGlyph]> {
        let word = self.slots.get(slot(plan, text))?.as_ref()?;
        (word.plan == plan && *word.text == *text).then_some(&*word.glyphs)
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
    let mut state = HASH_STATE.build_hasher();
    plan.hash(&mut state);
    text.hash(&mut state);
    usize::try_from(state.finish() & SLOT_MASK).unwrap_or(0)
}
