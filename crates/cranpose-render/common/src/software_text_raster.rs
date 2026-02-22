use cranpose_ui::text::{Shadow, TextDrawStyle, TextMotion, TextStyle};
use cranpose_ui_graphics::{Color, ImageBitmap, Rect, TileMode};
use rusttype::{point, Font, OutlineBuilder, Scale};
use tiny_skia::{LineCap, LineJoin, Paint, Path, PathBuilder, Pixmap, Stroke, Transform};

use crate::Brush;

const COMPOSE_STROKE_MITER_LIMIT: f32 = 4.0;
const SHADOW_SIGMA_SCALE: f32 = 0.57735;
const SHADOW_SIGMA_BIAS: f32 = 0.5;
const MAX_GAUSSIAN_KERNEL_HALF: i32 = 128;

#[derive(Clone, Copy)]
enum GlyphRasterStyle {
    Fill,
    Stroke { width_px: f32 },
}

struct GlyphMask {
    alpha: Vec<f32>,
    width: usize,
    height: usize,
    origin_x: i32,
    origin_y: i32,
}

pub fn rasterize_text_to_image_with_font(
    text: &str,
    rect: Rect,
    style: &TextStyle,
    fallback_color: Color,
    font_size: f32,
    scale: f32,
    font: &Font<'_>,
) -> Option<ImageBitmap> {
    if text.is_empty()
        || rect.width <= 0.0
        || rect.height <= 0.0
        || !font_size.is_finite()
        || font_size <= 0.0
        || !scale.is_finite()
        || scale <= 0.0
    {
        return None;
    }

    let width = rect.width.ceil().max(1.0) as u32;
    let height = rect.height.ceil().max(1.0) as u32;
    let mut canvas = vec![[0.0f32; 4]; (width * height) as usize];

    let fallback_brush = Brush::solid(fallback_color);
    let (brush, brush_alpha_multiplier) = match style.span_style.brush.as_ref() {
        Some(brush) => (brush, style.span_style.alpha.unwrap_or(1.0).clamp(0.0, 1.0)),
        None => (&fallback_brush, 1.0),
    };
    let raster_style = match style.span_style.draw_style.unwrap_or(TextDrawStyle::Fill) {
        TextDrawStyle::Fill => GlyphRasterStyle::Fill,
        TextDrawStyle::Stroke { width } => {
            if width.is_finite() && width > 0.0 {
                GlyphRasterStyle::Stroke {
                    width_px: width * scale,
                }
            } else {
                GlyphRasterStyle::Fill
            }
        }
    };
    let shadow = style
        .span_style
        .shadow
        .filter(|shadow| shadow.color.3 > 0.0);
    let static_text_motion = style
        .paragraph_style
        .text_motion
        .unwrap_or(TextMotion::Static)
        == TextMotion::Static;

    let origin_x = if static_text_motion {
        0.0
    } else {
        rect.x.fract()
    };
    let origin_y = if static_text_motion {
        0.0
    } else {
        rect.y.fract()
    };

    let scale_px = Scale::uniform(font_size * scale);
    let v_metrics = font.v_metrics(scale_px);
    let line_height = style
        .resolve_line_height(14.0, (v_metrics.ascent - v_metrics.descent).ceil())
        .max(1.0);

    for (line_idx, line) in text.split('\n').enumerate() {
        let baseline_y = v_metrics.ascent + line_idx as f32 * line_height + origin_y;
        let offset = point(origin_x, baseline_y);

        for glyph in font.layout(line, scale_px, offset) {
            let glyph = align_glyph_for_text_motion(glyph, static_text_motion);
            if let Some(bb) = glyph.pixel_bounding_box() {
                let Some(mask) = build_glyph_mask(&glyph, bb, raster_style) else {
                    continue;
                };

                if let Some(shadow) = shadow {
                    draw_shadow_mask(
                        &mut canvas,
                        width,
                        height,
                        &mask,
                        shadow,
                        scale,
                        static_text_motion,
                    );
                }

                draw_mask_glyph(
                    &mut canvas,
                    width,
                    height,
                    &mask,
                    brush,
                    brush_alpha_multiplier,
                    rect,
                );
            }
        }
    }

    let mut rgba = vec![0u8; canvas.len() * 4];
    for (index, pixel) in canvas.iter().enumerate() {
        let base = index * 4;
        rgba[base] = (pixel[0].clamp(0.0, 1.0) * 255.0).round() as u8;
        rgba[base + 1] = (pixel[1].clamp(0.0, 1.0) * 255.0).round() as u8;
        rgba[base + 2] = (pixel[2].clamp(0.0, 1.0) * 255.0).round() as u8;
        rgba[base + 3] = (pixel[3].clamp(0.0, 1.0) * 255.0).round() as u8;
    }

    ImageBitmap::from_rgba8(width, height, rgba).ok()
}

