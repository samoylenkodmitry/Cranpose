//! Pixels of 16 bits, 5 of red, 6 of green and 5 of blue, as most small
//! displays take them, stored in either byte order.

use crate::byte_blend::{blend_byte_pixel, mix_byte};

/// A 16-bit pixel stored with its low byte first.
#[repr(transparent)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, bytemuck::Pod, bytemuck::Zeroable)]
pub struct Rgb565Le(pub u16);

/// A 16-bit pixel stored with its high byte first, as displays on an SPI bus
/// read it.
#[repr(transparent)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, bytemuck::Pod, bytemuck::Zeroable)]
pub struct Rgb565Be(pub u16);

/// The order in which a display takes the two bytes of a 16-bit pixel.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Rgb565Order {
    /// The low byte first, as a little-endian CPU stores a `u16`.
    LittleEndian,
    /// The high byte first, as most displays on an SPI bus read it.
    BigEndian,
}

/// How a 16-bit pixel stores its color.
pub trait Rgb565: Copy {
    /// The pixel of the red, green and blue of `bytes`, each cut to its high
    /// bits, as a display keeps a byte color.
    fn from_rgb(bytes: [u8; 4]) -> Self;

    /// The pixel's red, green and blue as bytes, each widened by repeating
    /// its high bits, and an opaque alpha.
    fn to_rgba(self) -> [u8; 4];
}

fn pack(bytes: [u8; 4]) -> u16 {
    (u16::from(bytes[0] >> 3) << 11) | (u16::from(bytes[1] >> 2) << 5) | u16::from(bytes[2] >> 3)
}

fn unpack(value: u16) -> [u8; 4] {
    let red = (value >> 11) as u8;
    let green = ((value >> 5) & 0x3F) as u8;
    let blue = (value & 0x1F) as u8;
    [
        (red << 3) | (red >> 2),
        (green << 2) | (green >> 4),
        (blue << 3) | (blue >> 2),
        255,
    ]
}

impl Rgb565 for Rgb565Le {
    fn from_rgb(bytes: [u8; 4]) -> Self {
        Self(pack(bytes).to_le())
    }

    fn to_rgba(self) -> [u8; 4] {
        unpack(u16::from_le(self.0))
    }
}

impl Rgb565 for Rgb565Be {
    fn from_rgb(bytes: [u8; 4]) -> Self {
        Self(pack(bytes).to_be())
    }

    fn to_rgba(self) -> [u8; 4] {
        unpack(u16::from_be(self.0))
    }
}

/// A pixel that a solid glyph paints over.
pub trait GlyphPixel: Copy {
    /// Puts `src`, straight-alpha bytes, over the pixel, source over.
    fn blend_bytes(&mut self, src: [u8; 4]);
}

impl GlyphPixel for [u8; 4] {
    fn blend_bytes(&mut self, src: [u8; 4]) {
        blend_byte_pixel(self, src);
    }
}

// A 16-bit pixel blends as its bytes would, then keeps their high bits. It
// has no alpha, so the blend leaves the alpha byte out.
#[inline]
fn blend_bytes_565<P: Rgb565>(pixel: &mut P, src: [u8; 4]) {
    match src[3] {
        0 => {}
        255 => *pixel = P::from_rgb(src),
        alpha => {
            let under = pixel.to_rgba();
            let alpha = u32::from(alpha);
            *pixel = P::from_rgb([
                mix_byte(src[0], under[0], alpha),
                mix_byte(src[1], under[1], alpha),
                mix_byte(src[2], under[2], alpha),
                255,
            ]);
        }
    }
}

impl GlyphPixel for Rgb565Le {
    #[inline]
    fn blend_bytes(&mut self, src: [u8; 4]) {
        blend_bytes_565(self, src);
    }
}

impl GlyphPixel for Rgb565Be {
    #[inline]
    fn blend_bytes(&mut self, src: [u8; 4]) {
        blend_bytes_565(self, src);
    }
}

#[cfg(test)]
#[path = "tests/rgb565_tests.rs"]
mod tests;
