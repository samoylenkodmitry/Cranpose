use std::{ops::Range, sync::Arc};

use crate::{
    glyph_id::GlyphId,
    paragraph_glyphs::ParagraphGlyphs,
    shaped_glyphs::{BreakBefore, GlyphOffset, ShapedGlyph, ShapedGlyphs},
    text_layout::font_units,
};

// The glyphs of a layout, with where the first draw from the pen when any
// draws off it, as `ShapedGlyphs` holds them; the copies of a layout and a
// relayout that adds no glyphs share them.
#[derive(Clone, Debug, Default, PartialEq)]
pub(crate) struct LaidGlyphs {
    glyphs: Glyphs,
    offsets: Option<Arc<[GlyphOffset]>>,
}

// A layout's glyphs: in eight bytes each when every one fits them, as the
// glyphs of all but the widest fonts do, and four more for the advance
// where advances round to whole device pixels.
#[derive(Clone, Debug, PartialEq)]
enum Glyphs {
    Packed(Arc<[PackedGlyph]>),
    Rounded(Arc<[RoundedGlyph]>),
    Full(Arc<FullGlyphs>),
}

impl Default for Glyphs {
    fn default() -> Self {
        Self::Packed(Arc::default())
    }
}

// A glyph whose id fits 16 bits and whose advance fits 15.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct PackedGlyph {
    id: u16,
    // The advance above the lowest bit, which is set when a line break
    // before the glyph's cluster is unsafe.
    advance: i16,
    cluster: u32,
}

// A packed glyph with its advance rounded to whole device pixels, in pixels
// of the layout.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct RoundedGlyph {
    glyph: PackedGlyph,
    advance: f32,
}

// Glyphs that do not pack, with the advance of each in pixels of the layout
// where advances round to whole device pixels.
#[derive(Debug, PartialEq)]
struct FullGlyphs {
    glyphs: Box<[ShapedGlyph]>,
    advances: Option<Box<[f32]>>,
}

impl PackedGlyph {
    fn id(self) -> GlyphId {
        GlyphId(u32::from(self.id))
    }

    fn text_offset(self) -> usize {
        usize::try_from(self.cluster).unwrap_or(usize::MAX)
    }

    // `glyph` in eight bytes, its id and advance cut to their low bits, and
    // whether they fit them. No branch, so a layout's glyphs pack in one
    // pass.
    fn pack(glyph: &ShapedGlyph) -> (Self, bool) {
        let id = glyph.id().0;
        let advance = glyph.advance.cast_unsigned();
        let fits = (id >> 16 == 0) & (advance.wrapping_add(1 << 14) >> 15 == 0);
        let packed = Self {
            id: (id & 0xFFFF) as u16,
            advance: (((advance & 0x7FFF) as u16) << 1).cast_signed()
                | i16::from(glyph.unsafe_to_break()),
            cluster: glyph.cluster,
        };
        (packed, fits)
    }

    fn unpack(self) -> ShapedGlyph {
        let before = if self.advance & 1 == 0 {
            BreakBefore::Safe
        } else {
            BreakBefore::Unsafe
        };
        ShapedGlyph::new(
            GlyphId(u32::from(self.id)),
            self.cluster,
            i32::from(self.advance >> 1),
            before,
        )
    }
}

// Some of a layout's glyphs, as it keeps them.
#[derive(Clone, Copy)]
pub(crate) enum RunGlyphs<'a> {
    Packed(&'a [PackedGlyph]),
    Rounded(&'a [RoundedGlyph]),
    // Glyphs that do not pack, and their rounded advances.
    Full(&'a [ShapedGlyph], Option<&'a [f32]>),
}

impl<'a> RunGlyphs<'a> {
    pub(crate) fn len(self) -> usize {
        match self {
            Self::Packed(glyphs) => glyphs.len(),
            Self::Rounded(glyphs) => glyphs.len(),
            Self::Full(glyphs, _) => glyphs.len(),
        }
    }

    pub(crate) fn get(self, index: usize) -> Option<ShapedGlyph> {
        match self {
            Self::Packed(glyphs) => glyphs.get(index).map(|glyph| glyph.unpack()),
            Self::Rounded(glyphs) => glyphs.get(index).map(|glyph| glyph.glyph.unpack()),
            Self::Full(glyphs, _) => glyphs.get(index).copied(),
        }
    }

    // How far glyph `index`, `glyph`, advances, in pixels of the layout:
    // its advance from the font at `scale` where advances keep the font's
    // fractions.
    pub(crate) fn advance(self, index: usize, glyph: &ShapedGlyph, scale: f32) -> f32 {
        let own = || font_units(glyph.advance) * scale;
        match self {
            Self::Packed(_) => own(),
            Self::Rounded(glyphs) => glyphs.get(index).map_or_else(own, |glyph| glyph.advance),
            Self::Full(_, advances) => advances
                .and_then(|advances| advances.get(index))
                .map_or_else(own, |advance| *advance),
        }
    }

    pub(crate) fn iter(self) -> impl Iterator<Item = ShapedGlyph> + 'a {
        (0..self.len()).filter_map(move |index| self.get(index))
    }

    // Whether `found` holds for the id of any of the glyphs.
    pub(crate) fn any_id(self, found: impl Fn(GlyphId) -> bool) -> bool {
        match self {
            Self::Packed(glyphs) => glyphs.iter().any(|glyph| found(glyph.id())),
            Self::Rounded(glyphs) => glyphs.iter().any(|glyph| found(glyph.glyph.id())),
            Self::Full(glyphs, _) => glyphs.iter().any(|glyph| found(glyph.id())),
        }
    }

    // How many of the glyphs from the first have their clusters in `text`.
    fn count_in(self, text: &Range<usize>) -> usize {
        match self {
            Self::Packed(glyphs) => glyphs
                .iter()
                .take_while(|glyph| text.contains(&glyph.text_offset()))
                .count(),
            Self::Rounded(glyphs) => glyphs
                .iter()
                .take_while(|glyph| text.contains(&glyph.glyph.text_offset()))
                .count(),
            Self::Full(glyphs, _) => glyphs
                .iter()
                .take_while(|glyph| text.contains(&glyph.text_offset()))
                .count(),
        }
    }

    // Appends the glyphs to `out`, the first drawn at `offsets` from the
    // pen, each unpacked only to be appended.
    fn append_to(self, offsets: &[GlyphOffset], out: &mut ShapedGlyphs) {
        match self {
            Self::Packed(glyphs) => out.extend(glyphs.iter().map(|glyph| glyph.unpack()), offsets),
            Self::Rounded(glyphs) => {
                out.extend(glyphs.iter().map(|glyph| glyph.glyph.unpack()), offsets);
            }
            Self::Full(glyphs, _) => out.extend_from_slice(glyphs, offsets),
        }
    }

    fn slice(self, range: Range<usize>) -> Self {
        match self {
            Self::Packed(glyphs) => Self::Packed(&glyphs[range]),
            Self::Rounded(glyphs) => Self::Rounded(&glyphs[range]),
            Self::Full(glyphs, advances) => Self::Full(
                &glyphs[range.clone()],
                advances.map(|advances| &advances[range]),
            ),
        }
    }
}

