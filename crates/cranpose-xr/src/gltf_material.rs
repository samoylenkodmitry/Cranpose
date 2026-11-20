//! The picture that a glTF material puts on its meshes: its base color texture times its base
//! color factor.
//!
//! The scene draws a mesh with the content of a node, so the picture is that content. The factor
//! is baked into the pixels once, when the model loads, and the node draws the result as it is.

use cranpose::ImageBitmap;

use crate::gltf_model::{LayerContent, MaterialLayer};

// The side of the content of a material with no texture, in pixels. One color covers it all.
const FLAT_SIDE: u32 = 16;

// The longest side of a texture, in pixels. A texture is one offscreen layer of the scene, and a
// larger one is halved until it fits.
pub(crate) const MAX_SIDE: u32 = 2048;

// A decoded image: straight alpha, with the colors in sRGB as an image file has them.
pub(crate) struct Texture {
    pub(crate) width: u32,
    pub(crate) height: u32,
    pub(crate) rgba: Vec<u8>,
}

// How the alpha of a material counts.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) enum Alpha {
    // The surface hides what lies behind it, whatever the texture says.
    Opaque,
    // The surface shows where the alpha reaches the cutoff and is absent elsewhere.
    Mask(f32),
    // The alpha mixes the surface with what lies behind it.
    Blend,
}

impl Alpha {
    fn of(self, alpha: f32) -> f32 {
        match self {
            Self::Opaque => 1.0,
            Self::Mask(cutoff) => f32::from(alpha >= cutoff),
            Self::Blend => alpha.clamp(0.0, 1.0),
        }
    }
}

// sRGB to linear light, for a channel from 0 to 1.
fn decode(channel: f32) -> f32 {
    if channel <= 0.04045 {
        channel / 12.92
    } else {
        ((channel + 0.055) / 1.055).powf(2.4)
    }
}

// Linear light to sRGB, for a channel from 0 to 1.
fn encode(linear: f32) -> f32 {
    let linear = linear.clamp(0.0, 1.0);
    if linear <= 0.003_130_8 {
        linear * 12.92
    } else if linear == 1.0 {
        // The curve's value at one is one, which the power function misses by a rounding step
        1.0
    } else {
        1.055 * linear.powf(1.0 / 2.4) - 0.055
    }
}

// The picture of a material with base color `factor`, which is in linear light with straight alpha
// as the glTF format has it, and `texture`, if it has one.
pub(crate) fn bake(
    factor: [f32; 4],
    alpha: Alpha,
    texture: Option<Texture>,
) -> Result<MaterialLayer, String> {
    let Some(texture) = texture else {
        return Ok(MaterialLayer {
            size: [FLAT_SIDE; 2],
            content: LayerContent::Color([
                encode(factor[0]),
                encode(factor[1]),
                encode(factor[2]),
                alpha.of(factor[3]),
            ]),
        });
    };
    let mut texture = texture;
    while texture.width > MAX_SIDE || texture.height > MAX_SIDE {
        texture = halved(&texture);
    }
    tint(&mut texture, factor, alpha);
    let size = [texture.width, texture.height];
    let bitmap = ImageBitmap::from_rgba8(texture.width, texture.height, texture.rgba)
        .map_err(|error| error.to_string())?;
    Ok(MaterialLayer {
        size,
        content: LayerContent::Bitmap(bitmap),
    })
}

// Multiplies the texture by the factor, and applies the alpha mode.
fn tint(texture: &mut Texture, factor: [f32; 4], alpha: Alpha) {
    let colored = factor[..3].iter().any(|&channel| channel != 1.0);
    // The decode of each byte, worked out once
    let linear: [f32; 256] = std::array::from_fn(|byte| decode(byte as f32 / 255.0));
    for pixel in texture.rgba.as_chunks_mut::<4>().0 {
        if colored {
            for (channel, &scale) in pixel[..3].iter_mut().zip(&factor[..3]) {
                *channel = to_byte(encode(linear[usize::from(*channel)] * scale));
            }
        }
        pixel[3] = to_byte(alpha.of(f32::from(pixel[3]) / 255.0 * factor[3]));
    }
}

fn to_byte(channel: f32) -> u8 {
    (channel.clamp(0.0, 1.0) * 255.0).round() as u8
}

// The texture at half its width and height, each pixel the mean of the four it covers.
fn halved(texture: &Texture) -> Texture {
    let width = (texture.width / 2).max(1);
    let height = (texture.height / 2).max(1);
    let mut rgba = Vec::with_capacity(width as usize * height as usize * 4);
    for y in 0..height {
        for x in 0..width {
            let mut sums = [0_u32; 4];
            for (dx, dy) in [(0, 0), (1, 0), (0, 1), (1, 1)] {
                let sx = (x * 2 + dx).min(texture.width - 1);
                let sy = (y * 2 + dy).min(texture.height - 1);
                let at = (sy as usize * texture.width as usize + sx as usize) * 4;
                for (sum, &byte) in sums.iter_mut().zip(&texture.rgba[at..at + 4]) {
                    *sum += u32::from(byte);
                }
            }
            rgba.extend(sums.map(|sum| ((sum + 2) / 4) as u8));
        }
    }
    Texture {
        width,
        height,
        rgba,
    }
}

#[cfg(test)]
#[path = "tests/gltf_material_tests.rs"]
mod tests;
