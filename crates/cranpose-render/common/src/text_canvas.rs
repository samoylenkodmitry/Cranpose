//! The pixels a text's glyph masks are painted into: an image of the text's
//! own, or a renderer's pixels, and how a glyph's coverage blends there.

use cranpose_ui::{Brush, Color, text::Shadow};
use cranpose_ui_graphics::{BrushSampler, ImageBitmap, Point, Rect, whole};

use crate::{
    brush_sampling::{color_to_rgba, sample_brush_rgba},
    glyph_mask::GlyphMask,
    rgb565::{GlyphPixel, Rgb565Be, Rgb565Le},
    text_mask_gamma::{MaskGammaCorrection, TextLuminance},
    text_shadow::draw_shadow_mask,
};

/// Straight-alpha RGBA8 pixels, row by row, that a text was rasterized
/// into: what an image of it holds, without the content hash a bitmap
/// computes over every pixel.
pub struct TextPixels {
    /// Width in pixels.
    pub width: u32,
    /// Height in pixels.
    pub height: u32,
    /// Four bytes a pixel, `width` pixels a row.
    pub pixels: Vec<u8>,
}

impl TextPixels {
    /// The pixels as a bitmap, hashed for caches keyed by content.
    pub fn into_image(self) -> Option<ImageBitmap> {
        ImageBitmap::from_rgba8(self.width, self.height, self.pixels).ok()
    }
}

/// Pixels that a text paints over, as a renderer's target holds them:
/// `width` by `height` of them from the first, a row every `stride` pixels.
pub struct TextPixelWindow<'a> {
    /// The pixels from the window's top left corner on.
    pub pixels: TextPixelRows<'a>,
    /// Pixels from the start of one row to the start of the next.
    pub stride: usize,
    /// Width of the window in pixels.
    pub width: u32,
    /// Height of the window in pixels.
    pub height: u32,
}

/// The pixels of a [`TextPixelWindow`], in the format the renderer draws.
pub enum TextPixelRows<'a> {
    /// Straight-alpha RGBA bytes.
    Rgba(&'a mut [[u8; 4]]),
    /// 16-bit pixels stored low byte first.
    Rgb565Le(&'a mut [Rgb565Le]),
    /// 16-bit pixels stored high byte first.
    Rgb565Be(&'a mut [Rgb565Be]),
}

/// A glyph's mask placed on a canvas, its top left corner at `x`, `y`.
#[derive(Clone, Copy)]
pub(crate) struct PlacedMask<'a> {
    pub(crate) mask: &'a GlyphMask,
    pub(crate) x: i32,
    pub(crate) y: i32,
}

/// What a glyph's pixels paint with: its brush, sampled across
/// `brush_rect`, an alpha the brush's is multiplied by and the shadow under
/// it, on a canvas whose top left corner sits at `canvas_origin` in device
/// pixels, `scale` of them a layout pixel.
pub(crate) struct CanvasPaint<'a> {
    pub(crate) brush: &'a Brush,
    pub(crate) alpha: f32,
    // `brush` at `alpha` as a solid glyph's pixels take it.
    pub(crate) solid: Option<SolidInk>,
    pub(crate) shadow: Option<Shadow>,
    pub(crate) brush_rect: Rect,
    pub(crate) canvas_origin: Point,
    pub(crate) scale: f32,
    pub(crate) static_motion: bool,
}

// A solid paint as a glyph's pixels take it, worked out once for all the
// glyphs it paints.
#[derive(Clone, Copy)]
pub(crate) struct SolidInk {
    // The color's channels, each between 0 and 1.
    color: [f32; 4],
    red: u8,
    green: u8,
    blue: u8,
    // What a mask's coverage is multiplied by: the color's alpha times the
    // paint's.
    alpha_scale: f32,
    correction: MaskGammaCorrection,
}

