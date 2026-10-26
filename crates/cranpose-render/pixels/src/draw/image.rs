//! Drawing images: placed where their snap and transform put them, sampled
//! nearest or linear, with their source rect, colour filter and alpha.

use cranpose_render_common::graph_scene::RenderDiagnostics;
use cranpose_ui_graphics::{BlendMode, Point, Rect, whole};

use super::{ClipBounds, Target, UNIT, clip_rect_to_bounds, snap_delta_for_anchor};
use crate::{pixel::Pixel, scene::ImageDraw};

/// The device pixels an image covers: its rect where it lands, or the box
/// of its turned rect.
pub(super) fn image_rect(draw: &ImageDraw) -> Rect {
    if let Some(transform) = draw.transform {
        return transform.bounds_for_rect(draw.rect);
    }
    let snap_delta = draw
        .snap_anchor
        .map(snap_delta_for_anchor)
        .unwrap_or_default();
    draw.rect.translate(snap_delta.x, snap_delta.y)
}

#[inline(never)]
pub(super) fn draw_image<P: Pixel>(
    target: &mut Target<'_, P>,
    draw: &ImageDraw,
    diagnostics: &RenderDiagnostics,
) {
    let rect = image_rect(draw);
    if draw.alpha <= 0.0 || rect.width <= 0.0 || rect.height <= 0.0 {
        return;
    }
    let Some(clip_bounds) = clip_rect_to_bounds(rect, draw.clip, target) else {
        return;
    };
    let Some(sampler) = ImageSampler::of(draw) else {
        return;
    };
    // A turned image is read back through its transform: each pixel's
    // centre lands somewhere in its rect, or outside it.
    let to_image = match draw.transform {
        Some(transform) => match transform.inverse() {
            Some(inverse) => Some(inverse),
            None => return,
        },
        None => None,
    };
    let image = draw.rect;
    let local = |x: f32, y: f32| match to_image {
        Some(inverse) => {
            let point = inverse.map_point_in_front(Point::new(x, y))?;
            image.contains(point.x, point.y).then(|| {
                (
                    (point.x - image.x) / image.width,
                    (point.y - image.y) / image.height,
                )
            })
        }
        None => Some((
            ((x - rect.x) / rect.width).clamp(0.0, 1.0),
            ((y - rect.y) / rect.height).clamp(0.0, 1.0),
        )),
    };
    for py in clip_bounds.min_y..clip_bounds.max_y {
        let sample_y = py as f32 + 0.5;
        let row = target.row(py, clip_bounds.min_x, clip_bounds.max_x);
        for (px, dst) in (clip_bounds.min_x..).zip(row) {
            let Some((u, v)) = local(px as f32 + 0.5, sample_y) else {
                continue;
            };
            let sample = sampler.sample(u, v);
            if sample[3] > 0.0 {
                dst.blend_unit(sample, draw.blend_mode, diagnostics);
            }
        }
    }
}

/// Reads an image's pixels at a place within its rect, with its sampling,
/// source rect, colour filter and alpha applied.
struct ImageSampler<'a> {
    draw: &'a ImageDraw,
    source: Rect,
}

impl<'a> ImageSampler<'a> {
    /// `None` for an image with no pixels of its own to read.
    fn of(draw: &'a ImageDraw) -> Option<Self> {
        let (width, height) = (draw.image.width(), draw.image.height());
        if width == 0
            || height == 0
            || draw.image.format() == cranpose_ui_graphics::ImagePixelFormat::External
        {
            return None;
        }
        let source = draw.src_rect.unwrap_or(Rect {
            x: 0.0,
            y: 0.0,
            width: width as f32,
            height: height as f32,
        });
        Some(Self { draw, source })
    }

    /// The colour at `u`, `v`, the fractions of the rect's width and height.
    fn sample(&self, u: f32, v: f32) -> [f32; 4] {
        let Self { draw, source } = self;
        let image = &draw.image;
        let x = source.x + u * source.width;
        let y = source.y + v * source.height;
        let mut sample = match draw.sampling {
            cranpose_ui_graphics::ImageSampling::Nearest => {
                let src_x = (whole::floor(x) as i32).clamp(0, image.width() as i32 - 1);
                let src_y = (whole::floor(y) as i32).clamp(0, image.height() as i32 - 1);
                sample_image_nearest(image, src_x as u32, src_y as u32)
            }
            cranpose_ui_graphics::ImageSampling::Linear => {
                sample_image_linear(image, x - 0.5, y - 0.5)
            }
        };
        if let Some(filter) = &draw.color_filter {
            sample = filter.apply_rgba(sample);
        }
        sample[3] *= draw.alpha.clamp(0.0, 1.0);
        sample
    }
}

fn sample_image_nearest(
    image: &cranpose_ui_graphics::ImageBitmap,
    src_x: u32,
    src_y: u32,
) -> [f32; 4] {
    image
        .rgba8_pixel(src_y as usize * image.width() as usize + src_x as usize)
        .map(|channel| UNIT[usize::from(channel)])
}

fn sample_image_linear(image: &cranpose_ui_graphics::ImageBitmap, x: f32, y: f32) -> [f32; 4] {
    let img_width = image.width();
    let img_height = image.height();
    let x = x.clamp(0.0, img_width.saturating_sub(1) as f32);
    let y = y.clamp(0.0, img_height.saturating_sub(1) as f32);
    let x0 = whole::floor(x);
    let y0 = whole::floor(y);
    let tx = x - x0;
    let ty = y - y0;
    let x0 = (x0 as i32).clamp(0, img_width as i32 - 1) as u32;
    let y0 = (y0 as i32).clamp(0, img_height as i32 - 1) as u32;
    let x1 = (x0 + 1).min(img_width - 1);
    let y1 = (y0 + 1).min(img_height - 1);
    let top_left = sample_image_nearest(image, x0, y0);
    let top_right = sample_image_nearest(image, x1, y0);
    let bottom_left = sample_image_nearest(image, x0, y1);
    let bottom_right = sample_image_nearest(image, x1, y1);

    let mut out = [0.0; 4];
    for channel in 0..4 {
        let top = top_left[channel] + (top_right[channel] - top_left[channel]) * tx;
        let bottom = bottom_left[channel] + (bottom_right[channel] - bottom_left[channel]) * tx;
        out[channel] = top + (bottom - top) * ty;
    }
    out
}

/// Draws `image`, the size of `rect`, at a rect between whole pixels: each
/// pixel samples the image linearly.
pub(super) fn blit_image_between_pixels<P: Pixel>(
    target: &mut Target<'_, P>,
    rect: Rect,
    clip_bounds: ClipBounds,
    image: &cranpose_ui_graphics::ImageBitmap,
    diagnostics: &RenderDiagnostics,
) {
    for py in clip_bounds.min_y..clip_bounds.max_y {
        let image_y = py as f32 - rect.y;
        let row = target.row(py, clip_bounds.min_x, clip_bounds.max_x);
        for (px, dst) in (clip_bounds.min_x..).zip(row) {
            let src = sample_image_linear(image, px as f32 - rect.x, image_y);
            if src[3] <= 0.0 {
                continue;
            }
            dst.blend_unit(src, BlendMode::SrcOver, diagnostics);
        }
    }
}
