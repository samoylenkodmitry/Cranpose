//! The pixel formats the renderer draws into: straight-alpha RGBA bytes, and
//! the 16-bit pixels of small displays, which it draws without a second pass
//! over a frame of bytes.

use cranpose_render_common::{
    graph_scene::RenderDiagnostics,
    rgb565::{GlyphPixel, Rgb565, Rgb565Be, Rgb565Le},
    software_text_raster::TextPixelRows,
};
use cranpose_ui_graphics::BlendMode;

use crate::draw::{blend_pixel, unit_byte};

// How a format takes a gradient's colors.
#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum GradientColors {
    // Dithered as Skia dithers a gradient to bytes, so that a gradient lands
    // on the bytes the Compose gradient it stands in for does.
    Dithered,
    // Undithered, for a format whose steps are coarser than that dither,
    // which they would cut away.
    Plain,
}

pub(crate) trait Pixel: GlyphPixel {
    const GRADIENT: GradientColors;

    // A color worked out once to blend over many pixels of the format.
    type Paint: Copy;

    // The pixel an opaque color of `bytes` gives.
    fn opaque(bytes: [u8; 4]) -> Self;

    // `color`, straight-alpha unit values, to blend under `blend_mode`.
    fn paint(color: [f32; 4], blend_mode: BlendMode) -> Self::Paint;

    // `paint` over the pixel, its alpha scaled by `coverage`.
    fn blend_paint(&mut self, paint: Self::Paint, coverage: f32, diagnostics: &RenderDiagnostics);

    // Paints every pixel of `pixels` with `value`.
    fn fill(pixels: &mut [Self], value: Self);

    // Paints `pixels` with the four pixels of `pattern` over and over from
    // the first.
    fn fill_pattern(pixels: &mut [Self], pattern: [Self; 4]);

    // The pixels as a text paints over them.
    fn text_rows(pixels: &mut [Self]) -> TextPixelRows<'_>;
}

impl Pixel for [u8; 4] {
    const GRADIENT: GradientColors = GradientColors::Dithered;

    type Paint = ([f32; 4], BlendMode);

    fn opaque(bytes: [u8; 4]) -> Self {
        bytes
    }

    fn paint(color: [f32; 4], blend_mode: BlendMode) -> Self::Paint {
        (color, blend_mode)
    }

    fn blend_paint(
        &mut self,
        (mut color, blend_mode): Self::Paint,
        coverage: f32,
        diagnostics: &RenderDiagnostics,
    ) {
        color[3] *= coverage;
        blend_pixel(self, color, blend_mode, diagnostics);
    }

    // As whole 32-bit words where the pixels are aligned for them, a store a
    // pixel instead of four.
    fn fill(pixels: &mut [Self], value: Self) {
        match bytemuck::try_cast_slice_mut::<[u8; 4], u32>(pixels) {
            Ok(words) => fill_words(words, u32::from_ne_bytes(value)),
            Err(_) => pixels.fill(value),
        }
    }

    fn fill_pattern(pixels: &mut [Self], pattern: [Self; 4]) {
        match bytemuck::try_cast_slice_mut::<[u8; 4], u32>(pixels) {
            Ok(words) => repeat(words, pattern.map(u32::from_ne_bytes)),
            Err(_) => repeat(pixels, pattern),
        }
    }

    fn text_rows(pixels: &mut [Self]) -> TextPixelRows<'_> {
        TextPixelRows::Rgba(pixels)
    }
}

// A color to blend over 16-bit pixels, which blend as their bytes would and
// then keep their high bits.
#[derive(Clone, Copy)]
pub(crate) enum Paint565 {
    // Source over, the blend of nearly every drawing: the color's bytes,
    // which blend in whole numbers as a glyph's do, and its alpha.
    Over { bytes: [u8; 3], alpha: f32 },
    // Any other mode blends the unit values as the bytes' blend does.
    Unit([f32; 4], BlendMode),
}

fn paint_565(color: [f32; 4], blend_mode: BlendMode) -> Paint565 {
    match blend_mode {
        BlendMode::SrcOver => Paint565::Over {
            bytes: [
                unit_byte(color[0]),
                unit_byte(color[1]),
                unit_byte(color[2]),
            ],
            alpha: color[3],
        },
        _ => Paint565::Unit(color, blend_mode),
    }
}

