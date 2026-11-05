// The glyphs of the paragraph a layouter lays out, with what each glyph's
// advance gains where advances round to whole device pixels.

use std::ops::{Deref, Range};

use cranpose_ui_graphics::whole;
use read_fonts::types::GlyphId as FontGlyphId;

use crate::{
    layout_input::{GlyphAdvances, ParagraphLengths},
    shaped_glyphs::{ShapedGlyph, ShapedGlyphs},
    shaped_item::spaced_width,
    typeface::Typeface,
};

#[derive(Default)]
pub(crate) struct ParagraphGlyphs {
    glyphs: ShapedGlyphs,
    // What each glyph's advance gains, in pixels of the layout: its own
    // advance rounded to whole device pixels less that advance, the
    // kerning shaping added to it left whole. Empty where advances keep
    // the font's fractions.
    shifts: Vec<f32>,
    advances: GlyphAdvances,
    grid: f32,
}

impl Deref for ParagraphGlyphs {
    type Target = ShapedGlyphs;

    fn deref(&self) -> &ShapedGlyphs {
        &self.glyphs
    }
}

impl ParagraphGlyphs {
    // Empties the glyphs for a paragraph of `lengths`.
    pub(crate) fn clear(&mut self, lengths: &ParagraphLengths) {
        self.glyphs.clear();
        self.shifts.clear();
        self.advances = lengths.advances;
        self.grid = lengths.grid;
    }

    pub(crate) fn reserve(&mut self, additional: usize) {
        self.glyphs.reserve(additional);
    }

    // Keeps the first `len` glyphs.
    #[cfg(feature = "hyphenation")]
    pub(crate) fn truncate(&mut self, len: usize) {
        self.glyphs.truncate(len);
        self.shifts.truncate(len);
    }

    // The glyphs, to change what shaping gave them but their advances.
    pub(crate) fn glyphs_mut(&mut self) -> &mut [ShapedGlyph] {
        self.glyphs.glyphs_mut()
    }

    // Adds the glyphs `add` appends, all of `face` at `size` pixels, and
    // returns where they lie.
    pub(crate) fn add(
        &mut self,
        (face, size): (&Typeface, f32),
        add: impl FnOnce(&mut ShapedGlyphs),
    ) -> Range<usize> {
        let start = self.glyphs.len();
        add(&mut self.glyphs);
        if self.advances == GlyphAdvances::WholePixels {
            self.shift(start, face, size);
        }
        start..self.glyphs.len()
    }

    // Measures the shifts of the glyphs from `start` on: the font's own
    // advance of each, at the location its typeface draws, rounded half
    // up to device pixels as FreeType rounds a hinted advance.
    #[inline(never)]
    fn shift(&mut self, start: usize, face: &Typeface, size: f32) {
        let Self {
            glyphs,
            shifts,
            grid,
            ..
        } = self;
        let grid = *grid;
        let scale = size / f32::from(face.units_per_em());
        let device = scale * grid;
        shifts.resize(glyphs.len(), 0.0);
        face.font().glyph_metrics().h_advance_batched(
            |advance| {
                let units = advance.to_f32();
                whole::floor(units * device + 0.5) / grid - units * scale
            },
            glyphs[start..]
                .iter()
                .zip(&mut shifts[start..])
                .map(|(glyph, shift)| (FontGlyphId::new(glyph.id().0), shift)),
        );
    }

    // What the advances of the glyphs of `range` gain, empty where advances
    // keep the font's fractions.
    pub(crate) fn shifts(&self, range: Range<usize>) -> &[f32] {
        self.shifts.get(range).unwrap_or_default()
    }

    // The width of the glyphs of `range` at `scale`, each of their
    // clusters `spacing` wider.
    pub(crate) fn width(&self, range: Range<usize>, scale: f32, spacing: f32) -> f32 {
        let shifts = self.shifts(range.clone());
        spaced_width(&self.glyphs[range], scale, spacing) + shifts.iter().sum::<f32>()
    }
}
