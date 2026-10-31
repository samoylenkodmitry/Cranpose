use cranpose_render_common::{
    graph_scene::RenderDiagnostics,
    text_canvas::{TextPixelWindow, TextPixels},
    text_raster::{self, TextRasterRequest, paint_text},
};
use cranpose_ui::text::TextMotion;
use cranpose_ui_graphics::{Point, Rect};

use super::{
    Target, TextRaster, blit_image_between_pixels, clip_rect_to_bounds, on_pixel_grid,
    snap_delta_for_anchor,
};
use crate::{pixel::Pixel, scene::TextDraw};

pub(super) fn draw_text<P: Pixel>(
    target: &mut Target<'_, P>,
    draw: &TextDraw,
    diagnostics: &RenderDiagnostics,
    text: &TextRaster,
) {
    let Some(raster_rect) = text_raster_rect(draw) else {
        return;
    };
    // The glyphs draw only in their reach, which the text's damage covers.
    let reach = draw.glyph_reach(raster_rect);
    // A text on whole pixels rasterizes only its part inside the target, as
    // a strip takes a few rows of it; one between pixels samples its
    // neighbours and rasterizes whole.
    let part = if raster_rect.x.fract() == 0.0 && raster_rect.y.fract() == 0.0 {
        let Some(part) = clip_rect_to_bounds(reach, None, target) else {
            return;
        };
        if paint_text_over_target(draw, (raster_rect, reach), target, text) {
            return;
        }
        Rect {
            x: part.min_x as f32,
            y: part.min_y as f32,
            width: (part.max_x - part.min_x) as f32,
            height: (part.max_y - part.min_y) as f32,
        }
    } else {
        reach
    };
    draw_text_image(target, draw, (raster_rect, part), diagnostics, text);
}

// Draws `part` of `draw`, a text that covers `raster_rect`, through an image
// of its own, as a text between pixels or one with a gradient or a shadow
// takes. Out of line: the text most frames draw paints straight over the
// target, and the item loop keeps only the call.
#[inline(never)]
fn draw_text_image<P: Pixel>(
    target: &mut Target<'_, P>,
    draw: &TextDraw,
    (raster_rect, part): (Rect, Rect),
    diagnostics: &RenderDiagnostics,
    text: &TextRaster,
) {
    if let Some((pixels, rect)) = rasterize_text_part(draw, raster_rect, part, text) {
        blit_rasterized_text(target, rect, draw.clip, pixels, diagnostics);
    }
}

/// Paints the part of `reach` that the target and the clip of `draw` leave,
/// `draw` a text on whole pixels covering `raster_rect`, straight over the
/// target's pixels. Returns whether it did: a text that needs an image of
/// its own, as a gradient or a shadow does, goes through
/// [`rasterize_text_part`] instead.
fn paint_text_over_target<P: Pixel>(
    draw: &TextDraw,
    (raster_rect, reach): (Rect, Rect),
    target: &mut Target<'_, P>,
    text: &TextRaster,
) -> bool {
    let Some(bounds) = clip_rect_to_bounds(reach, draw.clip, target) else {
        return true;
    };
    let first = (bounds.min_y - target.top) as usize * target.width as usize
        + (bounds.min_x - target.left) as usize;
    let stride = target.width as usize;
    let Some(pixels) = target.pixels.get_mut(first..) else {
        return true;
    };
    let window = TextPixelWindow {
        pixels: P::text_rows(pixels),
        stride,
        width: (bounds.max_x - bounds.min_x) as u32,
        height: (bounds.max_y - bounds.min_y) as u32,
    };
    let mut glyphs = text.glyphs.try_borrow_mut().ok();
    let covered = Rect {
        x: bounds.min_x as f32,
        y: bounds.min_y as f32,
        width: window.width as f32,
        height: window.height as f32,
    };
    paint_text(
        raster_request(draw, covered, raster_rect),
        glyphs.as_deref_mut(),
        window,
    )
}

fn raster_request(draw: &TextDraw, part: Rect, raster_rect: Rect) -> TextRasterRequest<'_> {
    TextRasterRequest::new(
        &draw.layout,
        draw.text.as_ref(),
        part,
        &draw.text_style,
        draw.color,
        draw.scale,
    )
    .with_origin(Point::new(raster_rect.x, raster_rect.y))
    .with_ink(draw.ink)
}

/// `draw` rasterized into an image of its own, straight alpha, and the
/// rect that image covers.
pub(crate) fn rasterize_text(draw: &TextDraw, text: &TextRaster) -> Option<(TextPixels, Rect)> {
    let raster_rect = text_raster_rect(draw)?;
    rasterize_text_part(draw, raster_rect, draw.glyph_reach(raster_rect), text)
}

/// `part` of `draw`, whose text covers `raster_rect`, rasterized into an
/// image of its own, straight alpha, and the rect that image covers.
pub(super) fn rasterize_text_part(
    draw: &TextDraw,
    raster_rect: Rect,
    part: Rect,
    text: &TextRaster,
) -> Option<(TextPixels, Rect)> {
    let mut glyphs = text.glyphs.try_borrow_mut().ok();
    let pixels = text_raster::rasterize_text(
        raster_request(draw, part, raster_rect),
        glyphs.as_deref_mut(),
    )?;
    let rect = Rect {
        x: part.x,
        y: part.y,
        width: pixels.width as f32,
        height: pixels.height as f32,
    };
    Some((pixels, rect))
}

/// Whether `draw` keeps its glyphs on whole pixels, where the glyph cache
/// serves them.
pub(super) fn is_static_text(draw: &TextDraw) -> bool {
    draw.text_style
        .paragraph_style
        .text_motion
        .unwrap_or(TextMotion::Static)
        == TextMotion::Static
}

pub(super) fn text_raster_rect(draw: &TextDraw) -> Option<Rect> {
    if draw.scale.max(0.0) == 0.0 {
        return None;
    }
    if !is_static_text(draw) {
        return Some(draw.rect);
    }
    let snap_delta = draw
        .snap_anchor
        .map(snap_delta_for_anchor)
        .unwrap_or_default();
    Some(on_pixel_grid(
        draw.rect.translate(snap_delta.x, snap_delta.y),
    ))
}

fn blit_rasterized_text<P: Pixel>(
    target: &mut Target<'_, P>,
    rect: Rect,
    clip: Option<Rect>,
    text: TextPixels,
    diagnostics: &RenderDiagnostics,
) {
    if rect.width <= 0.0 || rect.height <= 0.0 || text.width == 0 || text.height == 0 {
        return;
    }
    let Some(clip_bounds) = clip_rect_to_bounds(rect, clip, target) else {
        return;
    };

    // The pixels are the rect's size. On whole pixels each pixel takes the
    // text pixel under it; between pixels it takes the text moved by the
    // fraction, sampled linearly from it as an image.
    if rect.x.fract() != 0.0 || rect.y.fract() != 0.0 {
        if let Some(image) = text.into_image() {
            blit_image_between_pixels(target, rect, clip_bounds, &image, diagnostics);
        }
        return;
    }
    let source_pixels = text.pixels.as_chunks::<4>().0;
    for py in clip_bounds.min_y..clip_bounds.max_y {
        let row = target.row(py, clip_bounds.min_x, clip_bounds.max_x);
        let first = (py as f32 - rect.y) as usize * text.width as usize
            + (clip_bounds.min_x as f32 - rect.x) as usize;
        let source = source_pixels
            .get(first..first + row.len())
            .unwrap_or_default();
        for (dst, src) in row.iter_mut().zip(source) {
            dst.blend_bytes(*src);
        }
    }
}
