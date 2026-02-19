use cranpose_ui::text::{Shadow, TextDrawStyle, TextMotion, TextStyle};
use cranpose_ui_graphics::{Color, ImageBitmap, Rect, TileMode};
use rusttype::{point, Font, Scale};

use crate::Brush;

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
}
