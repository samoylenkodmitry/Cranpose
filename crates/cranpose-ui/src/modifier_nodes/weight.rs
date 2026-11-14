// The weight modifier, which tells a row or a column its child's share of
// the room: a plain element, which a chain keeps as words.

use cranpose_foundation::{NodeCapabilities, PlainElement};

use crate::modifier::LayoutWeight;

#[derive(Debug, Clone, PartialEq)]
pub(crate) struct WeightElement {
    weight: f32,
    fill: bool,
}

impl WeightElement {
    pub(crate) fn new(weight: f32, fill: bool) -> Self {
        Self { weight, fill }
    }

    pub(crate) fn layout_weight(&self) -> LayoutWeight {
        LayoutWeight {
            weight: self.weight,
            fill: self.fill,
        }
    }
}

impl PlainElement for WeightElement {
    const WORDS: usize = 2;
    const CAPABILITIES: NodeCapabilities = NodeCapabilities::LAYOUT;

    fn to_words(&self, words: &mut [u32]) {
        words.copy_from_slice(&[self.weight.to_bits(), u32::from(self.fill)]);
    }

    #[inline]
    fn from_words(words: &[u32]) -> Self {
        let (weight, fill) = match *words {
            [weight, fill, ..] => (weight, fill),
            _ => (0, 0),
        };
        Self {
            weight: f32::from_bits(weight),
            fill: fill != 0,
        }
    }
}
