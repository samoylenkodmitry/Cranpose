use std::sync::{Arc, OnceLock, RwLock};

use cranpose_render_common::Brush;
use cranpose_ui::text::{FontFamily, FontStyle, Shadow, TextDrawStyle, TextMotion, TextStyle};
use cranpose_ui_graphics::{Color, ImageBitmap, Rect, TileMode};
use rusttype::{point, Font, Scale};
use ttf_parser::{name_id, Face};

struct RasterFontFace {
    font: Arc<Font<'static>>,
    weight: u16,
    italic: bool,
    family_name: Option<String>,
}

static RASTER_FONTS: OnceLock<RwLock<Vec<RasterFontFace>>> = OnceLock::new();

pub(crate) fn configure_raster_fonts(fonts: &[&[u8]]) {
    let parsed: Vec<RasterFontFace> = fonts
        .iter()
        .filter_map(|font_data| {
            let metadata = Face::parse(font_data, 0).ok();
            let font = Font::try_from_vec(font_data.to_vec())?;
            Some(RasterFontFace {
                font: Arc::new(font),
                weight: metadata
                    .as_ref()
                    .map(|face| face.weight().to_number())
                    .unwrap_or(400),
                italic: metadata.as_ref().is_some_and(Face::is_italic),
                family_name: metadata.as_ref().and_then(extract_family_name),
            })
        })
        .collect();
    assert!(
        !parsed.is_empty(),
        "rasterized styled text requires at least one valid configured font"
    );

    let storage = RASTER_FONTS.get_or_init(|| RwLock::new(Vec::new()));
    let mut configured = storage
        .write()
        .expect("rasterized styled text font storage poisoned");
    *configured = parsed;
}

fn extract_family_name(face: &Face<'_>) -> Option<String> {
    face.names()
        .into_iter()
        .filter(|name| {
            name.name_id == name_id::TYPOGRAPHIC_FAMILY || name.name_id == name_id::FAMILY
        })
        .find_map(|name| {
            name.to_string().and_then(|value| {
                let trimmed = value.trim();
                if trimmed.is_empty() {
                    None
                } else {
                    Some(trimmed.to_ascii_lowercase())
                }
            })
        })
}

fn choose_raster_face_index(style: &TextStyle, fonts: &[RasterFontFace]) -> Option<usize> {
    if fonts.is_empty() {
        return None;
    }
    if fonts.len() == 1 {
        return Some(0);
    }

    let requested_weight = style
        .span_style
        .font_weight
        .map(|weight| weight.0)
        .unwrap_or(400);
    let requested_italic = style.span_style.font_style == Some(FontStyle::Italic);
    let requested_family = style.span_style.font_family.as_ref();
    let requested_named_family = match requested_family {
        Some(FontFamily::Named(name)) => Some(name.to_ascii_lowercase()),
        _ => None,
    };

    let mut best_index = 0usize;
    let mut best_score = i32::MAX;
    for (index, face) in fonts.iter().enumerate() {
        let family_penalty = match requested_family {
            Some(FontFamily::Named(_)) => match requested_named_family.as_ref() {
                Some(requested) => match face.family_name.as_ref() {
                    Some(actual)
                        if actual == requested
                            || actual.contains(requested)
                            || requested.contains(actual) =>
                    {
                        0
                    }
                    _ => 30_000,
                },
                None => 30_000,
            },
            Some(FontFamily::Serif) => {
                if face
                    .family_name
                    .as_ref()
                    .is_some_and(|name| name.contains("serif") && !name.contains("sans"))
                {
                    0
                } else {
                    5_000
                }
            }
            Some(FontFamily::Monospace) => {
                if face
                    .family_name
                    .as_ref()
                    .is_some_and(|name| name.contains("mono"))
                {
                    0
                } else {
                    5_000
                }
            }
            Some(FontFamily::Cursive) => {
                if face
                    .family_name
                    .as_ref()
                    .is_some_and(|name| name.contains("cursive") || name.contains("script"))
                {
                    0
                } else {
                    5_000
                }
            }
            Some(FontFamily::Fantasy) => {
                if face
                    .family_name
                    .as_ref()
                    .is_some_and(|name| name.contains("fantasy"))
                {
                    0
                } else {
                    5_000
                }
            }
            Some(FontFamily::Default | FontFamily::SansSerif) | None => 0,
        };
        let style_penalty = if requested_italic == face.italic {
            0
        } else {
            2_000
        };
        let weight_penalty = (face.weight as i32 - requested_weight as i32).abs();
        let score = family_penalty + style_penalty + weight_penalty;
        if score < best_score {
            best_score = score;
            best_index = index;
        }
    }

    Some(best_index)
}

