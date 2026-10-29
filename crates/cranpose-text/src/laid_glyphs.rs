use std::{ops::Range, sync::Arc};

use crate::shaped_glyphs::{GlyphOffset, ShapedGlyph, ShapedGlyphs};

// The glyphs of a layout, with where the first draw from the pen when any
// draws off it, as `ShapedGlyphs` holds them; the copies of a layout and a
// relayout that adds no glyphs share them.
#[derive(Clone, Debug, Default, PartialEq)]
pub(crate) struct LaidGlyphs {
    glyphs: Arc<[ShapedGlyph]>,
    offsets: Option<Arc<[GlyphOffset]>>,
}

impl LaidGlyphs {
    // Keeps `shaped`.
    pub(crate) fn new(shaped: &ShapedGlyphs) -> Self {
        let offsets = shaped.offsets();
        Self {
            glyphs: Arc::from(&shaped[..]),
            offsets: offsets
                .iter()
                .any(|offset| *offset != GlyphOffset::default())
                .then(|| Arc::from(offsets)),
        }
    }

    // Where the first glyphs of `range` draw from the pen, when any glyph
    // draws off it; the glyphs past them draw at the pen.
    pub(crate) fn offsets(&self, range: Range<usize>) -> Option<&[GlyphOffset]> {
        self.offsets.as_deref().map(|offsets| {
            let len = offsets.len();
            &offsets[range.start.min(len)..range.end.min(len)]
        })
    }

    // Appends to `out` those of the glyphs of `range`, from its start,
    // whose clusters fall in `text`, and returns where the rest start.
    pub(crate) fn take(
        &self,
        range: Range<usize>,
        text: &Range<usize>,
        out: &mut ShapedGlyphs,
    ) -> usize {
        let start = range.start;
        let end = start
            + self.glyphs[range]
                .iter()
                .take_while(|glyph| text.contains(&glyph.text_offset()))
                .count();
        out.extend_from_slice(
            &self.glyphs[start..end],
            self.offsets(start..end).unwrap_or_default(),
        );
        end
    }
}

impl std::ops::Deref for LaidGlyphs {
    type Target = [ShapedGlyph];

    fn deref(&self) -> &[ShapedGlyph] {
        &self.glyphs
    }
}
