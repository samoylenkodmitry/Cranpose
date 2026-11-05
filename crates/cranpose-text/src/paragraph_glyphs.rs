// The glyphs of the paragraph a layouter lays out, with each glyph's advance
// in device pixels where advances round to whole device pixels.

use std::ops::{Deref, Range};

use cranpose_ui_graphics::{ceil_to_grid, whole};
use read_fonts::types::GlyphId as FontGlyphId;

use crate::{
    layout_input::{GlyphAdvances, ParagraphLengths},
    shaped_glyphs::{ShapedGlyph, ShapedGlyphs},
    shaped_item::{cluster_spacing, spaced_width},
    text_layout::font_units,
    typeface::Typeface,
};

#[derive(Default)]
pub(crate) struct ParagraphGlyphs {
    glyphs: ShapedGlyphs,
    // Each glyph's advance in device pixels: its own advance rounded to a
    // whole pixel, with the kerning shaping added to it left fractional.
    // Sums of them are exact where no kerning adds a fraction. Empty where
    // advances keep the font's fractions.
    pixels: Vec<f32>,
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
        self.pixels.clear();
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
        self.pixels.truncate(len);
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
            self.round_advances(start, face, size);
        }
        start..self.glyphs.len()
    }

    // Measures the device pixel advances of the glyphs from `start` on: the
    // font's own advance of each, at the location its typeface draws,
    // rounded half up as FreeType rounds a hinted advance, and the rest of
    // the advance shaping gave it, its kerning.
    #[inline(never)]
    fn round_advances(&mut self, start: usize, face: &Typeface, size: f32) {
        let Self {
            glyphs,
            pixels,
            grid,
            ..
        } = self;
        let device = size * *grid / f32::from(face.units_per_em());
        pixels.resize(glyphs.len(), 0.0);
        let added = &glyphs[start..];
        face.font().glyph_metrics().h_advance_batched(
            |own| own.to_f32(),
            added
                .iter()
                .zip(&mut pixels[start..])
                .map(|(glyph, own)| (FontGlyphId::new(glyph.id().0), own)),
        );
        for (glyph, pixel) in added.iter().zip(&mut pixels[start..]) {
            let own = *pixel;
            *pixel = whole::floor(own * device + 0.5) + (font_units(glyph.advance) - own) * device;
        }
    }

    // Device pixels per pixel of the layout.
    pub(crate) fn grid(&self) -> f32 {
        self.grid
    }

    // The pixels the widths of these glyphs add up in.
    pub(crate) fn width_unit(&self) -> WidthUnit {
        match self.advances {
            GlyphAdvances::Fractional => WidthUnit::default(),
            GlyphAdvances::WholePixels => WidthUnit {
                per_pixel: self.grid,
                slack: GRID_SLACK,
            },
        }
    }

    // The device pixel advances of the glyphs of `range`, empty where
    // advances keep the font's fractions.
    pub(crate) fn pixels(&self, range: Range<usize>) -> &[f32] {
        self.pixels.get(range).unwrap_or_default()
    }

    // The width of the glyphs of `range` at `scale`, each of their
    // clusters `spacing` wider.
    pub(crate) fn width(&self, range: Range<usize>, scale: f32, spacing: f32) -> f32 {
        let glyphs = &self.glyphs[range.clone()];
        let pixels = self.pixels(range);
        if pixels.is_empty() {
            return spaced_width(glyphs, scale, spacing);
        }
        pixels.iter().sum::<f32>() / self.grid + cluster_spacing(glyphs, spacing)
    }
}

// The pixels a paragraph's widths add up in: device pixels where advances
// round to them, so the widths of whole pixel advances add up exactly, and
// pixels of the layout otherwise.
#[derive(Clone, Copy, Debug)]
pub(crate) struct WidthUnit {
    // Its pixels per pixel of the layout.
    pub(crate) per_pixel: f32,
    // How far past a layout width a width may reach and still fit it: a
    // layout width on the device pixel grid scales back to a hair off a
    // whole pixel. Nothing where the pixels are the layout's.
    slack: f32,
}

// How far a layout width on the device pixel grid scales back from a whole
// pixel, at most: far under any kerning a font states.
const GRID_SLACK: f32 = 1.0 / 1024.0;

impl Default for WidthUnit {
    fn default() -> Self {
        Self {
            per_pixel: 1.0,
            slack: 0.0,
        }
    }
}

impl WidthUnit {
    // `width`, a width of the layout, in these pixels, with the slack a
    // width may reach past it by.
    pub(crate) fn limit(self, width: f32) -> f32 {
        width * self.per_pixel + self.slack
    }

    // Whether `width`, a width of the layout's text, fits `limit`, a width
    // of the layout.
    pub(crate) fn fits(self, width: f32, limit: f32) -> bool {
        width * self.per_pixel <= self.limit(limit)
    }

    // `width`, a width of the layout's text, rounded up to whole device
    // pixels at `grid`; one within the slack past a pixel stays on it.
    pub(crate) fn ceil(self, width: f32, grid: f32) -> f32 {
        ceil_to_grid(width - self.slack / self.per_pixel, grid)
    }
}
