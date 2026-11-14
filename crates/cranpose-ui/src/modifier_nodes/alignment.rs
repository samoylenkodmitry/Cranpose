// The alignment modifiers, which tell a box, a row or a column where to
// align the child: a plain element, which a chain keeps as one word.

use cranpose_foundation::{NodeCapabilities, PlainElement};
use cranpose_ui_layout::{Alignment, HorizontalAlignment, VerticalAlignment};

#[derive(Debug, Clone, PartialEq)]
pub(crate) struct AlignmentElement {
    box_alignment: Option<Alignment>,
    column_alignment: Option<HorizontalAlignment>,
    row_alignment: Option<VerticalAlignment>,
    row_baseline: bool,
}

// The bits of the element's word: each alignment takes two, 0 for none and
// its place past that, in the order box horizontal, box vertical, column,
// row; the baseline takes the bit after them.
const PLACE_BITS: u32 = 2;
const PLACE_MASK: u32 = (1 << PLACE_BITS) - 1;
const BASELINE: u32 = 1 << (4 * PLACE_BITS);

const HORIZONTAL: [HorizontalAlignment; 3] = [
    HorizontalAlignment::Start,
    HorizontalAlignment::CenterHorizontally,
    HorizontalAlignment::End,
];
const VERTICAL: [VerticalAlignment; 3] = [
    VerticalAlignment::Top,
    VerticalAlignment::CenterVertically,
    VerticalAlignment::Bottom,
];

// The two bits of `place` among `places`, 0 for `None`.
fn bits<T: PartialEq>(places: &[T; 3], place: Option<T>) -> u32 {
    place.map_or(0, |place| {
        places
            .iter()
            .zip(1..)
            .find_map(|(known, bits)| (*known == place).then_some(bits))
            .unwrap_or(0)
    })
}

// The place the bits at `field` of `word` name among `places`.
fn place<T: Copy>(places: &[T; 3], word: u32, field: u32) -> Option<T> {
    let bits = (word >> (field * PLACE_BITS)) & PLACE_MASK;
    bits.checked_sub(1)
        .and_then(|index| places.get(index as usize).copied())
}

impl AlignmentElement {
    pub(crate) fn box_alignment(alignment: Alignment) -> Self {
        Self {
            box_alignment: Some(alignment),
            column_alignment: None,
            row_alignment: None,
            row_baseline: false,
        }
    }

    pub(crate) fn column_alignment(alignment: HorizontalAlignment) -> Self {
        Self {
            box_alignment: None,
            column_alignment: Some(alignment),
            row_alignment: None,
            row_baseline: false,
        }
    }

    pub(crate) fn row_alignment(alignment: VerticalAlignment) -> Self {
        Self {
            box_alignment: None,
            column_alignment: None,
            row_alignment: Some(alignment),
            row_baseline: false,
        }
    }

    pub(crate) fn row_baseline() -> Self {
        Self {
            box_alignment: None,
            column_alignment: None,
            row_alignment: None,
            row_baseline: true,
        }
    }

    pub(crate) fn in_box(&self) -> Option<Alignment> {
        self.box_alignment
    }

    pub(crate) fn in_column(&self) -> Option<HorizontalAlignment> {
        self.column_alignment
    }

    pub(crate) fn in_row(&self) -> Option<VerticalAlignment> {
        self.row_alignment
    }

    pub(crate) fn on_row_baseline(&self) -> bool {
        self.row_baseline
    }
}

impl PlainElement for AlignmentElement {
    const WORDS: usize = 1;
    const CAPABILITIES: NodeCapabilities = NodeCapabilities::LAYOUT;

    fn to_words(&self, words: &mut [u32]) {
        let in_box = self.box_alignment;
        let fields = [
            bits(&HORIZONTAL, in_box.map(|alignment| alignment.horizontal)),
            bits(&VERTICAL, in_box.map(|alignment| alignment.vertical)),
            bits(&HORIZONTAL, self.column_alignment),
            bits(&VERTICAL, self.row_alignment),
        ];
        let places = fields
            .iter()
            .zip(0..)
            .fold(0, |word, (bits, field)| word | bits << (field * PLACE_BITS));
        let baseline = if self.row_baseline { BASELINE } else { 0 };
        words.copy_from_slice(&[places | baseline]);
    }

    #[inline]
    fn from_words(words: &[u32]) -> Self {
        let word = words.first().copied().unwrap_or(0);
        let in_box = place(&HORIZONTAL, word, 0).zip(place(&VERTICAL, word, 1));
        Self {
            box_alignment: in_box
                .map(|(horizontal, vertical)| Alignment::new(horizontal, vertical)),
            column_alignment: place(&HORIZONTAL, word, 2),
            row_alignment: place(&VERTICAL, word, 3),
            row_baseline: word & BASELINE != 0,
        }
    }
}
