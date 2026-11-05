// The glyphs of the paragraph a layouter lays out, with each glyph's advance
// in device pixels where advances round to whole device pixels.

use std::ops::{Deref, Range};

use cranpose_ui_graphics::{ceil_to_grid, whole};
use read_fonts::types::GlyphId as FontGlyphId;

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
    // HarfBuzz sets the advance of a mark to zero after it reads the rounded
    // advance, so a glyph with no shaped advance advances by nothing.
    #[inline(never)]
    fn round_advances(&mut self, start: usize, face: &Typeface, em: f32) {
        let Self { glyphs, pixels, .. } = self;
        // Only `add` appends glyphs, and it measures each one it adds.
        debug_assert_eq!(pixels.len(), start);
        let device = em / f32::from(face.units_per_em());
        let metrics = face.font().glyph_metrics();
        pixels.extend(glyphs[start..].iter().map(|glyph| {
            if glyph.advance == 0 {
                return 0.0;
            }
            let own = metrics.h_advance(FontGlyphId::new(glyph.id().0)) * device;
            // The difference between a float and its nearest whole number
            // is exact, so an unkerned advance lands on its whole pixel.
            font_units(glyph.advance) * device + (whole::floor(own + 0.5) - own)
        }));
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
    pub(crate) fn width(&self, range: Range<usize>, scale: f32, spacing: f32) -> f32 {
        let glyphs = &self.glyphs[range.clone()];
        match self.unit {
            WidthUnit::Layout { .. } => spaced_width(glyphs, scale, spacing),
            WidthUnit::Device { grid } => self.unit.layout(
                self.pixels[range].iter().sum::<f32>() + cluster_spacing(glyphs, spacing * grid),
            ),
        }
    }
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
