use std::{ops::Range, sync::Arc};

use crate::{
    glyph_id::GlyphId,
    paragraph_glyphs::ParagraphGlyphs,
    shaped_glyphs::{BreakBefore, GlyphOffset, ShapedGlyph, ShapedGlyphs},
};

// The glyphs of a layout, with where the first draw from the pen when any
// draws off it, as `ShapedGlyphs` holds them; the copies of a layout and a
// relayout that adds no glyphs share them.
#[derive(Clone, Debug, Default, PartialEq)]
pub(crate) struct LaidGlyphs {
    glyphs: Glyphs,
    offsets: Option<Arc<[GlyphOffset]>>,
    // What each glyph's advance gains where advances round to whole device
    // pixels, as `ParagraphGlyphs` measures it.
    shifts: Option<Arc<[f32]>>,
}

// A layout's glyphs: in eight bytes each when every one fits them, as the
// glyphs of all but the widest fonts do.
#[derive(Clone, Debug, PartialEq)]
enum Glyphs {
    Packed(Arc<[PackedGlyph]>),
    Full(Arc<[ShapedGlyph]>),
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

impl PackedGlyph {
    pub(crate) fn id(self) -> GlyphId {
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
pub(crate) enum GlyphSlice<'a> {
    Packed(&'a [PackedGlyph]),
    Full(&'a [ShapedGlyph]),
}

// Some of a layout's glyphs, with what their advances gain where advances
// round to whole device pixels: none where they keep the font's fractions.
#[derive(Clone, Copy)]
pub(crate) struct RunGlyphs<'a> {
    pub(crate) glyphs: GlyphSlice<'a>,
    shifts: &'a [f32],
}

impl<'a> RunGlyphs<'a> {
    pub(crate) fn len(self) -> usize {
        match self.glyphs {
            GlyphSlice::Packed(glyphs) => glyphs.len(),
            GlyphSlice::Full(glyphs) => glyphs.len(),
        }
    }

    pub(crate) fn get(self, index: usize) -> Option<ShapedGlyph> {
        match self.glyphs {
            GlyphSlice::Packed(glyphs) => glyphs.get(index).map(|glyph| glyph.unpack()),
            GlyphSlice::Full(glyphs) => glyphs.get(index).copied(),
        }
    }

    // What the advance of glyph `index` gains.
    pub(crate) fn shift(self, index: usize) -> f32 {
        self.shifts.get(index).copied().unwrap_or(0.0)
    }

    pub(crate) fn iter(self) -> impl Iterator<Item = ShapedGlyph> + 'a {
        (0..self.len()).filter_map(move |index| self.get(index))
    }

    fn slice(self, range: Range<usize>) -> Self {
        let glyphs = match self.glyphs {
            GlyphSlice::Packed(glyphs) => GlyphSlice::Packed(&glyphs[range.clone()]),
            GlyphSlice::Full(glyphs) => GlyphSlice::Full(&glyphs[range.clone()]),
        };
        Self {
            glyphs,
            shifts: self.shifts.get(range).unwrap_or_default(),
        }
    }
}

impl LaidGlyphs {
    // Keeps the glyphs of `paragraph` with what their advances gain: those
    // held already when they are as many, as a relayout that shaped no
    // glyph takes them.
    pub(crate) fn keep(&mut self, paragraph: &ParagraphGlyphs) {
        if self.len() != paragraph.len() {
            *self = Self::new(paragraph);
        }
        let shifts = paragraph.shifts(0..paragraph.len());
        if self.shifts.as_deref().unwrap_or_default() != shifts {
            self.shifts = (!shifts.is_empty()).then(|| Arc::from(shifts));
        }
    }

    // Keeps `shaped`.
    fn new(shaped: &ShapedGlyphs) -> Self {
        let offsets = shaped.offsets();
        let mut fits = true;
        // Collected from an iterator of known length, in one allocation.
        let packed: Arc<[PackedGlyph]> = shaped
            .iter()
            .map(|glyph| {
                let (packed, fit) = PackedGlyph::pack(glyph);
                fits &= fit;
                packed
            })
            .collect();
        let glyphs = if fits {
            Glyphs::Packed(packed)
        } else {
            Glyphs::Full(Arc::from(&shaped[..]))
        };
        Self {
            glyphs,
            offsets: offsets
                .iter()
                .any(|offset| *offset != GlyphOffset::default())
                .then(|| Arc::from(offsets)),
            shifts: None,
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
        let glyphs = match &self.glyphs {
            Glyphs::Packed(glyphs) => GlyphSlice::Packed(glyphs),
            Glyphs::Full(glyphs) => GlyphSlice::Full(glyphs),
        };
        RunGlyphs {
            glyphs,
            shifts: self.shifts.as_deref().unwrap_or_default(),
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
        // Read in place, so each glyph is unpacked only to be appended.
        let taken = match self.run(range).glyphs {
            GlyphSlice::Packed(glyphs) => glyphs
                .iter()
                .take_while(|glyph| text.contains(&glyph.text_offset()))
                .count(),
            GlyphSlice::Full(glyphs) => glyphs
                .iter()
                .take_while(|glyph| text.contains(&glyph.text_offset()))
                .count(),
        };
        let end = start + taken;
        let offsets = self.offsets(start..end).unwrap_or_default();
        match self.run(start..end).glyphs {
            GlyphSlice::Packed(glyphs) => {
                out.extend(glyphs.iter().map(|glyph| glyph.unpack()), offsets);
            }
            GlyphSlice::Full(glyphs) => out.extend_from_slice(glyphs, offsets),
        }
        end
    }
}
