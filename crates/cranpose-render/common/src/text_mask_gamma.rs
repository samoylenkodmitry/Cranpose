//! Skia's glyph mask gamma: the coverage correction Jetpack Compose's text
//! gets before it is blended.
//!
//! Every Cranpose target blends in encoded sRGB, where raw glyph coverage
//! thins light text on a dark background and thickens dark text on a light
//! one. Skia corrects each A8 glyph mask for the text's luminance: it
//! assumes the destination has the opposite luminance, picks the coverage
//! whose encoded blend equals the blend in linear light, and boosts contrast
//! by an amount that fades out as the text approaches white. This is
//! `SkTMaskGamma<3, 3, 3>` with Skia's default parameters (sRGB transfer,
//! contrast 0.5), which both Android's HWUI and Compose Desktop use.

use cranpose_ui_graphics::{Brush, Color};

/// The bits of luminance that pick a class; `build.rs` writes a table for
/// each class.
const LUMINANCE_BITS: u32 = 3;
const LUMINANCE_CLASSES: usize = 1 << LUMINANCE_BITS;

type CorrectionTable = [u8; 256];

/// The luminance class a glyph mask is corrected for. Text colors whose
/// luminance falls in the same eighth share a correction, so a glyph cached
/// for one of them serves them all.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct TextLuminance(u8);

impl TextLuminance {
    /// The class of text drawn in `color`; alpha does not affect it.
    pub fn of_color(color: Color) -> Self {
        let channel = |value: f32| u32::from(unit_to_u8(value));
        let luminance =
            (channel(color.0) * 54 + channel(color.1) * 183 + channel(color.2) * 19) >> 8;
        Self((luminance >> (8 - LUMINANCE_BITS)) as u8)
    }

    /// The class of text painted with `brush`. Skia corrects text a shader
    /// paints as mid grey, since no single color describes it.
    pub fn of_brush(brush: &Brush) -> Self {
        match brush {
            Brush::Solid(color) => Self::of_color(*color),
            _ => Self::of_color(Color(0.5, 0.5, 0.5, 1.0)),
        }
    }

    /// The correction for glyph masks of this class.
    pub fn correction(self) -> MaskGammaCorrection {
        MaskGammaCorrection(&CORRECTION_TABLES[usize::from(self.0)])
    }
}

/// One luminance class's coverage correction, resolved once per glyph so
/// its pixels only index a table.
#[derive(Clone, Copy)]
pub struct MaskGammaCorrection(&'static CorrectionTable);

impl MaskGammaCorrection {
    /// The 8-bit mask value to blend in place of `coverage`, a mask's
    /// coverage byte.
    pub fn apply(self, coverage: u8) -> u8 {
        self.0[usize::from(coverage)]
    }

    /// [`Self::apply`] as a unit coverage.
    pub fn apply_unit(self, coverage: u8) -> f32 {
        unit_coverage(self.apply(coverage))
    }
}

/// `value` over 255 as a float.
pub(crate) fn unit_coverage(value: u8) -> f32 {
    UNIT_COVERAGE[usize::from(value)]
}

/// `UNIT_COVERAGE[c]` is `c as f32 / 255.0`, divided at build time: a pixel
/// on a target without a float divider looks it up instead.
static UNIT_COVERAGE: [f32; 256] = {
    let mut table = [0.0; 256];
    let mut value = 0;
    while value < 256 {
        table[value] = value as f32 / 255.0;
        value += 1;
    }
    table
};

/// `value`, a unit coverage or channel, as a byte.
pub(crate) fn unit_to_u8(value: f32) -> u8 {
    // Each pixel of a new glyph mask takes one: without a float rounding
    // instruction the method is a library call.
    cranpose_ui_graphics::whole::round(value.clamp(0.0, 1.0) * 255.0) as u8
}

/// The correction table of each luminance class, which `build.rs` works
/// out: Skia's `SkTMaskGamma_build_correcting_lut` with the sRGB transfer
/// and contrast 0.5.
static CORRECTION_TABLES: [CorrectionTable; LUMINANCE_CLASSES] =
    include!(concat!(env!("OUT_DIR"), "/mask_gamma.rs"));

#[cfg(test)]
#[path = "tests/text_mask_gamma_tests.rs"]
mod tests;
