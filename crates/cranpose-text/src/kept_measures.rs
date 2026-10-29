use std::sync::{Arc, Weak};

use crate::{paragraph_measures::ParagraphMeasures, shaper::ShapedGlyph};

// Whether the layouter holds the measures of the text it lays out, kept
// for a layout of it at another width, or measures the text anew, to keep
// them for its glyphs when given.
#[derive(Clone, Copy)]
pub(crate) enum Measured<'a> {
    Kept,
    Anew(Option<&'a Arc<[ShapedGlyph]>>),
}

// The most texts whose measures the layouter keeps.
const KEPT: usize = 64;

// The measures of the texts relaid out last, by the glyphs their layouts
// share, so a text laid out at width after width, as it animates, measures
// once. An entry's weak handle keeps the storage of its glyphs, and so its
// address, from going to other glyphs while the entry stands; an entry
// whose glyphs no layout holds leaves when the next one comes.
#[derive(Default)]
pub(crate) struct KeptMeasures {
    entries: Vec<(Weak<[ShapedGlyph]>, Arc<ParagraphMeasures>)>,
}

impl KeptMeasures {
    // The measures kept for the text `glyphs` were shaped from, which go
    // to `measures`; else the text is measured anew, and its measures kept
    // for those glyphs once its lines wrap.
    pub(crate) fn take<'a>(
        &self,
        glyphs: Option<&'a Arc<[ShapedGlyph]>>,
        measures: &mut Arc<ParagraphMeasures>,
    ) -> Measured<'a> {
        match glyphs.and_then(|glyphs| self.of(glyphs)) {
            Some(kept) => {
                *measures = Arc::clone(kept);
                Measured::Kept
            }
            None => Measured::Anew(glyphs),
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

    // Keeps `measures` for the text `glyphs` were shaped from, in place of
    // the oldest entry when there is no room.
    pub(crate) fn keep(&mut self, glyphs: &Arc<[ShapedGlyph]>, measures: &Arc<ParagraphMeasures>) {
        self.entries.retain(|(kept, _)| kept.strong_count() > 0);
        if self.entries.len() == KEPT {
            self.entries.remove(0);
        }
        self.entries
            .push((Arc::downgrade(glyphs), Arc::clone(measures)));
    }
}