impl SolidInk {
    // `brush` at `alpha`; `None` for a brush that is no solid color or that
    // paints nothing.
    pub(crate) fn of(brush: &Brush, alpha: f32) -> Option<Self> {
        let Brush::Solid(color) = brush else {
            return None;
        };
        let color = color_to_rgba(*color);
        let alpha_scale = color[3].clamp(0.0, 1.0) * alpha.clamp(0.0, 1.0);
        if alpha_scale <= 0.0 {
            return None;
        }
        let byte = |channel: f32| whole::round(channel.clamp(0.0, 1.0) * 255.0) as u8;
        Some(Self {
            color,
            red: byte(color[0]),
            green: byte(color[1]),
            blue: byte(color[2]),
            alpha_scale,
            correction: TextLuminance::of_color(Color(color[0], color[1], color[2], 1.0))
                .correction(),
        })
    }
}

enum CanvasPixels<'a> {
    Solid(Vec<u8>),
    Blended(Vec<[f32; 4]>),
    /// A renderer's pixels, which solid glyphs blend over as its own
    /// drawing does.
    Window(TextPixelWindow<'a>),
}

/// Pixels a text's glyphs are painted into.
pub(crate) struct TextCanvas<'a> {
    pub(crate) width: u32,
    pub(crate) height: u32,
    pixels: CanvasPixels<'a>,
}

impl TextCanvas<'static> {
    // An image of its own, `width` by `height`, of bytes, for a text whose
    // glyphs all paint solid colors.
    pub(crate) fn solid(width: u32, height: u32) -> Self {
        Self {
            width,
            height,
            pixels: CanvasPixels::Solid(vec![0; width as usize * height as usize * 4]),
        }
    }

    // An image of its own, `width` by `height`, of floats, for a text where a
    // gradient or a shadow blends.
    pub(crate) fn blended(width: u32, height: u32) -> Self {
        Self {
            width,
            height,
            pixels: CanvasPixels::Blended(vec![[0.0; 4]; width as usize * height as usize]),
        }
    }
}

impl<'a> TextCanvas<'a> {
    /// A canvas over `window`, for a text whose paint is solid.
    pub(crate) fn over(window: TextPixelWindow<'a>) -> Self {
        Self {
            width: window.width,
            height: window.height,
            pixels: CanvasPixels::Window(window),
        }
    }

    pub(crate) fn paint(&mut self, placed: PlacedMask<'_>, paint: &CanvasPaint<'_>) {
        let area = (self.width, self.height);
        match &mut self.pixels {
            CanvasPixels::Solid(pixels) => {
                if let Some(ink) = &paint.solid {
                    draw_mask_glyph_solid(
                        pixels.as_chunks_mut::<4>().0,
                        self.width as usize,
                        area,
                        placed,
                        ink,
                        CanvasBlend { color: ink.color },
                    );
                }
            }
            CanvasPixels::Window(window) => {
                if let Some(ink) = &paint.solid {
                    let stride = window.stride;
                    match &mut window.pixels {
                        TextPixelRows::Rgba(pixels) => {
                            draw_mask_glyph_solid(pixels, stride, area, placed, ink, OverBlend);
                        }
                        TextPixelRows::Rgb565Le(pixels) => {
                            draw_mask_glyph_solid(pixels, stride, area, placed, ink, OverBlend);
                        }
                        TextPixelRows::Rgb565Be(pixels) => {
                            draw_mask_glyph_solid(pixels, stride, area, placed, ink, OverBlend);
                        }
                    }
                }
            }
            CanvasPixels::Blended(pixels) => {
                if let Some(shadow) = paint.shadow {
                    draw_shadow_mask(
                        pixels,
                        area,
                        placed,
                        shadow,
                        (paint.scale, paint.static_motion),
                    );
                }
                draw_mask_glyph(pixels, area, placed, paint);
            }
        }
    }

    pub(crate) fn into_pixels(self) -> Option<TextPixels> {
        if self.width == 0 || self.height == 0 {
            return None;
        }
        let pixels = match self.pixels {
            CanvasPixels::Solid(pixels) => pixels,
            CanvasPixels::Window(_) => return None,
            CanvasPixels::Blended(canvas) => {
                let mut pixels = Vec::with_capacity(canvas.len() * 4);
                for pixel in canvas {
                    pixels.extend(
                        pixel.map(|channel| whole::round(channel.clamp(0.0, 1.0) * 255.0) as u8),
                    );
                }
                pixels
            }
        };
        Some(TextPixels {
            width: self.width,
            height: self.height,
            pixels,
        })
    }
}

