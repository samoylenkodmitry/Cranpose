//! The shared benchmark data (`perf-data`), with its palettes as Cranpose colors.

use cranpose::Color;
pub use perf_data::*;

const fn colors(rgb: [[u8; 3]; 8]) -> [Color; 8] {
    let mut out = [Color::from_rgb_u8(0, 0, 0); 8];
    let mut index = 0;
    while index < 8 {
        out[index] = Color::from_rgb_u8(rgb[index][0], rgb[index][1], rgb[index][2]);
        index += 1;
    }
    out
}

/// Eight saturated colors, used for avatars, chips, particles and tiles.
pub const PALETTE: [Color; 8] = colors(PALETTE_RGB);
/// Light chip backgrounds matching [`PALETTE`].
pub const CHIP_BACKGROUND: [Color; 8] = colors(CHIP_BACKGROUND_RGB);
/// Dark ends of the media gradients.
pub const GRADIENT_END: [Color; 8] = colors(GRADIENT_END_RGB);