impl LaidGlyphs {
    // Keeps the glyphs of `paragraph`. A relayout that shaped no glyph holds
    // them already, and they stay when they advance as far.
    pub(crate) fn keep(&mut self, paragraph: &ParagraphGlyphs) {
        if !self.advances_as(paragraph) {
            *self = Self::new(paragraph);
        }
    }

    // Whether the glyphs held are as many as those of `paragraph` and
    // advance as far.
    fn advances_as(&self, paragraph: &ParagraphGlyphs) -> bool {
        if self.len() != paragraph.len() {
            return false;
        }
        let pixels = paragraph.pixels(0..paragraph.len());
        let grid = paragraph.grid();
        match (&self.glyphs, pixels) {
            (Glyphs::Packed(_), None) => true,
            (Glyphs::Rounded(glyphs), Some(pixels)) => {
                same_advances(glyphs.iter().map(|glyph| glyph.advance), pixels, grid)
            }
            (Glyphs::Full(full), pixels) => match (&full.advances, pixels) {
                (None, None) => true,
                (Some(advances), Some(pixels)) => {
                    same_advances(advances.iter().copied(), pixels, grid)
                }
                (None, Some(_)) | (Some(_), None) => false,
            },
            (Glyphs::Packed(_), Some(_)) | (Glyphs::Rounded(_), None) => false,
        }
    }

    // Keeps the glyphs of `paragraph`, each array collected from an
    // iterator of known length in one allocation.
    fn new(paragraph: &ParagraphGlyphs) -> Self {
        let pixels = paragraph.pixels(0..paragraph.len());
        let grid = paragraph.grid();
        let mut fits = true;
        let mut pack = |glyph: &ShapedGlyph| {
            let (packed, fit) = PackedGlyph::pack(glyph);
            fits &= fit;
            packed
        };
        let packed = match pixels {
            None => Glyphs::Packed(paragraph.iter().map(&mut pack).collect()),
            Some(pixels) => Glyphs::Rounded(
                paragraph
                    .iter()
                    .zip(pixels)
                    .map(|(glyph, pixels)| RoundedGlyph {
                        glyph: pack(glyph),
                        advance: pixels / grid,
                    })
                    .collect(),
            ),
        };
        let glyphs = if fits {
            packed
        } else {
            Glyphs::Full(Arc::new(FullGlyphs {
                glyphs: Box::from(&paragraph[..]),
                advances: pixels.map(|pixels| pixels.iter().map(|pixels| pixels / grid).collect()),
            }))
        };
        let offsets = paragraph.offsets();
        Self {
            glyphs,
            offsets: offsets
                .iter()
                .any(|offset| *offset != GlyphOffset::default())
                .then(|| Arc::from(offsets)),
        }
    }

    pub(crate) fn len(&self) -> usize {
        self.all().len()
    }

    // The glyphs of `range`.
    pub(crate) fn run(&self, range: Range<usize>) -> RunGlyphs<'_> {
        self.all().slice(range)
    }

    fn all(&self) -> RunGlyphs<'_> {
        match &self.glyphs {
            Glyphs::Packed(glyphs) => RunGlyphs::Packed(glyphs),
            Glyphs::Rounded(glyphs) => RunGlyphs::Rounded(glyphs),
            Glyphs::Full(full) => RunGlyphs::Full(&full.glyphs, full.advances.as_deref()),
        }
    }

    // Whether a glyph draws off its pen.
    pub(crate) fn has_offsets(&self) -> bool {
        self.offsets.is_some()
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
        let end = start + self.run(range).count_in(text);
        let offsets = self.offsets(start..end).unwrap_or_default();
        self.run(start..end).append_to(offsets, out);
        end
    }
}

// Whether `held`, advances in pixels of the layout, are `pixels`, device
// pixel advances at `grid` of them to a pixel of the layout.
fn same_advances(held: impl ExactSizeIterator<Item = f32>, pixels: &[f32], grid: f32) -> bool {
    held.len() == pixels.len()
        && held
            .zip(pixels)
            .all(|(held, pixels)| held.to_bits() == (pixels / grid).to_bits())
}
