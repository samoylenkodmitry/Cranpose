use std::{ops::Range, sync::Arc};

use crate::{
    glyph_id::GlyphId,
    paragraph_glyphs::ParagraphGlyphs,
    shaped_glyphs::{BreakBefore, GlyphOffset, ShapedGlyph, ShapedGlyphs},
    text_layout::font_units,
};

// The glyphs of a layout, with what few layouts keep beside them; the copies
// of a layout and a relayout that adds no glyphs share them.
#[derive(Clone, Debug, Default, PartialEq)]
pub(crate) struct LaidGlyphs {
    glyphs: Glyphs,
    // Behind one pointer, as most layouts keep none of it.
    extras: Option<Arc<GlyphExtras>>,
}

// What a layout keeps beside its glyphs: where the first glyphs draw from
// the pen when any draws off it, as `ShapedGlyphs` holds them, and where
// advances round to whole device pixels, each glyph's advance in pixels of
// the layout.
#[derive(Clone, Debug, Default, PartialEq)]
struct GlyphExtras {
    offsets: Box<[GlyphOffset]>,
    advances: Box<[f32]>,
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
pub(crate) enum RunGlyphs<'a> {
    Packed(&'a [PackedGlyph]),
    Full(&'a [ShapedGlyph]),
}

impl<'a> RunGlyphs<'a> {
    pub(crate) fn len(self) -> usize {
        match self {
            Self::Packed(glyphs) => glyphs.len(),
            Self::Full(glyphs) => glyphs.len(),
        }
    }

    pub(crate) fn get(self, index: usize) -> Option<ShapedGlyph> {
        match self {
            Self::Packed(glyphs) => glyphs.get(index).map(|glyph| glyph.unpack()),
            Self::Full(glyphs) => glyphs.get(index).copied(),
        }
    }

    pub(crate) fn iter(self) -> impl Iterator<Item = ShapedGlyph> + 'a {
        (0..self.len()).filter_map(move |index| self.get(index))
    }

    fn slice(self, range: Range<usize>) -> Self {
        match self {
            Self::Packed(glyphs) => Self::Packed(&glyphs[range]),
            Self::Full(glyphs) => Self::Full(&glyphs[range]),
        }
    }
}

// What the glyphs of a run keep beside them in their layout: where each
// draws from its pen, and how far it advances where advances round to whole
// device pixels.
#[derive(Clone, Copy, Default)]
pub(crate) struct RunExtras<'a> {
    extras: Option<&'a GlyphExtras>,
    // The run's first glyph among the layout's.
    start: usize,
}

impl RunExtras<'_> {
    // Where glyph `index` of the run draws from its pen.
    pub(crate) fn offset(self, index: usize) -> GlyphOffset {
        self.extras
            .and_then(|extras| extras.offsets.get(self.start + index))
            .copied()
            .unwrap_or_default()
    }

    // How far glyph `index` of the run, `glyph`, advances, in pixels of the
    // layout: its advance from the font at `scale` where advances keep the
    // font's fractions.
    pub(crate) fn advance(self, index: usize, glyph: &ShapedGlyph, scale: f32) -> f32 {
        match self
            .extras
            .and_then(|extras| extras.advances.get(self.start + index))
        {
            Some(advance) => *advance,
            None => font_units(glyph.advance) * scale,
        }
    }
}

impl LaidGlyphs {
    // Keeps the glyphs of `paragraph` with their rounded advances: those held
    // already when they are as many, as a relayout that shaped no glyph
    // takes them.
    pub(crate) fn keep(&mut self, paragraph: &ParagraphGlyphs) {
        if self.len() != paragraph.len() {
            *self = Self::new(paragraph);
        }
        let pixels = paragraph.pixels(0..paragraph.len());
        let held = self
            .extras
            .as_deref()
            .map_or(&[][..], |extras| &extras.advances[..]);
        // Most layouts keep the font's fractions, before and after.
        if pixels.is_empty() && held.is_empty() {
            return;
        }
        let grid = paragraph.grid();
        let same = held.len() == pixels.len()
            && held
                .iter()
                .zip(pixels)
                .all(|(held, pixels)| held.to_bits() == (pixels / grid).to_bits());
        if same {
            return;
        }
        let advances: Box<[f32]> = pixels.iter().map(|pixels| pixels / grid).collect();
        match &mut self.extras {
            Some(extras) => Arc::make_mut(extras).advances = advances,
            None => {
                self.extras = Some(Arc::new(GlyphExtras {
                    offsets: Box::default(),
                    advances,
                }));
            }
        }
        if self
            .extras
            .as_deref()
            .is_some_and(|extras| extras.offsets.is_empty() && extras.advances.is_empty())
        {
            self.extras = None;
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
        let drawn_off = offsets
            .iter()
            .any(|offset| *offset != GlyphOffset::default());
        Self {
            glyphs,
            extras: drawn_off.then(|| {
                Arc::new(GlyphExtras {
                    offsets: Box::from(offsets),
                    advances: Box::default(),
                })
            }),
        }
    }

    pub(crate) fn len(&self) -> usize {
        self.all().len()
    }

    // The glyphs of `range`.
    pub(crate) fn run(&self, range: Range<usize>) -> RunGlyphs<'_> {
        self.all().slice(range)
    }

    // What the glyphs from `start` keep beside them.
    pub(crate) fn extras(&self, start: usize) -> RunExtras<'_> {
        RunExtras {
            extras: self.extras.as_deref(),
            start,
        }
    }

    fn all(&self) -> RunGlyphs<'_> {
        match &self.glyphs {
            Glyphs::Packed(glyphs) => RunGlyphs::Packed(glyphs),
            Glyphs::Full(glyphs) => RunGlyphs::Full(glyphs),
        }
    }

    // Whether a glyph draws off its pen.
    pub(crate) fn has_offsets(&self) -> bool {
        self.extras
            .as_deref()
            .is_some_and(|extras| !extras.offsets.is_empty())
    }

    // Where the first glyphs of `range` draw from the pen, when any glyph
    // draws off it; the glyphs past them draw at the pen.
    pub(crate) fn offsets(&self, range: Range<usize>) -> Option<&[GlyphOffset]> {
        self.extras
            .as_deref()
            .filter(|extras| !extras.offsets.is_empty())
            .map(|extras| {
                let len = extras.offsets.len();
                &extras.offsets[range.start.min(len)..range.end.min(len)]
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
        let taken = match self.run(range) {
            RunGlyphs::Packed(glyphs) => glyphs
                .iter()
                .take_while(|glyph| text.contains(&glyph.text_offset()))
                .count(),
            RunGlyphs::Full(glyphs) => glyphs
                .iter()
                .take_while(|glyph| text.contains(&glyph.text_offset()))
                .count(),
        };
        let end = start + taken;
        let offsets = self.offsets(start..end).unwrap_or_default();
        match self.run(start..end) {
            RunGlyphs::Packed(glyphs) => {
                out.extend(glyphs.iter().map(|glyph| glyph.unpack()), offsets);
            }
            RunGlyphs::Full(glyphs) => out.extend_from_slice(glyphs, offsets),
        }
        end
    }
}
