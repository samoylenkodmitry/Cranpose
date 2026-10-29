use std::sync::{Arc, Weak};

use crate::{paragraph_measures::ParagraphMeasures, shaper::ShapedGlyph};

// Whether the layouter holds the measures of the text it lays out, kept
// for a layout of it at another width, or measures the text anew.
#[derive(Clone, Copy)]
pub(crate) enum Measured {
    Kept,
    Anew,
}

// How a layout's lines were found: broken at the text's opportunities,
// whose measures the layouter keeps for the layout's glyphs, or as its one
// line.
#[derive(Clone, Copy)]
pub(crate) enum Broken {
    AtOpportunities,
    AsOneLine,
}

// The most texts whose measures the layouter keeps.
const KEPT: usize = 64;

// The measures of the texts whose lines wrapped last, by the glyphs of
// their layouts, so a text laid out at width after width, as it animates,
// measures once. An entry's weak handle keeps the storage of its glyphs, and so its
// address, from going to other glyphs while the entry stands; an entry
// whose glyphs no layout holds leaves when the next one comes.
#[derive(Default)]
pub(crate) struct KeptMeasures {
    entries: Vec<(Weak<[ShapedGlyph]>, Arc<ParagraphMeasures>)>,
}

impl KeptMeasures {
    // The measures kept for the layout whose glyphs are `glyphs`, which go
    // to `measures`; else the text is measured anew.
    pub(crate) fn take(
        &self,
        glyphs: Option<&Arc<[ShapedGlyph]>>,
        measures: &mut Arc<ParagraphMeasures>,
    ) -> Measured {
        match glyphs.and_then(|glyphs| self.of(glyphs)) {
            Some(kept) => {
                *measures = Arc::clone(kept);
                Measured::Kept
            }
            None => Measured::Anew,
        }
    }

    // The measures kept for the text `glyphs` were shaped from.
    fn of(&self, glyphs: &Arc<[ShapedGlyph]>) -> Option<&Arc<ParagraphMeasures>> {
        let address = Arc::as_ptr(glyphs).cast::<ShapedGlyph>();
        self.entries
            .iter()
            .find(|(kept, _)| kept.as_ptr().cast::<ShapedGlyph>() == address)
            .map(|(_, measures)| measures)
    }

    // Keeps `measures` for the layout whose glyphs are `glyphs`, in place
    // of the oldest entry when there is no room, unless it keeps them.
    pub(crate) fn keep(&mut self, glyphs: &Arc<[ShapedGlyph]>, measures: &Arc<ParagraphMeasures>) {
        if self.of(glyphs).is_some() {
            return;
        }
        self.entries.retain(|(kept, _)| kept.strong_count() > 0);
        if self.entries.len() == KEPT {
            self.entries.remove(0);
        }
        self.entries
            .push((Arc::downgrade(glyphs), Arc::clone(measures)));
    }
}
