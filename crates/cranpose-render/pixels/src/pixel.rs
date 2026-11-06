//! The pixel formats the renderer draws into: straight-alpha RGBA bytes, and
//! the 16-bit pixels of small displays, which it draws without a second pass
//! over a frame of bytes.

use cranpose_render_common::{
    graph_scene::RenderDiagnostics,
    rgb565::{GlyphPixel, Rgb565, Rgb565Be, Rgb565Le},
    text_canvas::TextPixelRows,
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

    // `paint` over every pixel of `pixels`, which it covers whole. `table`
    // keeps what the paint left over the bytes it met before, for a format
    // that blends in floats; every call with the same table takes the same
    // paint.
    fn blend_run(
        pixels: &mut [Self],
        paint: Self::Paint,
        table: &mut Option<BlendTable>,
        diagnostics: &RenderDiagnostics,
    );

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

    // Out of line: inline, the table grew the loop of a fill's rows past
    // where the compiler inlines the arrays of each row's paints.
    #[inline(never)]
    fn blend_run(
        pixels: &mut [Self],
        (color, blend_mode): Self::Paint,
        table: &mut Option<BlendTable>,
        diagnostics: &RenderDiagnostics,
    ) {
        let table = table.get_or_insert_with(BlendTable::default);
        for pixel in pixels {
            table.blend(pixel, color, blend_mode, diagnostics);
        }
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

    // A 16-bit pixel blends its bytes in whole numbers, which cost about
    // what a look-up costs, so it keeps no table.
    fn blend_run(
        pixels: &mut [Self],
        paint: Self::Paint,
        _table: &mut Option<BlendTable>,
        diagnostics: &RenderDiagnostics,
    ) {
        for pixel in pixels {
            blend_paint_565(pixel, paint, 1.0, diagnostics);
        }
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

// What one paint leaves over each byte of each channel, kept from the first
// pixel that shows the byte: `blend_pixel` works each channel apart, so a
// byte under the paint gives one byte, whatever the other channels hold.
pub(crate) struct BlendTable {
    bytes: [[u8; 256]; 4],
    // Bit `byte % 64` of word `byte / 64` of a channel: whether `bytes`
    // holds what the paint leaves over that byte.
    known: [[u64; 4]; 4],
}

impl Default for BlendTable {
    fn default() -> Self {
        Self {
            bytes: [[0; 256]; 4],
            known: [[0; 4]; 4],
        }
    }
}

impl BlendTable {
    // `color` over `pixel` under `blend_mode`, as `blend_pixel` puts it.
    fn blend(
        &mut self,
        pixel: &mut [u8; 4],
        color: [f32; 4],
        blend_mode: BlendMode,
        diagnostics: &RenderDiagnostics,
    ) {
        let under = *pixel;
        let known = (0..4).all(|channel| {
            let byte = usize::from(under[channel]);
            self.known[channel][byte / 64] & (1 << (byte % 64)) != 0
        });
        if known {
            *pixel = [0, 1, 2, 3].map(|channel| self.bytes[channel][usize::from(under[channel])]);
            return;
        }
        blend_pixel(pixel, color, blend_mode, diagnostics);
        for channel in 0..4 {
            let byte = usize::from(under[channel]);
            self.bytes[channel][byte] = pixel[channel];
            self.known[channel][byte / 64] |= 1 << (byte % 64);
        }
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
