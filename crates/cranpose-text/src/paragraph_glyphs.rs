// The glyphs of the paragraph a layouter lays out, with each glyph's advance
// in device pixels where advances round to whole device pixels.

use std::ops::{Deref, Range};

use cranpose_ui_graphics::{ceil_to_grid, whole};
use read_fonts::types::{F48Dot16, GlyphId as FontGlyphId};

use crate::{
    layout_input::{GlyphAdvances, ParagraphLengths},
    shaped_glyphs::{ShapedGlyph, ShapedGlyphs},
    shaped_item::{cluster_spacing, spaced_width},
    text_layout::{HOLD_MARGIN, font_units},
    typeface::Typeface,
};

#[derive(Default)]
pub(crate) struct ParagraphGlyphs {
    glyphs: ShapedGlyphs,
    // Where advances round to whole device pixels, each glyph's advance in
    // device pixels: its own advance rounded to a whole pixel, with the
    // kerning from shaping added to it left fractional. Sums of them are
    // exact where no kerning adds a fraction.
    pixels: Vec<f32>,
    unit: WidthUnit,
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
        self.unit = WidthUnit::of(lengths);
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
        if let WidthUnit::Device { grid } = self.unit {
            self.round_advances(start, face, size * grid);
        }
        start..self.glyphs.len()
    }

    // Measures the device pixel advances of the glyphs from `start` on, at
    // `em` device pixels to the em: the font's own advance of each, at the
    // location its typeface draws, rounded half up as FreeType rounds a
    // hinted advance, and the rest of the advance from shaping, its kerning.
    // A font at its default location gives its advances straight from its
    // bytes, in one pass.
    #[inline(never)]
    fn round_advances(&mut self, start: usize, face: &Typeface, em: f32) {
        let Self { glyphs, pixels, .. } = self;
        // Only `add` appends glyphs, and it measures each one it adds.
        debug_assert_eq!(pixels.len(), start);
        let device = em / f32::from(face.units_per_em());
        let added = &glyphs[start..];
        if let Some((advances, bytes)) = face.h_advances() {
            let metrics = advances.metrics(bytes);
            pixels.extend(added.iter().map(|glyph| {
                let own = advances.advance(metrics, glyph.id().0);
                whole_pixels(glyph, f32::from(own), device)
            }));
            return;
        }
        pixels.resize(glyphs.len(), 0.0);
        let pixels = &mut pixels[start..];
        face.font().glyph_metrics().h_advance_batched(
            F48Dot16::to_f32,
            added
                .iter()
                .zip(pixels.iter_mut())
                .map(|(glyph, own)| (FontGlyphId::new(glyph.id().0), own)),
        );
        for (glyph, pixel) in added.iter().zip(pixels) {
            *pixel = whole_pixels(glyph, *pixel, device);
        }
    }

    // Device pixels per pixel of the layout.
    pub(crate) fn grid(&self) -> f32 {
        self.unit.grid()
    }

    // The pixels the widths of these glyphs add up in.
    pub(crate) fn width_unit(&self) -> WidthUnit {
        self.unit
    }

    // The device pixel advances of the glyphs of `range`, where advances
    // round to whole device pixels.
    pub(crate) fn pixels(&self, range: Range<usize>) -> Option<&[f32]> {
        match self.unit {
            WidthUnit::Layout { .. } => None,
            WidthUnit::Device { .. } => Some(&self.pixels[range]),
        }
    }

    // The width of the glyphs of `range` at `scale`, each of their
    // clusters `spacing` wider.
    #[inline]
    pub(crate) fn width(&self, range: Range<usize>, scale: f32, spacing: f32) -> f32 {
        match self.unit {
            WidthUnit::Layout { .. } => spaced_width(&self.glyphs[range], scale, spacing),
            WidthUnit::Device { grid } => self.device_width(range, spacing * grid),
        }
    }

    // The width of the glyphs of `range` where advances round to whole
    // device pixels, each of their clusters `spacing` device pixels wider.
    // Out of line, so the width of fractional advances stays in its callers.
    #[inline(never)]
    fn device_width(&self, range: Range<usize>, spacing: f32) -> f32 {
        let spacing = cluster_spacing(&self.glyphs[range.clone()], spacing);
        self.unit
            .layout(self.pixels[range].iter().sum::<f32>() + spacing)
    }
}

