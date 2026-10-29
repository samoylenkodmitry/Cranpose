use std::sync::Arc;

use crate::{shaper::ShapedGlyph, text_layout::TextLayout};

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

    // The glyphs of a layout in the same spans, by which the layouter keeps
    // the text's measures.
    pub(crate) fn measured_glyphs(self) -> Option<&'a Arc<[ShapedGlyph]>> {
        match self {
            Self::AtOtherWidths(layout) => Some(&layout.body().glyphs),
            Self::AtOtherSizes(_) => None,
        }
    }
}