fn align_glyph_for_text_motion(
    glyph: rusttype::PositionedGlyph<'_>,
    static_text_motion: bool,
) -> rusttype::PositionedGlyph<'_> {
    if !static_text_motion {
        return glyph;
    }

    let position = glyph.position();
    let snapped_x = position.x.round();
    let snapped_y = position.y.round();
    if (snapped_x - position.x).abs() < f32::EPSILON
        && (snapped_y - position.y).abs() < f32::EPSILON
    {
        return glyph;
    }

    glyph
        .into_unpositioned()
        .positioned(point(snapped_x, snapped_y))
}

fn blend_src_over(dst: &mut [f32; 4], src: [f32; 4]) {
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

fn draw_mask_glyph(
    canvas: &mut [[f32; 4]],
    width: u32,
    height: u32,
    mask: &GlyphMask,
    brush: &Brush,
    brush_alpha_multiplier: f32,
    brush_rect: Rect,
) {
    for y in 0..mask.height {
        let py = mask.origin_y + y as i32;
        if py < 0 || py >= height as i32 {
            continue;
        }

        for x in 0..mask.width {
            let px = mask.origin_x + x as i32;
            if px < 0 || px >= width as i32 {
                continue;
            }

            let coverage = mask.alpha[y * mask.width + x];
            if coverage <= 0.0 {
                continue;
            }

            let sample = sample_brush(
                brush,
                brush_rect,
                brush_rect.x + px as f32 + 0.5,
                brush_rect.y + py as f32 + 0.5,
            );
            let alpha = coverage * sample[3] * brush_alpha_multiplier;
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

fn draw_shadow_mask(
    canvas: &mut [[f32; 4]],
    width: u32,
    height: u32,
    mask: &GlyphMask,
    shadow: Shadow,
    text_scale: f32,
    static_text_motion: bool,
) {
    if mask.width == 0 || mask.height == 0 {
        return;
    }

    let shadow_dx = shadow.offset.x * text_scale;
    let shadow_dy = shadow.offset.y * text_scale;
    let blur_radius = (shadow.blur_radius * text_scale).max(0.0);
    let sigma = shadow_blur_sigma(blur_radius);
    let blur_margin = if sigma > 0.0 {
        (sigma * 3.0).ceil() as i32
    } else {
        0
    };

    let padded_width = mask.width + (blur_margin as usize) * 2;
    let padded_height = mask.height + (blur_margin as usize) * 2;
    let mut padded_mask = vec![0.0f32; padded_width * padded_height];

    for y in 0..mask.height {
        let src_offset = y * mask.width;
        let dst_offset = (y + blur_margin as usize) * padded_width + blur_margin as usize;
        padded_mask[dst_offset..dst_offset + mask.width]
            .copy_from_slice(&mask.alpha[src_offset..src_offset + mask.width]);
    }

    let blurred = if sigma > 0.0 {
        gaussian_blur_alpha(&padded_mask, padded_width, padded_height, sigma)
    } else {
        padded_mask
    };

    let shadow_rgba = color_to_rgba(shadow.color);
    let shadow_origin_x = mask.origin_x - blur_margin;
    let shadow_origin_y = mask.origin_y - blur_margin;

    for y in 0..padded_height {
        for x in 0..padded_width {
            let alpha = blurred[y * padded_width + x] * shadow_rgba[3];
            if alpha <= 0.0 {
                continue;
            }

            let target_x = shadow_origin_x as f32 + x as f32 + shadow_dx;
            let target_y = shadow_origin_y as f32 + y as f32 + shadow_dy;
            if static_text_motion {
                blend_shadow_pixel(
                    canvas,
                    width,
                    height,
                    target_x.round() as i32,
                    target_y.round() as i32,
                    shadow_rgba,
                    alpha.clamp(0.0, 1.0),
                );
            } else {
                blend_shadow_pixel_subpixel(
                    canvas,
                    width,
                    height,
                    target_x,
                    target_y,
                    shadow_rgba,
                    alpha.clamp(0.0, 1.0),
                );
            }
        }
    }
}

fn blend_shadow_pixel(
    canvas: &mut [[f32; 4]],
    width: u32,
    height: u32,
    px: i32,
    py: i32,
    color: [f32; 4],
    alpha: f32,
) {
    if px < 0 || py < 0 || px >= width as i32 || py >= height as i32 || alpha <= 0.0 {
        return;
    }
    let idx = (py as u32 * width + px as u32) as usize;
    blend_src_over(
        &mut canvas[idx],
        [color[0], color[1], color[2], alpha.clamp(0.0, 1.0)],
    );
}

fn blend_shadow_pixel_subpixel(
    canvas: &mut [[f32; 4]],
    width: u32,
    height: u32,
    x: f32,
    y: f32,
    color: [f32; 4],
    alpha: f32,
) {
    if alpha <= 0.0 {
        return;
    }

    let base_x = x.floor();
    let base_y = y.floor();
    let frac_x = x - base_x;
    let frac_y = y - base_y;
    let base_x_i32 = base_x as i32;
    let base_y_i32 = base_y as i32;
    let weights = [
        ((1.0 - frac_x) * (1.0 - frac_y), 0i32, 0i32),
        (frac_x * (1.0 - frac_y), 1, 0),
        ((1.0 - frac_x) * frac_y, 0, 1),
        (frac_x * frac_y, 1, 1),
    ];

    for (weight, dx, dy) in weights {
        if weight <= 0.0 {
            continue;
        }
        blend_shadow_pixel(
            canvas,
            width,
            height,
            base_x_i32 + dx,
            base_y_i32 + dy,
            color,
            alpha * weight,
        );
    }
}

fn shadow_blur_sigma(blur_radius: f32) -> f32 {
    if blur_radius <= 0.0 {
        0.0
    } else {
        (blur_radius * SHADOW_SIGMA_SCALE + SHADOW_SIGMA_BIAS).max(0.5)
    }
}

fn gaussian_blur_alpha(src: &[f32], width: usize, height: usize, sigma: f32) -> Vec<f32> {
    let kernel = gaussian_kernel_1d(sigma);
    if kernel.len() == 1 {
        return src.to_vec();
    }
    let half = (kernel.len() / 2) as i32;

    let mut horizontal = vec![0.0f32; src.len()];
    for y in 0..height {
        for x in 0..width {
            let mut sum = 0.0f32;
            for (index, weight) in kernel.iter().enumerate() {
                let offset = index as i32 - half;
                let sample_x = (x as i32 + offset).clamp(0, width as i32 - 1) as usize;
                sum += src[y * width + sample_x] * *weight;
            }
            horizontal[y * width + x] = sum;
        }
    }

    let mut output = vec![0.0f32; src.len()];
    for y in 0..height {
        for x in 0..width {
            let mut sum = 0.0f32;
            for (index, weight) in kernel.iter().enumerate() {
                let offset = index as i32 - half;
                let sample_y = (y as i32 + offset).clamp(0, height as i32 - 1) as usize;
                sum += horizontal[sample_y * width + x] * *weight;
            }
            output[y * width + x] = sum;
        }
    }

    output
}

fn gaussian_kernel_1d(sigma: f32) -> Vec<f32> {
    let half = ((sigma * 3.0).ceil() as i32).clamp(1, MAX_GAUSSIAN_KERNEL_HALF);
    if half <= 0 {
        return vec![1.0];
    }

    let mut kernel = Vec::with_capacity((half * 2 + 1) as usize);
    let mut sum = 0.0f32;
    for offset in -half..=half {
        let distance = offset as f32;
        let weight = (-0.5 * (distance / sigma).powi(2)).exp();
        kernel.push(weight);
        sum += weight;
    }

    if sum > f32::EPSILON {
        for weight in &mut kernel {
            *weight /= sum;
        }
    }

    kernel
}

fn build_glyph_mask(
    glyph: &rusttype::PositionedGlyph<'_>,
    bb: rusttype::Rect<i32>,
    style: GlyphRasterStyle,
) -> Option<GlyphMask> {
    match style {
        GlyphRasterStyle::Fill => build_fill_mask(glyph, bb),
        GlyphRasterStyle::Stroke { width_px } => build_stroke_mask(glyph, bb, width_px),
    }
}

fn build_fill_mask(
    glyph: &rusttype::PositionedGlyph<'_>,
    bb: rusttype::Rect<i32>,
) -> Option<GlyphMask> {
    let mask_width = (bb.max.x - bb.min.x).max(0) as usize;
    let mask_height = (bb.max.y - bb.min.y).max(0) as usize;
    if mask_width == 0 || mask_height == 0 {
        return None;
    }

    let mut alpha = vec![0.0f32; mask_width * mask_height];
    glyph.draw(|gx, gy, value| {
        let idx = gy as usize * mask_width + gx as usize;
        alpha[idx] = value;
    });

    Some(GlyphMask {
        alpha,
        width: mask_width,
        height: mask_height,
        origin_x: bb.min.x,
        origin_y: bb.min.y,
    })
}

fn build_stroke_mask(
    glyph: &rusttype::PositionedGlyph<'_>,
    bb: rusttype::Rect<i32>,
    stroke_width_px: f32,
) -> Option<GlyphMask> {
    if !stroke_width_px.is_finite() || stroke_width_px <= 0.0 {
        return build_fill_mask(glyph, bb);
    }

    let mask_width = (bb.max.x - bb.min.x).max(0);
    let mask_height = (bb.max.y - bb.min.y).max(0);
    if mask_width <= 0 || mask_height <= 0 {
        return None;
    }

    let mut path_builder = GlyphPathBuilder::default();
    if !glyph.build_outline(&mut path_builder) {
        return None;
    }
    let path = path_builder.finish()?;

    let half_width = stroke_width_px * 0.5;
    let miter_pad = (half_width * COMPOSE_STROKE_MITER_LIMIT).ceil();
    let pad = miter_pad.max(1.0) as i32 + 1;
    let raster_width = mask_width + pad * 2;
    let raster_height = mask_height + pad * 2;
    if raster_width <= 0 || raster_height <= 0 {
        return None;
    }

    let mut pixmap = Pixmap::new(raster_width as u32, raster_height as u32)?;
    let mut paint = Paint::default();
    paint.set_color_rgba8(255, 255, 255, 255);
    paint.anti_alias = true;

    let stroke = Stroke {
        width: stroke_width_px,
        line_cap: LineCap::Butt,
        line_join: LineJoin::Miter,
        miter_limit: COMPOSE_STROKE_MITER_LIMIT,
        ..Stroke::default()
    };

    pixmap.stroke_path(
        &path,
        &paint,
        &stroke,
        Transform::from_translate(pad as f32, pad as f32),
        None,
    );

    let alpha = pixmap
        .data()
        .chunks_exact(4)
        .map(|pixel| pixel[3] as f32 / 255.0)
        .collect();

    Some(GlyphMask {
        alpha,
        width: raster_width as usize,
        height: raster_height as usize,
        origin_x: bb.min.x - pad,
        origin_y: bb.min.y - pad,
    })
}

#[derive(Default)]
struct GlyphPathBuilder {
    builder: PathBuilder,
    has_segments: bool,
}

impl GlyphPathBuilder {
    fn finish(self) -> Option<Path> {
        if !self.has_segments {
            return None;
        }
        self.builder.finish()
    }
}

impl OutlineBuilder for GlyphPathBuilder {
    fn move_to(&mut self, x: f32, y: f32) {
        self.builder.move_to(x, y);
        self.has_segments = true;
    }

    fn line_to(&mut self, x: f32, y: f32) {
        self.builder.line_to(x, y);
        self.has_segments = true;
    }

    fn quad_to(&mut self, x1: f32, y1: f32, x: f32, y: f32) {
        self.builder.quad_to(x1, y1, x, y);
        self.has_segments = true;
    }

    fn curve_to(&mut self, x1: f32, y1: f32, x2: f32, y2: f32, x: f32, y: f32) {
        self.builder.cubic_to(x1, y1, x2, y2, x, y);
        self.has_segments = true;
    }

    fn close(&mut self) {
        self.builder.close();
    }
}

fn color_to_rgba(color: Color) -> [f32; 4] {
    [
        color.0.clamp(0.0, 1.0),
        color.1.clamp(0.0, 1.0),
        color.2.clamp(0.0, 1.0),
        color.3.clamp(0.0, 1.0),
    ]
}

fn sample_brush(brush: &Brush, rect: Rect, x: f32, y: f32) -> [f32; 4] {
    match brush {
        Brush::Solid(color) => color_to_rgba(*color),
        Brush::LinearGradient {
            colors,
            stops,
            start,
            end,
            tile_mode,
        } => {
            let sx = resolve_gradient_point(rect.x, rect.width, start.x);
            let sy = resolve_gradient_point(rect.y, rect.height, start.y);
            let ex = resolve_gradient_point(rect.x, rect.width, end.x);
            let ey = resolve_gradient_point(rect.y, rect.height, end.y);
            let dx = ex - sx;
            let dy = ey - sy;
            let denom = (dx * dx + dy * dy).max(f32::EPSILON);
            let t = ((x - sx) * dx + (y - sy) * dy) / denom;
            match normalize_gradient_t(t, *tile_mode) {
                Some(sample_t) => {
                    color_to_rgba(interpolate_colors(colors, stops.as_deref(), sample_t))
                }
                None => [0.0, 0.0, 0.0, 0.0],
            }
        }
        Brush::RadialGradient {
            colors,
            stops,
            center,
            radius,
            tile_mode,
        } => {
            let cx = rect.x + center.x;
            let cy = rect.y + center.y;
            let radius = (*radius).max(f32::EPSILON);
            let dx = x - cx;
            let dy = y - cy;
            let distance = (dx * dx + dy * dy).sqrt();
            let t = distance / radius;
            match normalize_gradient_t(t, *tile_mode) {
                Some(sample_t) => {
                    color_to_rgba(interpolate_colors(colors, stops.as_deref(), sample_t))
                }
                None => [0.0, 0.0, 0.0, 0.0],
            }
        }
        Brush::SweepGradient {
            colors,
            stops,
            center,
        } => {
            let cx = rect.x + center.x;
            let cy = rect.y + center.y;
            let dx = x - cx;
            let dy = y - cy;
            let angle = dy.atan2(dx);
            let t = (angle / std::f32::consts::TAU + 0.5).clamp(0.0, 1.0);
            color_to_rgba(interpolate_colors(colors, stops.as_deref(), t))
        }
    }
}

fn resolve_gradient_point(origin: f32, extent: f32, value: f32) -> f32 {
    if value.is_finite() {
        origin + value
    } else if value.is_sign_positive() {
        origin + extent
    } else {
        origin
    }
}

fn normalize_gradient_t(t: f32, tile_mode: TileMode) -> Option<f32> {
    match tile_mode {
        TileMode::Clamp => Some(t.clamp(0.0, 1.0)),
        TileMode::Decal => {
            if (0.0..=1.0).contains(&t) {
                Some(t)
            } else {
                None
            }
        }
        TileMode::Repeated => Some(t.rem_euclid(1.0)),
        TileMode::Mirror => {
            let wrapped = t.rem_euclid(2.0);
            if wrapped <= 1.0 {
                Some(wrapped)
            } else {
                Some(2.0 - wrapped)
            }
        }
    }
}

fn interpolate_colors(colors: &[Color], stops: Option<&[f32]>, t: f32) -> Color {
    if colors.is_empty() {
        return Color(0.0, 0.0, 0.0, 0.0);
    }
    if colors.len() == 1 {
        return colors[0];
    }
    let clamped = t.clamp(0.0, 1.0);

    if let Some(stops) = stops {
        if stops.len() == colors.len() {
            if clamped <= stops[0] {
                return colors[0];
            }
            for index in 0..(stops.len() - 1) {
                let start = stops[index];
                let end = stops[index + 1];
                if clamped <= end {
                    let span = (end - start).max(f32::EPSILON);
                    let frac = ((clamped - start) / span).clamp(0.0, 1.0);
                    return lerp_color(colors[index], colors[index + 1], frac);
                }
            }
            return *colors.last().unwrap_or(&colors[0]);
        }
    }

    let segments = (colors.len() - 1) as f32;
    let scaled = clamped * segments;
    let index = scaled.floor() as usize;
    if index >= colors.len() - 1 {
        return *colors.last().unwrap();
    }
    let frac = scaled - index as f32;
    lerp_color(colors[index], colors[index + 1], frac)
}

fn lerp_color(a: Color, b: Color, t: f32) -> Color {
    let lerp = |start: f32, end: f32| start + (end - start) * t;
    Color(
        lerp(a.0, b.0),
        lerp(a.1, b.1),
        lerp(a.2, b.2),
        lerp(a.3, b.3),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use cranpose_ui::text::SpanStyle;
    use cranpose_ui_graphics::Point;

    fn count_ink_pixels(image: &ImageBitmap) -> usize {
        image
            .pixels()
            .chunks_exact(4)
            .filter(|px| px[3] > 0)
            .count()
    }

    fn average_ink_rgb(
        image: &ImageBitmap,
        x_start: u32,
        x_end: u32,
        y_start: u32,
        y_end: u32,
    ) -> Option<[f32; 3]> {
        let width = image.width();
        let height = image.height();
        let mut sums = [0.0f32; 3];
        let mut count = 0usize;
        let pixels = image.pixels();

        let x_end = x_end.min(width);
        let y_end = y_end.min(height);
        for y in y_start.min(height)..y_end {
            for x in x_start.min(width)..x_end {
                let idx = ((y * width + x) * 4) as usize;
                let alpha = pixels[idx + 3];
                if alpha == 0 {
                    continue;
                }
                sums[0] += pixels[idx] as f32 / 255.0;
                sums[1] += pixels[idx + 1] as f32 / 255.0;
                sums[2] += pixels[idx + 2] as f32 / 255.0;
                count += 1;
            }
        }

        if count == 0 {
            return None;
        }
        Some([
            sums[0] / count as f32,
            sums[1] / count as f32,
            sums[2] / count as f32,
        ])
    }

    fn top_ink_row(image: &ImageBitmap) -> Option<u32> {
        let width = image.width();
        let height = image.height();
        let pixels = image.pixels();
        for y in 0..height {
            for x in 0..width {
                let idx = ((y * width + x) * 4) as usize;
                if pixels[idx + 3] > 0 {
                    return Some(y);
                }
            }
        }
        None
    }

    fn reference_dilation_offsets(radius: i32) -> Vec<(i32, i32)> {
        let mut offsets = Vec::new();
        let squared_radius = radius * radius;
        for dy in -radius..=radius {
            for dx in -radius..=radius {
                if dx * dx + dy * dy <= squared_radius {
                    offsets.push((dx, dy));
                }
            }
        }
        if offsets.is_empty() {
            offsets.push((0, 0));
        }
        offsets
    }

    fn reference_dilation_stroke_mask(fill: &GlyphMask, stroke_width: f32) -> GlyphMask {
        let radius = (stroke_width * 0.5).ceil() as i32;
        let offsets = reference_dilation_offsets(radius);
        let out_width = fill.width as i32 + radius * 2;
        let out_height = fill.height as i32 + radius * 2;
        let fill_width_i32 = fill.width as i32;
        let fill_height_i32 = fill.height as i32;
        let mut alpha = vec![0.0f32; (out_width * out_height) as usize];

        for out_y in 0..out_height {
            let oy = out_y - radius;
            for out_x in 0..out_width {
                let ox = out_x - radius;
                let base_alpha =
                    if ox >= 0 && oy >= 0 && ox < fill_width_i32 && oy < fill_height_i32 {
                        fill.alpha[oy as usize * fill.width + ox as usize]
                    } else {
                        0.0
                    };

                let mut dilated_alpha = 0.0f32;
                for (dx, dy) in &offsets {
                    let sx = ox + dx;
                    let sy = oy + dy;
                    if sx < 0 || sy < 0 || sx >= fill_width_i32 || sy >= fill_height_i32 {
                        continue;
                    }
                    let sample = fill.alpha[sy as usize * fill.width + sx as usize];
                    if sample > dilated_alpha {
                        dilated_alpha = sample;
                        if dilated_alpha >= 0.999 {
                            break;
                        }
                    }
                }
                alpha[out_y as usize * out_width as usize + out_x as usize] =
                    (dilated_alpha - base_alpha).max(0.0);
            }
        }

        GlyphMask {
            alpha,
            width: out_width as usize,
            height: out_height as usize,
            origin_x: fill.origin_x - radius,
            origin_y: fill.origin_y - radius,
        }
    }

    fn rasterize_reference_dilation_stroke(
        text: &str,
        rect: Rect,
        font_size: f32,
        stroke_width: f32,
        font: &Font<'_>,
    ) -> ImageBitmap {
        let width = rect.width.ceil().max(1.0) as u32;
        let height = rect.height.ceil().max(1.0) as u32;
        let mut canvas = vec![[0.0f32; 4]; (width * height) as usize];

        let scale_px = Scale::uniform(font_size);
        let v_metrics = font.v_metrics(scale_px);
        let baseline = v_metrics.ascent;
        for glyph in font.layout(text, scale_px, point(0.0, baseline)) {
            let Some(bb) = glyph.pixel_bounding_box() else {
                continue;
            };
            let Some(fill) = build_fill_mask(&glyph, bb) else {
                continue;
            };
            let reference = reference_dilation_stroke_mask(&fill, stroke_width);
            draw_mask_glyph(
                &mut canvas,
                width,
                height,
                &reference,
                &Brush::solid(Color::WHITE),
                1.0,
                rect,
            );
        }

        let mut rgba = vec![0u8; canvas.len() * 4];
        for (index, pixel) in canvas.iter().enumerate() {
            let base = index * 4;
            rgba[base] = (pixel[0].clamp(0.0, 1.0) * 255.0).round() as u8;
            rgba[base + 1] = (pixel[1].clamp(0.0, 1.0) * 255.0).round() as u8;
            rgba[base + 2] = (pixel[2].clamp(0.0, 1.0) * 255.0).round() as u8;
            rgba[base + 3] = (pixel[3].clamp(0.0, 1.0) * 255.0).round() as u8;
        }
        ImageBitmap::from_rgba8(width, height, rgba).expect("reference dilation image")
    }

    #[test]
    fn rasterized_gradient_text_shows_color_transition() {
        let font = Font::try_from_bytes(include_bytes!(
            "../../../../apps/desktop-demo/assets/Roboto-Regular.ttf"
        ) as &[u8])
        .expect("font");
        let style = TextStyle {
            span_style: SpanStyle {
                brush: Some(Brush::linear_gradient_range(
                    vec![Color::RED, Color::BLUE],
                    Point::new(0.0, 0.0),
                    Point::new(320.0, 0.0),
                )),
                ..Default::default()
            },
            ..Default::default()
        };

        let image = rasterize_text_to_image_with_font(
            "MMMMMMMM",
            Rect {
                x: 0.0,
                y: 0.0,
                width: 320.0,
                height: 96.0,
            },
            &style,
            Color::WHITE,
            48.0,
            1.0,
            &font,
        )
        .expect("rasterized image");

        let left = average_ink_rgb(&image, 12, 130, 8, 90).expect("left ink");
        let right = average_ink_rgb(&image, 190, 308, 8, 90).expect("right ink");
        assert!(
            left[0] > left[2] * 1.15,
            "left region should be red dominant, got {left:?}"
        );
        assert!(
            right[2] > right[0] * 1.15,
            "right region should be blue dominant, got {right:?}"
        );
    }

    #[test]
    fn rasterized_stroke_and_fill_ink_coverage_differs() {
        let font = Font::try_from_bytes(include_bytes!(
            "../../../../apps/desktop-demo/assets/Roboto-Regular.ttf"
        ) as &[u8])
        .expect("font");
        let fill_style = TextStyle::default();
        let stroke_style = TextStyle {
            span_style: SpanStyle {
                draw_style: Some(TextDrawStyle::Stroke { width: 6.0 }),
                ..Default::default()
            },
            ..Default::default()
        };
        let rect = Rect {
            x: 0.0,
            y: 0.0,
            width: 320.0,
            height: 96.0,
        };

        let fill = rasterize_text_to_image_with_font(
            "MMMMMMMM",
            rect,
            &fill_style,
            Color::WHITE,
            48.0,
            1.0,
            &font,
        )
        .expect("fill image");
        let stroke = rasterize_text_to_image_with_font(
            "MMMMMMMM",
            rect,
            &stroke_style,
            Color::WHITE,
            48.0,
            1.0,
            &font,
        )
        .expect("stroke image");

        let fill_ink = count_ink_pixels(&fill);
        let stroke_ink = count_ink_pixels(&stroke);
        assert_ne!(fill.pixels(), stroke.pixels());
        assert!(
            fill_ink.abs_diff(stroke_ink) > 300,
            "fill/stroke ink coverage should differ; fill={fill_ink}, stroke={stroke_ink}"
        );
    }

    #[test]
    fn stroke_path_uses_miter_join_for_acute_apexes() {
        let font = Font::try_from_bytes(include_bytes!(
            "../../../../apps/desktop-demo/assets/Roboto-Regular.ttf"
        ) as &[u8])
        .expect("font");
        let fill_style = TextStyle::default();
        let stroke_width = 12.0;
        let stroke_style = TextStyle {
            span_style: SpanStyle {
                draw_style: Some(TextDrawStyle::Stroke {
                    width: stroke_width,
                }),
                ..Default::default()
            },
            ..Default::default()
        };
        let rect = Rect {
            x: 0.0,
            y: 0.0,
            width: 180.0,
            height: 140.0,
        };

        let fill = rasterize_text_to_image_with_font(
            "A",
            rect,
            &fill_style,
            Color::WHITE,
            110.0,
            1.0,
            &font,
        )
        .expect("fill image");
        let stroke = rasterize_text_to_image_with_font(
            "A",
            rect,
            &stroke_style,
            Color::WHITE,
            110.0,
            1.0,
            &font,
        )
        .expect("stroke image");

        let fill_top = top_ink_row(&fill).expect("fill top row");
        let stroke_top = top_ink_row(&stroke).expect("stroke top row");
        let reference_dilation =
            rasterize_reference_dilation_stroke("A", rect, 110.0, stroke_width, &font);
        let reference_top = top_ink_row(&reference_dilation).expect("reference top row");
        let extra_extension = fill_top.saturating_sub(stroke_top) as f32;
        let half_stroke = stroke_width * 0.5;
        assert!(
            extra_extension >= half_stroke - 0.25,
            "stroke apex should extend by roughly at least half stroke width; fill_top={fill_top}, stroke_top={stroke_top}, half_stroke={half_stroke:.2}"
        );
        assert!(
            stroke.pixels() != reference_dilation.pixels(),
            "path stroke should diverge from mask-dilation reference output"
        );
        assert!(
            stroke_top <= reference_top,
            "miter stroke should keep acute apex at least as extended as mask-dilation reference; stroke_top={stroke_top}, reference_top={reference_top}"
        );
    }

    #[test]
    fn shadow_blur_radius_changes_spread_for_shared_raster_path() {
        let font = Font::try_from_bytes(include_bytes!(
            "../../../../apps/desktop-demo/assets/Roboto-Regular.ttf"
        ) as &[u8])
        .expect("font");
        let base_shadow = Shadow {
            color: Color(0.0, 0.0, 0.0, 0.9),
            offset: Point::new(5.5, 4.25),
            blur_radius: 0.0,
        };
        let hard_shadow_style = TextStyle {
            span_style: SpanStyle {
                shadow: Some(base_shadow),
                ..Default::default()
            },
            ..Default::default()
        };
        let blurred_shadow_style = TextStyle {
            span_style: SpanStyle {
                shadow: Some(Shadow {
                    blur_radius: 9.0,
                    ..base_shadow
                }),
                ..Default::default()
            },
            ..Default::default()
        };
        let rect = Rect {
            x: 0.0,
            y: 0.0,
            width: 320.0,
            height: 120.0,
        };

        let hard_shadow = rasterize_text_to_image_with_font(
            "Shared shadow",
            rect,
            &hard_shadow_style,
            Color::TRANSPARENT,
            48.0,
            1.0,
            &font,
        )
        .expect("hard shadow image");
        let blurred_shadow = rasterize_text_to_image_with_font(
            "Shared shadow",
            rect,
            &blurred_shadow_style,
            Color::TRANSPARENT,
            48.0,
            1.0,
            &font,
        )
        .expect("blurred shadow image");

        let hard_ink = count_ink_pixels(&hard_shadow);
        let blurred_ink = count_ink_pixels(&blurred_shadow);
        assert_ne!(
            hard_shadow.pixels(),
            blurred_shadow.pixels(),
            "blur radius should change rasterized shadow output"
        );
        assert!(
            blurred_ink > hard_ink,
            "blurred shadow should spread to more pixels; hard={hard_ink}, blurred={blurred_ink}"
        );
    }

    #[test]
    fn text_motion_changes_fractional_shadow_sampling() {
        let font = Font::try_from_bytes(include_bytes!(
            "../../../../apps/desktop-demo/assets/Roboto-Regular.ttf"
        ) as &[u8])
        .expect("font");
        let base_shadow = Shadow {
            color: Color(0.0, 0.0, 0.0, 0.9),
            offset: Point::new(3.35, 2.65),
            blur_radius: 6.0,
        };
        let static_style = TextStyle {
            span_style: SpanStyle {
                shadow: Some(base_shadow),
                ..Default::default()
            },
            paragraph_style: cranpose_ui::text::ParagraphStyle {
                text_motion: Some(TextMotion::Static),
                ..Default::default()
            },
        };
        let animated_style = TextStyle {
            span_style: SpanStyle {
                shadow: Some(base_shadow),
                ..Default::default()
            },
            paragraph_style: cranpose_ui::text::ParagraphStyle {
                text_motion: Some(TextMotion::Animated),
                ..Default::default()
            },
        };
        let rect = Rect {
            x: 11.35,
            y: 7.65,
            width: 280.0,
            height: 120.0,
        };

        let static_image = rasterize_text_to_image_with_font(
            "Motion shadow",
            rect,
            &static_style,
            Color::TRANSPARENT,
            42.0,
            1.0,
            &font,
        )
        .expect("static image");
        let animated_image = rasterize_text_to_image_with_font(
            "Motion shadow",
            rect,
            &animated_style,
            Color::TRANSPARENT,
            42.0,
            1.0,
            &font,
        )
        .expect("animated image");

        assert_ne!(
            static_image.pixels(),
            animated_image.pixels(),
            "TextMotion::Static should quantize shadow placement while Animated keeps fractional sampling"
        );
    }

    #[test]
    fn static_text_motion_aligns_glyph_positions_to_pixel_grid() {
        let font = Font::try_from_bytes(include_bytes!(
            "../../../../apps/desktop-demo/assets/Roboto-Regular.ttf"
        ) as &[u8])
        .expect("font");
        let scale = Scale::uniform(17.0);

        let base_glyph = font
            .layout("A", scale, point(0.0, 13.37))
            .next()
            .expect("glyph");
        let static_aligned = align_glyph_for_text_motion(base_glyph, true);
        let static_position = static_aligned.position();
        assert!(
            (static_position.x - static_position.x.round()).abs() < f32::EPSILON,
            "static text should snap glyph x to pixel grid"
        );
        assert!(
            (static_position.y - static_position.y.round()).abs() < f32::EPSILON,
            "static text should snap glyph y to pixel grid"
        );

        let animated_source = font
            .layout("A", scale, point(0.0, 13.37))
            .next()
            .expect("glyph");
        let animated_aligned = align_glyph_for_text_motion(animated_source, false);
        let animated_position = animated_aligned.position();
        assert!(
            (animated_position.y - 13.37).abs() < 1e-3,
            "animated text should preserve fractional glyph position"
        );
    }
}