pub(crate) fn blend_src_over(dst: &mut [f32; 4], src: [f32; 4]) {
    let src_alpha = src[3].clamp(0.0, 1.0);
    if src_alpha <= 0.0 {
        return;
    }

    let dst_alpha = dst[3].clamp(0.0, 1.0);
    let out_alpha = src_alpha + dst_alpha * (1.0 - src_alpha);

    if out_alpha <= f32::EPSILON {
        *dst = [0.0, 0.0, 0.0, 0.0];
        return;
    }

    for channel in 0..3 {
        let src_premult = src[channel].clamp(0.0, 1.0) * src_alpha;
        let dst_premult = dst[channel].clamp(0.0, 1.0) * dst_alpha;
        dst[channel] =
            ((src_premult + dst_premult * (1.0 - src_alpha)) / out_alpha).clamp(0.0, 1.0);
    }
    dst[3] = out_alpha;
}

// Out of line, as is a glyph's shadow: most text paints solid colors.
#[inline(never)]
fn draw_mask_glyph(
    canvas: &mut [[f32; 4]],
    (width, height): (u32, u32),
    placed: PlacedMask<'_>,
    paint: &CanvasPaint<'_>,
) {
    let mask = placed.mask;
    let correction = TextLuminance::of_brush(paint.brush).correction();
    let brush = BrushSampler::new(paint.brush, paint.brush_rect);
    for y in 0..mask.height {
        let py = placed.y + y as i32;
        if py < 0 || py >= height as i32 {
            continue;
        }

        for x in 0..mask.width {
            let px = placed.x + x as i32;
            if px < 0 || px >= width as i32 {
                continue;
            }

            let coverage = correction.apply_unit(mask.alpha[y * mask.width + x]);
            if coverage <= 0.0 {
                continue;
            }

            let sample = sample_brush_rgba(
                &brush,
                paint.canvas_origin.x + px as f32 + 0.5,
                paint.canvas_origin.y + py as f32 + 0.5,
                Point::default(),
            );
            let alpha = coverage * sample[3] * paint.alpha;
            if alpha <= 0.0 {
                continue;
            }
            let idx = (py as u32 * width + px as u32) as usize;
            blend_src_over(
                &mut canvas[idx],
                [sample[0], sample[1], sample[2], alpha.clamp(0.0, 1.0)],
            );
        }
    }
}

fn blend_src_over_u8(dst: &mut [u8], src: [f32; 4]) {
    let src_alpha = src[3].clamp(0.0, 1.0);
    if src_alpha <= 0.0 {
        return;
    }

    let dst_alpha = dst[3] as f32 / 255.0;
    if dst_alpha <= 0.0 {
        dst[0] = whole::round(src[0].clamp(0.0, 1.0) * 255.0) as u8;
        dst[1] = whole::round(src[1].clamp(0.0, 1.0) * 255.0) as u8;
        dst[2] = whole::round(src[2].clamp(0.0, 1.0) * 255.0) as u8;
        dst[3] = whole::round(src_alpha * 255.0) as u8;
        return;
    }

    let out_alpha = src_alpha + dst_alpha * (1.0 - src_alpha);
    if out_alpha <= f32::EPSILON {
        dst.fill(0);
        return;
    }

    for channel in 0..3 {
        let src_premult = src[channel].clamp(0.0, 1.0) * src_alpha;
        let dst_premult = (dst[channel] as f32 / 255.0) * dst_alpha;
        dst[channel] =
            whole::round((src_premult + dst_premult * (1.0 - src_alpha)) / out_alpha * 255.0) as u8;
    }
    dst[3] = whole::round(out_alpha.clamp(0.0, 1.0) * 255.0) as u8;
}

/// How a solid glyph's pixel goes over the pixel `P` under it.
trait SolidGlyphBlend<P> {
    /// Puts `src`, the glyph's color with its coverage as alpha, over
    /// `dst`; `alpha` gives that coverage as a unit value.
    fn blend(&self, dst: &mut P, src: [u8; 4], alpha: impl FnOnce() -> f32);
}