fn raster_font(style: &TextStyle) -> Option<Arc<Font<'static>>> {
    let storage = RASTER_FONTS.get()?;
    let fonts = storage.read().ok()?;
    let best_index = choose_raster_face_index(style, &fonts)?;
    Some(Arc::clone(&fonts[best_index].font))
}

pub(crate) fn requires_rasterized_glyph_path(style: &TextStyle) -> bool {
    let uses_non_solid_brush = matches!(
        style.span_style.brush,
        Some(
            Brush::LinearGradient { .. }
                | Brush::RadialGradient { .. }
                | Brush::SweepGradient { .. }
        )
    );
    let uses_stroke = matches!(
        style.span_style.draw_style,
        Some(TextDrawStyle::Stroke { width }) if width.is_finite() && width > 0.0
    );
    uses_non_solid_brush || uses_stroke
}

pub(crate) fn rasterize_text_to_image(
    text: &str,
    rect: Rect,
    style: &TextStyle,
    fallback_color: Color,
    font_size: f32,
    scale: f32,
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
    let stroke_radius = match style.span_style.draw_style.unwrap_or(TextDrawStyle::Fill) {
        TextDrawStyle::Fill => 0,
        TextDrawStyle::Stroke { width } => {
            if width.is_finite() && width > 0.0 {
                ((width * scale) * 0.5).ceil() as i32
            } else {
                0
            }
        }
    };
    let stroke_offsets = (stroke_radius > 0).then(|| build_stroke_offsets(stroke_radius));
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
    let font = raster_font(style)?;
    let v_metrics = font.v_metrics(scale_px);
    let line_height = style
        .resolve_line_height(14.0, (v_metrics.ascent - v_metrics.descent).ceil())
        .max(1.0);

    for (line_idx, line) in text.split('\n').enumerate() {
        let baseline_y = v_metrics.ascent + line_idx as f32 * line_height + origin_y;
        let offset = point(origin_x, baseline_y);

        for glyph in font.layout(line, scale_px, offset) {
            if let Some(bb) = glyph.pixel_bounding_box() {
                if let Some(shadow) = shadow {
                    draw_shadow_fill_glyph(&mut canvas, width, height, &glyph, bb, shadow, scale);
                }

                if let Some(stroke_offsets) = stroke_offsets.as_deref() {
                    draw_stroke_glyph(
                        &mut canvas,
                        width,
                        height,
                        &glyph,
                        bb,
                        stroke_offsets,
                        brush,
                        brush_alpha_multiplier,
                        rect,
                    );
                    continue;
                }

                glyph.draw(|gx, gy, value| {
                    let px = bb.min.x + gx as i32;
                    let py = bb.min.y + gy as i32;
                    if px < 0 || py < 0 || px as u32 >= width || py as u32 >= height {
                        return;
                    }
                    let sample = sample_brush(
                        brush,
                        rect,
                        rect.x + px as f32 + 0.5,
                        rect.y + py as f32 + 0.5,
                    );
                    let alpha = value * sample[3] * brush_alpha_multiplier;
                    if alpha <= 0.0 {
                        return;
                    }
                    let idx = (py as u32 * width + px as u32) as usize;
                    blend_src_over(
                        &mut canvas[idx],
                        [sample[0], sample[1], sample[2], alpha.clamp(0.0, 1.0)],
                    );
                });
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

fn draw_shadow_fill_glyph(
    canvas: &mut [[f32; 4]],
    width: u32,
    height: u32,
    glyph: &rusttype::PositionedGlyph<'_>,
    bb: rusttype::Rect<i32>,
    shadow: Shadow,
    text_scale: f32,
) {
    let mask_width = (bb.max.x - bb.min.x).max(0) as usize;
    let mask_height = (bb.max.y - bb.min.y).max(0) as usize;
    if mask_width == 0 || mask_height == 0 {
        return;
    }

    let mut mask = vec![0.0f32; mask_width * mask_height];
    glyph.draw(|gx, gy, value| {
        let idx = gy as usize * mask_width + gx as usize;
        mask[idx] = value;
    });

    let shadow_dx = shadow.offset.x * text_scale;
    let shadow_dy = shadow.offset.y * text_scale;
    let blur_radius = (shadow.blur_radius * text_scale).max(0.0);
    let blur_margin = if blur_radius > 0.0 {
        (blur_radius * 3.0).ceil() as i32
    } else {
        0
    };
    let padded_width = mask_width + (blur_margin as usize) * 2;
    let padded_height = mask_height + (blur_margin as usize) * 2;
    let mut padded_mask = vec![0.0f32; padded_width * padded_height];

    for y in 0..mask_height {
        let src_offset = y * mask_width;
        let dst_offset = (y + blur_margin as usize) * padded_width + blur_margin as usize;
        padded_mask[dst_offset..dst_offset + mask_width]
            .copy_from_slice(&mask[src_offset..src_offset + mask_width]);
    }

    let mask = if blur_radius > 0.0 {
        gaussian_blur_alpha(&padded_mask, padded_width, padded_height, blur_radius)
    } else {
        padded_mask
    };

    let shadow_rgba = color_to_rgba(shadow.color);
    let shadow_origin_x = bb.min.x - blur_margin;
    let shadow_origin_y = bb.min.y - blur_margin;

    for y in 0..padded_height {
        for x in 0..padded_width {
            let alpha = mask[y * padded_width + x] * shadow_rgba[3];
            if alpha <= 0.0 {
                continue;
            }

            let px = (shadow_origin_x as f32 + x as f32 + shadow_dx).round() as i32;
            let py = (shadow_origin_y as f32 + y as f32 + shadow_dy).round() as i32;
            if px < 0 || py < 0 || px >= width as i32 || py >= height as i32 {
                continue;
            }

            let idx = (py as u32 * width + px as u32) as usize;
            blend_src_over(
                &mut canvas[idx],
                [
                    shadow_rgba[0],
                    shadow_rgba[1],
                    shadow_rgba[2],
                    alpha.clamp(0.0, 1.0),
                ],
            );
        }
    }
}

fn gaussian_blur_alpha(src: &[f32], width: usize, height: usize, radius: f32) -> Vec<f32> {
    let kernel = gaussian_kernel_1d(radius);
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

fn gaussian_kernel_1d(radius: f32) -> Vec<f32> {
    let half = radius.ceil() as i32;
    if half <= 0 {
        return vec![1.0];
    }

    let sigma = (radius * 0.5).max(0.5);
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

fn build_stroke_offsets(radius: i32) -> Vec<(i32, i32)> {
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

#[allow(clippy::too_many_arguments)]
fn draw_stroke_glyph(
    canvas: &mut [[f32; 4]],
    width: u32,
    height: u32,
    glyph: &rusttype::PositionedGlyph<'_>,
    bb: rusttype::Rect<i32>,
    stroke_offsets: &[(i32, i32)],
    brush: &Brush,
    brush_alpha_multiplier: f32,
    brush_rect: Rect,
) {
    let mask_width = (bb.max.x - bb.min.x).max(0) as usize;
    let mask_height = (bb.max.y - bb.min.y).max(0) as usize;
    if mask_width == 0 || mask_height == 0 {
        return;
    }

    let mut mask = vec![0.0f32; mask_width * mask_height];
    glyph.draw(|gx, gy, value| {
        let idx = gy as usize * mask_width + gx as usize;
        mask[idx] = value;
    });

    let radius = stroke_offsets
        .iter()
        .map(|(dx, dy)| dx.abs().max(dy.abs()))
        .max()
        .unwrap_or(0);
    let min_x = bb.min.x - radius;
    let min_y = bb.min.y - radius;
    let max_x = bb.max.x + radius;
    let max_y = bb.max.y + radius;
    let mask_width_i32 = mask_width as i32;
    let mask_height_i32 = mask_height as i32;

    for py in min_y..max_y {
        if py < 0 || py >= height as i32 {
            continue;
        }

        for px in min_x..max_x {
            if px < 0 || px >= width as i32 {
                continue;
            }

            let ox = px - bb.min.x;
            let oy = py - bb.min.y;
            let base_alpha = if ox >= 0 && oy >= 0 && ox < mask_width_i32 && oy < mask_height_i32 {
                mask[oy as usize * mask_width + ox as usize]
            } else {
                0.0
            };

            let mut dilated_alpha = 0.0f32;
            for (dx, dy) in stroke_offsets {
                let sx = ox + dx;
                let sy = oy + dy;
                if sx < 0 || sy < 0 || sx >= mask_width_i32 || sy >= mask_height_i32 {
                    continue;
                }
                let sample = mask[sy as usize * mask_width + sx as usize];
                if sample > dilated_alpha {
                    dilated_alpha = sample;
                    if dilated_alpha >= 0.999 {
                        break;
                    }
                }
            }

            let outline_alpha = (dilated_alpha - base_alpha).max(0.0);
            if outline_alpha <= 0.0 {
                continue;
            }
            let sample = sample_brush(
                brush,
                brush_rect,
                brush_rect.x + px as f32 + 0.5,
                brush_rect.y + py as f32 + 0.5,
            );
            let alpha = outline_alpha * sample[3] * brush_alpha_multiplier;
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
    use cranpose_ui::text::{FontWeight, SpanStyle};
    use cranpose_ui_graphics::Point;

    fn init_test_fonts() {
        configure_raster_fonts(&[
            include_bytes!("../../../../apps/desktop-demo/assets/Roboto-Light.ttf") as &[u8],
            include_bytes!("../../../../apps/desktop-demo/assets/Roboto-Regular.ttf") as &[u8],
        ]);
    }

    fn selected_weight(style: &TextStyle) -> u16 {
        let storage = RASTER_FONTS.get().expect("configured test fonts");
        let fonts = storage
            .read()
            .expect("rasterized styled text font storage poisoned");
        let index = choose_raster_face_index(style, &fonts).expect("selected font index");
        fonts[index].weight
    }

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

    #[test]
    fn requires_rasterized_glyph_path_for_gradient_or_stroke() {
        init_test_fonts();
        let fill_style = TextStyle::default();
        assert!(!requires_rasterized_glyph_path(&fill_style));

        let gradient_style = TextStyle {
            span_style: SpanStyle {
                brush: Some(Brush::linear_gradient_range(
                    vec![Color::RED, Color::BLUE],
                    Point::new(0.0, 0.0),
                    Point::new(100.0, 0.0),
                )),
                ..Default::default()
            },
            ..Default::default()
        };
        assert!(requires_rasterized_glyph_path(&gradient_style));

        let stroke_style = TextStyle {
            span_style: SpanStyle {
                draw_style: Some(TextDrawStyle::Stroke { width: 4.0 }),
                ..Default::default()
            },
            ..Default::default()
        };
        assert!(requires_rasterized_glyph_path(&stroke_style));
    }

    #[test]
    fn raster_font_selection_prefers_closest_weight() {
        init_test_fonts();
        let default_style = TextStyle::default();
        let light_style = TextStyle {
            span_style: SpanStyle {
                font_weight: Some(FontWeight::LIGHT),
                ..Default::default()
            },
            ..Default::default()
        };
        let medium_style = TextStyle {
            span_style: SpanStyle {
                font_weight: Some(FontWeight::MEDIUM),
                ..Default::default()
            },
            ..Default::default()
        };

        let default_weight = selected_weight(&default_style);
        let light_weight = selected_weight(&light_style);
        let medium_weight = selected_weight(&medium_style);

        assert!(
            default_weight.abs_diff(400) <= default_weight.abs_diff(300),
            "default style should prefer regular-ish face, got {default_weight}"
        );
        assert!(
            light_weight.abs_diff(300) <= light_weight.abs_diff(400),
            "light style should prefer light-ish face, got {light_weight}"
        );
        assert!(
            medium_weight.abs_diff(400) <= medium_weight.abs_diff(300),
            "medium style should stay closest to regular-ish face with configured fonts, got {medium_weight}"
        );
    }

    #[test]
    fn rasterized_gradient_text_shows_color_transition() {
        init_test_fonts();
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

        let image = rasterize_text_to_image(
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
        init_test_fonts();
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

        let fill = rasterize_text_to_image("MMMMMMMM", rect, &fill_style, Color::WHITE, 48.0, 1.0)
            .expect("fill image");
        let stroke =
            rasterize_text_to_image("MMMMMMMM", rect, &stroke_style, Color::WHITE, 48.0, 1.0)
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
    fn configure_raster_fonts_replaces_previously_configured_faces() {
        configure_raster_fonts(&[include_bytes!(
            "../../../../apps/desktop-demo/assets/Roboto-Light.ttf"
        ) as &[u8]]);
        let style = TextStyle {
            span_style: SpanStyle {
                font_weight: Some(FontWeight::MEDIUM),
                ..Default::default()
            },
            ..Default::default()
        };
        let light_only_weight = selected_weight(&style);

        configure_raster_fonts(&[include_bytes!(
            "../../../../apps/desktop-demo/assets/Roboto-Regular.ttf"
        ) as &[u8]]);
        let regular_only_weight = selected_weight(&style);

        assert!(
            light_only_weight.abs_diff(300) <= light_only_weight.abs_diff(400),
            "single light-face config should select light-ish weight, got {light_only_weight}"
        );
        assert!(
            regular_only_weight.abs_diff(400) <= regular_only_weight.abs_diff(300),
            "single regular-face config should select regular-ish weight, got {regular_only_weight}"
        );
    }

    #[test]
    fn rasterized_radial_stroke_bidi_text_shows_visible_color_variation() {
        init_test_fonts();
        let style = TextStyle {
            span_style: SpanStyle {
                brush: Some(Brush::radial_gradient(
                    vec![Color(0.35, 0.95, 1.0, 1.0), Color(1.0, 0.7, 0.45, 1.0)],
                    Point::new(180.0, 48.0),
                    210.0,
                )),
                alpha: Some(0.9),
                draw_style: Some(TextDrawStyle::Stroke { width: 2.2 }),
                ..Default::default()
            },
            ..Default::default()
        };
        let image = rasterize_text_to_image(
            "Gradient שלום stroke",
            Rect {
                x: 0.0,
                y: 0.0,
                width: 360.0,
                height: 96.0,
            },
            &style,
            Color::WHITE,
            32.0,
            1.0,
        )
        .expect("rasterized image");

        let mut min_green = 1.0f32;
        let mut max_green = 0.0f32;
        let mut min_blue = 1.0f32;
        let mut max_blue = 0.0f32;
        let mut ink = 0usize;
        let pixels = image.pixels();
        for px in pixels.chunks_exact(4) {
            if px[3] == 0 {
                continue;
            }
            let green = px[1] as f32 / 255.0;
            let blue = px[2] as f32 / 255.0;
            min_green = min_green.min(green);
            max_green = max_green.max(green);
            min_blue = min_blue.min(blue);
            max_blue = max_blue.max(blue);
            ink += 1;
        }

        assert!(
            ink > 600,
            "expected visible ink for bidi stroke sample, got {ink}"
        );
        let green_span = max_green - min_green;
        let blue_span = max_blue - min_blue;
        assert!(
            green_span.max(blue_span) > 0.12,
            "radial brush stroke should vary channels, green_span={green_span:.3}, blue_span={blue_span:.3}"
        );
    }
}
