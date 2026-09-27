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

use std::sync::OnceLock;

use cranpose_ui_graphics::{Brush, Color};

const LUMINANCE_BITS: u32 = 3;
const LUMINANCE_CLASSES: usize = 1 << LUMINANCE_BITS;
const CONTRAST: f32 = 0.5;

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
        MaskGammaCorrection(&correction_tables()[usize::from(self.0)])
    }
}

/// One luminance class's coverage correction, resolved once per glyph so
/// its pixels only index a table.
#[derive(Clone, Copy)]
pub struct MaskGammaCorrection(&'static CorrectionTable);

impl MaskGammaCorrection {
    /// The 8-bit mask value to blend in place of `coverage`.
    pub fn apply(self, coverage: f32) -> u8 {
        self.0[usize::from(unit_to_u8(coverage))]
    }

    /// [`Self::apply`] as a unit coverage.
    pub fn apply_unit(self, coverage: f32) -> f32 {
        f32::from(self.apply(coverage)) / 255.0
    }
}

fn unit_to_u8(value: f32) -> u8 {
    (value.clamp(0.0, 1.0) * 255.0).round() as u8
}

fn correction_tables() -> &'static [CorrectionTable; LUMINANCE_CLASSES] {
    static TABLES: OnceLock<[CorrectionTable; LUMINANCE_CLASSES]> = OnceLock::new();
    TABLES.get_or_init(|| std::array::from_fn(correction_table))
}

/// `SkTMaskGamma_build_correcting_lut` for the class's representative
/// luminance: the class index repeated across eight bits, as
/// `sk_t_scale255` spreads it. No class sits within 1/256 of mid grey, so
/// the source and the guessed destination always differ.
fn correction_table(class: usize) -> CorrectionTable {
    let class = class as u32;
    let representative = (class << 5) | (class << 2) | (class >> 1);
    let src = representative as f32 / 255.0;
    let dst = 1.0 - src;
    let linear_src = srgb_to_linear(src);
    let linear_dst = srgb_to_linear(dst);
    let contrast = CONTRAST * linear_dst;
    std::array::from_fn(|index| {
        let raw = index as f32 / 255.0;
        let boosted = raw + (1.0 - raw) * contrast * raw;
        let blended = linear_to_srgb(linear_src * boosted + (1.0 - boosted) * linear_dst);
        unit_to_u8((blended - dst) / (src - dst))
    })
}

fn srgb_to_linear(value: f32) -> f32 {
    if value <= 0.04045 {
        value / 12.92
    } else {
        ((value + 0.055) / 1.055).powf(2.4)
    }
}

fn linear_to_srgb(value: f32) -> f32 {
    if value <= 0.003_130_8 {
        value * 12.92
    } else {
        1.055 * value.powf(1.0 / 2.4) - 0.055
    }
}

#[cfg(test)]
#[path = "tests/text_mask_gamma_tests.rs"]
mod tests;