/// A text's own canvas: a pixel no glyph covered yet takes the glyph's,
/// and glyphs that overlap combine in floats before the canvas is drawn.
struct CanvasBlend {
    color: [f32; 4],
}

impl SolidGlyphBlend<[u8; 4]> for CanvasBlend {
    fn blend(&self, dst: &mut [u8; 4], src: [u8; 4], alpha: impl FnOnce() -> f32) {
        if dst[3] == 0 {
            *dst = src;
        } else {
            blend_src_over_u8(dst, [self.color[0], self.color[1], self.color[2], alpha()]);
        }
    }
}

/// A renderer's pixels: each glyph pixel goes over them as a text canvas's
/// pixel would, so a glyph that overlaps no other draws the same bytes.
struct OverBlend;

impl<P: GlyphPixel> SolidGlyphBlend<P> for OverBlend {
    fn blend(&self, dst: &mut P, src: [u8; 4], _alpha: impl FnOnce() -> f32) {
        dst.blend_bytes(src);
    }
}

/// Paints the solid glyph `placed` in `ink` over an `area` of `pixels`,
/// width by height, a row every `stride` pixels.
// Out of line, one function a format: a display takes one byte order, and a
// build that keeps its drawing in fast memory keeps only that one.
#[inline(never)]
fn draw_mask_glyph_solid<P>(
    pixels: &mut [P],
    stride: usize,
    (width, height): (u32, u32),
    placed: PlacedMask<'_>,
    ink: &SolidInk,
    blend: impl SolidGlyphBlend<P>,
) {
    let mask = placed.mask;
    let SolidInk {
        red,
        green,
        blue,
        alpha_scale,
        correction,
        ..
    } = *ink;
    if width == 0 || mask.width == 0 || stride == 0 {
        return;
    }
    // The mask columns and rows that land in the area, and the area's column
    // and row of the first of them. A row is found by its index: walking
    // rows through iterator adapters cost more than its pixels.
    let columns = canvas_span(placed.x, mask.width, width);
    let rows = canvas_span(placed.y, mask.height, height);
    let left = columns.start.saturating_add_signed(placed.x as isize);
    let top = rows.start.saturating_add_signed(placed.y as isize);
    for (row, mask_y) in (top..).zip(rows) {
        let start = mask_y.saturating_mul(mask.width);
        let coverage = mask
            .alpha
            .get(start + columns.start..start + columns.end)
            .unwrap_or_default();
        let area_row = row_pixels(pixels, stride, row, (left, columns.len()));
        for (dst, &mask_coverage) in area_row.iter_mut().zip(coverage) {
            let corrected = correction.apply(mask_coverage);
            if corrected == 0 {
                continue;
            }
            // At full alpha the byte is the corrected coverage itself: a
            // byte over 255 multiplied back by 255 rounds to that byte.
            let alpha =
                || (crate::text_mask_gamma::unit_coverage(corrected) * alpha_scale).clamp(0.0, 1.0);
            let alpha_u8 = if alpha_scale == 1.0 {
                corrected
            } else {
                whole::round(alpha() * 255.0) as u8
            };
            if alpha_u8 == 0 {
                continue;
            }
            blend.blend(dst, [red, green, blue, alpha_u8], alpha);
        }
    }
}

// The pixels of `row` of `pixels`, a row every `stride` of them, from
// column `left` on, `len` of them or fewer where the pixels end.
fn row_pixels<P>(
    pixels: &mut [P],
    stride: usize,
    row: usize,
    (left, len): (usize, usize),
) -> &mut [P] {
    let start = row.saturating_mul(stride);
    let end = start.saturating_add(stride).min(pixels.len());
    let from = start.saturating_add(left).min(end);
    let to = from.saturating_add(len).min(end);
    pixels.get_mut(from..to).unwrap_or_default()
}

/// The indices of a mask's `len` columns or rows, the first at `origin`
/// on the canvas, that land in the canvas's `extent`.
fn canvas_span(origin: i32, len: usize, extent: u32) -> std::ops::Range<usize> {
    let start = usize::try_from(-i64::from(origin)).unwrap_or(0).min(len);
    let end = usize::try_from(i64::from(extent) - i64::from(origin))
        .unwrap_or(0)
        .clamp(start, len);
    start..end
}
