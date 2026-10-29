use std::sync::Arc;

use crate::{paragraph_measures::ParagraphMeasures, shaper::ShapedGlyph, text_layout::TextLayout};

/// A layout of the same text in the same typefaces whose glyphs a relayout
/// takes in place of shaping the text again.
#[derive(Clone, Copy, Debug)]
pub enum Shaped<'a> {
    /// A layout in the same spans at other widths, whose cluster widths and
    /// break opportunities the relayout takes too.
    AtOtherWidths(&'a TextLayout),
    /// A layout in spans of other font sizes.
    AtOtherSizes(&'a TextLayout),
}

impl<'a> Shaped<'a> {
    pub(crate) fn layout(self) -> &'a TextLayout {
        match self {
            Self::AtOtherWidths(layout) | Self::AtOtherSizes(layout) => layout,
        }
    }

    // The glyphs of the layout when shaping placed them all, which a
    // relayout that adds none keeps.
    pub(crate) fn glyphs(self) -> Option<&'a Arc<[ShapedGlyph]>> {
        let body = self.layout().body();
        (body.glyphs.len() == body.shaped).then_some(&body.glyphs)
    }

    // The measures of a layout in the same spans, which it kept when it
    // wrapped.
    pub(crate) fn measures(self) -> Option<&'a Arc<ParagraphMeasures>> {
        match self {
            Self::AtOtherWidths(layout) => layout.body().measures.as_ref().map(|kept| &kept.0),
            Self::AtOtherSizes(_) => None,
        }
    }
}