fn blend_paint_565<P: Rgb565 + GlyphPixel>(
    pixel: &mut P,
    paint: Paint565,
    coverage: f32,
    diagnostics: &RenderDiagnostics,
) {
    match paint {
        Paint565::Over {
            bytes: [red, green, blue],
            alpha,
        } => pixel.blend_bytes([red, green, blue, unit_byte(alpha * coverage)]),
        Paint565::Unit(mut color, blend_mode) => {
            color[3] *= coverage;
            let mut rgba = pixel.to_rgba();
            blend_pixel(&mut rgba, color, blend_mode, diagnostics);
            *pixel = P::from_rgb(rgba);
        }
    }
}

// Two 16-bit pixels a 32-bit store: the first pixel alone when it starts
// between words, and the last when one is left over.
fn fill_565(pixels: &mut [u16], value: u16) {
    let (head, words, tail) = bytemuck::pod_align_to_mut::<u16, u32>(pixels);
    head.fill(value);
    fill_words(words, u32::from(value) * 0x0001_0001);
    tail.fill(value);
}

// The pattern of four pixels over and over from the first: two words a
// step, which the pattern's halves fill wherever the words start.
fn fill_pattern_565(pixels: &mut [u16], pattern: [u16; 4]) {
    let (head, words, tail) = bytemuck::pod_align_to_mut::<u16, u32>(pixels);
    repeat(head, pattern);
    let start = head.len();
    let after = start + 2 * words.len();
    let word =
        |first: usize| -> u32 { bytemuck::cast([pattern[first & 3], pattern[(first + 1) & 3]]) };
    let pair = [word(start), word(start + 2)];
    let (pairs, rest) = words.as_chunks_mut::<2>();
    for chunk in pairs {
        *chunk = pair;
    }
    // At most one word, at an even word of the run.
    rest.fill(pair[0]);
    for (offset, pixel) in tail.iter_mut().enumerate() {
        *pixel = pattern[(after + offset) & 3];
    }
}

// A 16-bit format: its pixels blend as their bytes would and keep their
// high bits, and a text paints into them in the format's byte order.
trait Pixel565: Rgb565 + GlyphPixel + bytemuck::Pod {
    fn text_rows_565(pixels: &mut [Self]) -> TextPixelRows<'_>;
}

impl Pixel565 for Rgb565Le {
    fn text_rows_565(pixels: &mut [Self]) -> TextPixelRows<'_> {
        TextPixelRows::Rgb565Le(pixels)
    }
}

impl Pixel565 for Rgb565Be {
    fn text_rows_565(pixels: &mut [Self]) -> TextPixelRows<'_> {
        TextPixelRows::Rgb565Be(pixels)
    }
}

impl<P: Pixel565> Pixel for P {
    const GRADIENT: GradientColors = GradientColors::Plain;

    type Paint = Paint565;

    fn opaque(bytes: [u8; 4]) -> Self {
        Self::from_rgb(bytes)
    }

    fn paint(color: [f32; 4], blend_mode: BlendMode) -> Self::Paint {
        paint_565(color, blend_mode)
    }

    fn blend_paint(&mut self, paint: Self::Paint, coverage: f32, diagnostics: &RenderDiagnostics) {
        blend_paint_565(self, paint, coverage, diagnostics);
    }

    fn fill(pixels: &mut [Self], value: Self) {
        fill_565(bytemuck::cast_slice_mut(pixels), bytemuck::cast(value));
    }

    fn fill_pattern(pixels: &mut [Self], pattern: [Self; 4]) {
        fill_pattern_565(
            bytemuck::cast_slice_mut(pixels),
            pattern.map(bytemuck::cast),
        );
    }

    fn text_rows(pixels: &mut [Self]) -> TextPixelRows<'_> {
        Self::text_rows_565(pixels)
    }
}

// `words.fill(word)` four words a step: a step of the plain loop stores one
// word, and its counter and branch cost more than the store.
fn fill_words(words: &mut [u32], word: u32) {
    let (quads, rest) = words.as_chunks_mut::<4>();
    for quad in quads {
        *quad = [word; 4];
    }
    rest.fill(word);
}

// Four items a step: a step of the plain loop stores one item, and its
// counter and branch cost more than the store.
fn repeat<T: Copy>(items: &mut [T], pattern: [T; 4]) {
    let (quads, rest) = items.as_chunks_mut::<4>();
    for quad in quads {
        *quad = pattern;
    }
    // At most three items: a copy of a slice calls memcpy for them.
    for (item, &value) in rest.iter_mut().zip(&pattern) {
        *item = value;
    }
}