// The device pixels `glyph` advances, at `device` pixels to a font unit,
// with `own` units its own advance. HarfBuzz sets the advance of a mark to
// zero after it reads the rounded advance, so a glyph with no shaped
// advance advances by nothing.
#[inline(always)]
fn whole_pixels(glyph: &ShapedGlyph, own: f32, device: f32) -> f32 {
    if glyph.advance == 0 {
        return 0.0;
    }
    let own = own * device;
    // The difference between a float and its nearest whole number is exact,
    // so an unkerned advance lands on its whole pixel.
    font_units(glyph.advance) * device + (whole::floor(own + 0.5) - own)
}

// The pixels a paragraph's widths add up in, on a device of `grid` device
// pixels to a pixel of the layout.
#[derive(Clone, Copy, Debug)]
pub(crate) enum WidthUnit {
    // Pixels of the layout, where advances keep the font's fractions.
    Layout { grid: f32 },
    // Device pixels, where advances round to them: the widths of whole
    // pixel advances add up exactly.
    Device { grid: f32 },
}

impl Default for WidthUnit {
    fn default() -> Self {
        Self::Layout { grid: 1.0 }
    }
}

// How far, in pixels of the layout, a device pixel width may reach past a
// width of the layout and still fit it: a width on the device pixel grid
// scales to a hair off a whole pixel, and padding added and taken off moves
// it by a hair. Twice the margin a hold keeps, so a layout whose widest line
// lands on the grid holds at its own width. Kerning a font states lies far
// over this.
const WIDTH_SLACK: f32 = 2.0 * HOLD_MARGIN;

impl WidthUnit {
    // The pixels of a paragraph of `lengths`.
    fn of(lengths: &ParagraphLengths) -> Self {
        let grid = lengths.grid;
        match lengths.advances {
            GlyphAdvances::Fractional => Self::Layout { grid },
            GlyphAdvances::WholePixels => Self::Device { grid },
        }
    }

    fn grid(self) -> f32 {
        match self {
            Self::Layout { grid } | Self::Device { grid } => grid,
        }
    }

    // `width`, in these pixels.
    pub(crate) fn pixels(self, width: f32) -> f32 {
        match self {
            Self::Layout { .. } => width,
            Self::Device { grid } => width * grid,
        }
    }

    // `pixels` of these, in pixels of the layout. A device pixel width sits
    // the slack inside its layout width, so a plain comparison lets it fit
    // any width up to the slack short of it.
    #[inline]
    pub(crate) fn layout(self, pixels: f32) -> f32 {
        match self {
            Self::Layout { .. } => pixels,
            Self::Device { grid } => {
                let width = pixels / grid;
                if width > WIDTH_SLACK {
                    width - WIDTH_SLACK
                } else {
                    width
                }
            }
        }
    }

    // How wide a text may be, in these pixels, to fit `width`, a width of
    // the layout: as `layout` takes the pixels, up to the slack past it.
    pub(crate) fn limit(self, width: f32) -> f32 {
        match self {
            Self::Layout { .. } => width,
            Self::Device { grid } => (width + WIDTH_SLACK) * grid,
        }
    }

    // `width`, a width of the layout's text, rounded up to whole device
    // pixels.
    pub(crate) fn ceil(self, width: f32) -> f32 {
        match self {
            Self::Layout { grid } => whole::ceil(width * grid) / grid,
            Self::Device { grid } => ceil_to_grid(width, grid),
        }
    }
}
