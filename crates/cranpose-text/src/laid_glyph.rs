use std::{ops::Range, sync::Arc};

use crate::{glyph_id::GlyphId, shaper::ShapedGlyph};

// The top bit of a kept glyph's id: whether a line break before its
// cluster changes the shapes on either side of it. No font format numbers
// glyphs past 24 bits.
const UNSAFE_TO_BREAK: u32 = 1 << 31;

// A glyph as a layout keeps it, in half the room of a shaped glyph: the
// offsets, which most text has none of, stand apart in `LaidGlyphs`.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub(crate) struct LaidGlyph {
    id: u32,
    pub(crate) cluster: u32,
    pub(crate) advance: i32,
}

// How far right of the pen and up from the baseline a glyph draws.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub(crate) struct GlyphOffset {
    pub(crate) x: i32,
    pub(crate) y: i32,
}

// The glyphs of a layout, with the offset of each when any has one; the
// copies of a layout and a relayout that adds no glyphs share them.
#[derive(Clone, Debug, Default, PartialEq)]
pub(crate) struct LaidGlyphs {
    glyphs: Arc<[LaidGlyph]>,
    offsets: Option<Arc<[GlyphOffset]>>,
}

impl LaidGlyph {
    pub(crate) fn id(self) -> GlyphId {
        GlyphId(self.id & !UNSAFE_TO_BREAK)
    }

    // The byte offset in the text of the glyph's cluster.
    pub(crate) fn text_offset(self) -> usize {
        usize::try_from(self.cluster).unwrap_or(usize::MAX)
    }

    // The shaped glyph this one keeps, drawn at `offset`.
    fn shaped(self, offset: GlyphOffset) -> ShapedGlyph {
        ShapedGlyph {
            id: self.id(),
            cluster: self.cluster,
            advance: self.advance,
            x_offset: offset.x,
            y_offset: offset.y,
            unsafe_to_break: self.id & UNSAFE_TO_BREAK != 0,
        }
    }
}

impl From<&ShapedGlyph> for LaidGlyph {
    fn from(glyph: &ShapedGlyph) -> Self {
        let unsafe_to_break = if glyph.unsafe_to_break {
            UNSAFE_TO_BREAK
        } else {
            0
        };
        Self {
            id: glyph.id.0 | unsafe_to_break,
            cluster: glyph.cluster,
            advance: glyph.advance,
        }
    }
}

impl LaidGlyphs {
    // Keeps `glyphs`.
    pub(crate) fn new(glyphs: &[ShapedGlyph]) -> Self {
        let offsets = glyphs
            .iter()
            .any(|glyph| glyph.x_offset != 0 || glyph.y_offset != 0)
            .then(|| {
                glyphs
                    .iter()
                    .map(|glyph| GlyphOffset {
                        x: glyph.x_offset,
                        y: glyph.y_offset,
                    })
                    .collect()
            });
        Self {
            glyphs: glyphs.iter().map(LaidGlyph::from).collect(),
            offsets,
        }
    }

    // The offsets of the glyphs of `range`, when any glyph has one.
    pub(crate) fn offsets(&self, range: Range<usize>) -> Option<&[GlyphOffset]> {
        self.offsets.as_deref().map(|offsets| &offsets[range])
    }

    // Appends to `out` as shaped glyphs those of `range`, from its start,
    // whose clusters fall in `text`, and returns where the rest start.
    pub(crate) fn take(
        &self,
        range: Range<usize>,
        text: &Range<usize>,
        out: &mut Vec<ShapedGlyph>,
    ) -> usize {
        let start = range.start;
        let count = self.glyphs[range]
            .iter()
            .take_while(|glyph| text.contains(&glyph.text_offset()))
            .count();
        let end = start + count;
        let glyphs = self.glyphs[start..end].iter();
        match self.offsets(start..end) {
            Some(offsets) => out.extend(
                glyphs
                    .zip(offsets)
                    .map(|(glyph, &offset)| glyph.shaped(offset)),
            ),
            None => out.extend(glyphs.map(|glyph| glyph.shaped(GlyphOffset::default()))),
        }
        end
    }
}

impl std::ops::Deref for LaidGlyphs {
    type Target = [LaidGlyph];

    fn deref(&self) -> &[LaidGlyph] {
        &self.glyphs
    }
}
