//! Writes the glyph mask gamma tables of `src/text_mask_gamma.rs` to
//! `OUT_DIR/mask_gamma.rs`: Skia's `SkTMaskGamma_build_correcting_lut` for
//! each of the eight luminance classes, with the sRGB transfer and contrast
//! 0.5. Each table takes 256 `powf` calls, which cost a chip without a float
//! divider milliseconds at the first text a class draws.

#![allow(
    clippy::disallowed_methods,
    reason = "a build script reads cargo's environment"
)]

use std::{env, fmt::Write as _, fs, io, path::Path};

const LUMINANCE_BITS: u32 = 3;
const CONTRAST: f32 = 0.5;

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

/// The correction of luminance class `class`, whose representative is the
/// class index repeated across eight bits, as `sk_t_scale255` spreads it.
/// No class sits within 1/256 of mid grey, so the source and the guessed
/// destination always differ.
fn correction_table(class: u32) -> [u8; 256] {
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
        (((blended - dst) / (src - dst)).clamp(0.0, 1.0) * 255.0).round() as u8
    })
}

fn main() -> io::Result<()> {
    println!("cargo::rerun-if-changed=build.rs");
    let mut tables = String::from("[");
    for class in 0..1 << LUMINANCE_BITS {
        write!(tables, "{:?},", correction_table(class)).map_err(io::Error::other)?;
    }
    tables.push(']');
    let out = env::var_os("OUT_DIR").ok_or_else(|| io::Error::other("OUT_DIR unset"))?;
    fs::write(Path::new(&out).join("mask_gamma.rs"), tables)
}
