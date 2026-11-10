// The clip-to-bounds modifier: a plain element with no value, which a
// chain keeps as an entry of no words.

use cranpose_foundation::{NodeCapabilities, PlainElement};

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ClipToBoundsElement;

impl PlainElement for ClipToBoundsElement {
    const WORDS: usize = 0;
    const CAPABILITIES: NodeCapabilities = NodeCapabilities::DRAW;

    fn to_words(&self, _words: &mut [u32]) {}

    #[inline]
    fn from_words(_words: &[u32]) -> Self {
        Self
    }
}
