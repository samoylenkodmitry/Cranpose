use lru::LruCache;
use once_cell::sync::Lazy;
use rusttype::{point, Font, Scale};
use std::borrow::Borrow;
use std::hash::{Hash, Hasher};
use std::num::NonZeroUsize;
use std::sync::{Arc, Mutex};

use compose_ui::{Brush, TextMeasurer, TextMetrics};
use compose_ui_graphics::{Color, Rect};

use crate::scene::{Scene, TextDraw};
use crate::style::point_in_resolved_rounded_rect;

const TEXT_SIZE: f32 = 24.0;
static FONT: Lazy<Font<'static>> = Lazy::new(|| {
    let f = Font::try_from_bytes(include_bytes!(
        "../../../../apps/desktop-demo/assets/Roboto-Light.ttf"
    ) as &[u8])
    .expect("font");
    f
});

pub struct CachedRusttypeTextMeasurer {
    cache: Mutex<TextMetricsCache>,
}

#[derive(Clone)]
struct TextKey {
    text: Arc<str>,
}

impl PartialEq for TextKey {
    fn eq(&self, other: &Self) -> bool {
        Arc::ptr_eq(&self.text, &other.text) || *self.text == *other.text
    }
}

impl Eq for TextKey {}

impl Hash for TextKey {
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.text.hash(state);
    }
}

impl Borrow<str> for TextKey {
    fn borrow(&self) -> &str {
        &self.text
    }
}

struct TextMetricsCache {
    map: LruCache<TextKey, TextMetrics>,
}

impl TextMetricsCache {
    fn new(capacity: usize) -> Self {
        let capped = capacity.max(1);
        let size = NonZeroUsize::new(capped).unwrap();
        Self {
            map: LruCache::new(size),
        }
    }

    fn get_or_measure<F>(&mut self, text: &str, measure: F) -> TextMetrics
    where
        F: FnOnce(&str) -> TextMetrics,
    {
        if let Some(metrics) = self.map.get(text).copied() {
            return metrics;
        }
        let key = TextKey {
            text: Arc::from(text),
        };
        let metrics = measure(text);
        self.map.put(key, metrics);
        metrics
    }
}

impl CachedRusttypeTextMeasurer {
    pub(crate) fn new(capacity: usize) -> Self {
        Self {
            cache: Mutex::new(TextMetricsCache::new(capacity)),
        }
    }
}

#[derive(Clone, Copy)]
struct ClipBounds {
    min_x: i32,
    min_y: i32,
    max_x: i32,
    max_y: i32,
}

fn clip_rect_to_bounds(
    rect: Rect,
    clip: Option<Rect>,
    width: u32,
    height: u32,
) -> Option<ClipBounds> {
    let mut min_x = rect.x;
    let mut min_y = rect.y;
    let mut max_x = rect.x + rect.width;
    let mut max_y = rect.y + rect.height;

    if let Some(clip_rect) = clip {
        min_x = min_x.max(clip_rect.x);
        min_y = min_y.max(clip_rect.y);
        max_x = max_x.min(clip_rect.x + clip_rect.width);
        max_y = max_y.min(clip_rect.y + clip_rect.height);
    }

    min_x = min_x.max(0.0);
    min_y = min_y.max(0.0);
    max_x = max_x.min(width as f32);
    max_y = max_y.min(height as f32);

    if max_x <= min_x || max_y <= min_y {
        return None;
    }

    let min_x = min_x.floor() as i32;
    let min_y = min_y.floor() as i32;
    let max_x = max_x.ceil() as i32;
    let max_y = max_y.ceil() as i32;

    let min_x = min_x.clamp(0, width as i32);
    let min_y = min_y.clamp(0, height as i32);
    let max_x = max_x.clamp(0, width as i32);
    let max_y = max_y.clamp(0, height as i32);

    if min_x >= max_x || min_y >= max_y {
        return None;
    }

    Some(ClipBounds {
        min_x,
        min_y,
        max_x,
        max_y,
    })
}

fn clip_bounds_from_clip(clip: Option<Rect>, width: u32, height: u32) -> Option<ClipBounds> {
    clip.and_then(|rect| clip_rect_to_bounds(rect, None, width, height))
}

impl TextMeasurer for CachedRusttypeTextMeasurer {
    fn measure(&self, text: &str) -> TextMetrics {
        self.cache
            .lock()
            .expect("text metrics cache poisoned")
            .get_or_measure(text, measure_text_impl)
    }

    fn get_offset_for_position(&self, text: &str, x: f32, _y: f32) -> usize {
        if text.is_empty() {
            return 0;
        }

        let scale = Scale::uniform(TEXT_SIZE);
        let font = &*FONT;
        let v_metrics = font.v_metrics(scale);
        let origin = point(0.0, v_metrics.ascent);

        // Find the glyph whose center is closest to x
        let mut best_offset = 0;
        let mut best_distance = f32::INFINITY;
        let mut current_byte_offset = 0;

        for c in text.chars() {
            // Get glyph position for this character
            let prefix = &text[..current_byte_offset];
            let mut glyph_x = 0.0f32;

            // Measure prefix width to get glyph start position
            for glyph in font.layout(prefix, scale, origin) {
                if let Some(bb) = glyph.pixel_bounding_box() {
                    glyph_x = bb.max.x as f32;
                }
            }

            // Get width of current character to find center
            let char_str = &text[current_byte_offset..current_byte_offset + c.len_utf8()];
            let char_width = {
                let mut w = 0.0f32;
                for glyph in font.layout(char_str, scale, origin) {
                    if let Some(bb) = glyph.pixel_bounding_box() {
                        w = (bb.max.x - bb.min.x) as f32;
                    }
                }
                w.max(TEXT_SIZE * 0.5) // Minimum width for whitespace
            };

            // Check distance to left edge of character
            let left_dist = (x - glyph_x).abs();
            if left_dist < best_distance {
                best_distance = left_dist;
                best_offset = current_byte_offset;
            }

            // Check distance to right edge (= after this character)
            let right_x = glyph_x + char_width;
            let right_dist = (x - right_x).abs();
            if right_dist < best_distance {
                best_distance = right_dist;
                best_offset = current_byte_offset + c.len_utf8();
            }

            current_byte_offset += c.len_utf8();
        }

        // Also check end of text
        let total_width = measure_text_impl(text).width;
        let end_dist = (x - total_width).abs();
        if end_dist < best_distance {
            best_offset = text.len();
        }

        best_offset
    }

    fn get_cursor_x_for_offset(&self, text: &str, offset: usize) -> f32 {
        let clamped_offset = offset.min(text.len());
        if clamped_offset == 0 {
            return 0.0;
        }

        // Measure text up to offset
        let prefix = &text[..clamped_offset];
        measure_text_impl(prefix).width
    }

    fn layout(&self, text: &str) -> compose_ui::text_layout_result::TextLayoutResult {
        use compose_ui::text_layout_result::{LineLayout, TextLayoutResult};

        let scale = Scale::uniform(TEXT_SIZE);
        let font = &*FONT;
        let v_metrics = font.v_metrics(scale);
        let line_height = (v_metrics.ascent - v_metrics.descent).ceil();
        let _origin = point(0.0, v_metrics.ascent);

        let mut glyph_x_positions = Vec::new();
        let mut char_to_byte = Vec::new();
        let mut lines = Vec::new();
        let mut current_x = 0.0f32;
        let mut line_start = 0;
        let mut y = 0.0f32;

        // Build glyph positions
        for (byte_offset, c) in text.char_indices() {
            glyph_x_positions.push(current_x);
            char_to_byte.push(byte_offset);

            if c == '\n' {
                lines.push(LineLayout {
                    start_offset: line_start,
                    end_offset: byte_offset,
                    y,
                    height: line_height,
                });
                line_start = byte_offset + 1;
                y += line_height;
                current_x = 0.0;
            } else {
                // Get glyph advance
                let glyph = font.glyph(c).scaled(scale);
                current_x += glyph.h_metrics().advance_width;
            }
        }

        // Add end position
        glyph_x_positions.push(current_x);
        char_to_byte.push(text.len());

        // Add final line
        lines.push(LineLayout {
            start_offset: line_start,
            end_offset: text.len(),
            y,
            height: line_height,
        });

        let metrics = measure_text_impl(text);
        TextLayoutResult::new(
            metrics.width,
            metrics.height,
            line_height,
            glyph_x_positions,
            char_to_byte,
            lines,
            text,
        )
    }
}

fn measure_text_impl(text: &str) -> TextMetrics {
    let scale = Scale::uniform(TEXT_SIZE);
    let font = &*FONT;
    let v_metrics = font.v_metrics(scale);
    let line_height = (v_metrics.ascent - v_metrics.descent).ceil();

    // Split by newlines for multiline support
    let lines: Vec<&str> = text.split('\n').collect();
    let line_count = lines.len().max(1);

    // Measure max width across all lines
    let mut max_width: f32 = 0.0;
    for line in &lines {
        let origin = point(0.0, v_metrics.ascent);
        let mut min_x: f32 = f32::INFINITY;
        let mut line_max_x: f32 = 0.0;
        let mut glyph_count = 0_u32;

        for glyph in font.layout(line, scale, origin) {
            glyph_count += 1;
            if let Some(bb) = glyph.pixel_bounding_box() {
                min_x = min_x.min(bb.min.x as f32);
                line_max_x = line_max_x.max(bb.max.x as f32);
            }
        }

        let line_width = if glyph_count == 0 {
            0.0
        } else if min_x.is_infinite() {
            line_max_x
        } else {
            (line_max_x - min_x).max(0.0)
        };
        max_width = max_width.max(line_width);
    }

    TextMetrics {
        width: max_width,
        height: line_count as f32 * line_height,
        line_height,
        line_count,
    }
}

pub fn draw_scene(frame: &mut [u8], width: u32, height: u32, scene: &Scene) {
    for chunk in frame.chunks_exact_mut(4) {
        chunk.copy_from_slice(&[18, 18, 24, 255]);
    }

    let mut shapes = scene.shapes.clone();
    shapes.sort_by(|a, b| a.z_index.cmp(&b.z_index));
    for shape in shapes {
        draw_shape(frame, width, height, shape);
    }

    let mut texts = scene.texts.clone();
    texts.sort_by(|a, b| a.z_index.cmp(&b.z_index));
    for text in texts {
        draw_text(frame, width, height, text);
    }
}

fn draw_shape(frame: &mut [u8], width: u32, height: u32, draw: crate::scene::DrawShape) {
    let clip_bounds = match clip_rect_to_bounds(draw.rect, draw.clip, width, height) {
        Some(bounds) => bounds,
        None => return,
    };
    let Rect {
        width: rect_width,
        height: rect_height,
        ..
    } = draw.rect;
    let resolved_shape = draw
        .shape
        .map(|shape| shape.resolve(rect_width, rect_height));
    for py in clip_bounds.min_y..clip_bounds.max_y {
        if py < 0 || py >= height as i32 {
            continue;
        }
        for px in clip_bounds.min_x..clip_bounds.max_x {
            if px < 0 || px >= width as i32 {
                continue;
            }
            let center_x = px as f32 + 0.5;
            let center_y = py as f32 + 0.5;
            if let Some(ref radii) = resolved_shape {
                if !point_in_resolved_rounded_rect(center_x, center_y, draw.rect, radii) {
                    continue;
                }
            }
            let sample = sample_brush(&draw.brush, draw.rect, center_x, center_y);
            let alpha = sample[3];
            if alpha <= 0.0 {
                continue;
            }
            let idx = ((py as u32 * width + px as u32) * 4) as usize;
            let existing = &mut frame[idx..idx + 4];
            let dst_r = existing[0] as f32 / 255.0;
            let dst_g = existing[1] as f32 / 255.0;
            let dst_b = existing[2] as f32 / 255.0;
            let dst_a = existing[3] as f32 / 255.0;
            let out_r = sample[0] * alpha + dst_r * (1.0 - alpha);
            let out_g = sample[1] * alpha + dst_g * (1.0 - alpha);
            let out_b = sample[2] * alpha + dst_b * (1.0 - alpha);
            let out_a = alpha + dst_a * (1.0 - alpha);
            existing[0] = (out_r.clamp(0.0, 1.0) * 255.0).round() as u8;
            existing[1] = (out_g.clamp(0.0, 1.0) * 255.0).round() as u8;
            existing[2] = (out_b.clamp(0.0, 1.0) * 255.0).round() as u8;
            existing[3] = (out_a.clamp(0.0, 1.0) * 255.0).round() as u8;
        }
    }
}

fn draw_text(frame: &mut [u8], width: u32, height: u32, draw: TextDraw) {
    let color = color_to_rgba(draw.color);
    let text_scale = draw.scale.max(0.0);
    if text_scale == 0.0 {
        return;
    }
    let clip_bounds = clip_bounds_from_clip(draw.clip, width, height);
    if draw.clip.is_some() && clip_bounds.is_none() {
        return;
    }
    let clip_limits =
        clip_bounds.map(|bounds| (bounds.min_x, bounds.min_y, bounds.max_x, bounds.max_y));
    let scale = Scale::uniform(TEXT_SIZE * text_scale);
    let font = &*FONT;
    let v_metrics = font.v_metrics(scale);
    let offset = point(draw.rect.x, draw.rect.y + v_metrics.ascent);
    for glyph in font.layout(&draw.text, scale, offset) {
        if let Some(bb) = glyph.pixel_bounding_box() {
            if let Some((min_x, min_y, max_x, max_y)) = clip_limits {
                if bb.max.x <= min_x || bb.min.x >= max_x || bb.max.y <= min_y || bb.min.y >= max_y
                {
                    continue;
                }
            }
            glyph.draw(|gx, gy, value| {
                let px = bb.min.x + gx as i32;
                let py = bb.min.y + gy as i32;
                if let Some((min_x, min_y, max_x, max_y)) = clip_limits {
                    if px < min_x || px >= max_x || py < min_y || py >= max_y {
                        return;
                    }
                }
                if px < 0 || py < 0 || px as u32 >= width || py as u32 >= height {
                    return;
                }
                let idx = ((py as u32 * width + px as u32) * 4) as usize;
                let alpha = value;
                let existing = &mut frame[idx..idx + 4];
                for i in 0..3 {
                    let dst = existing[i] as f32 / 255.0;
                    let blended = (color[i] * alpha) + dst * (1.0 - alpha);
                    existing[i] = (blended.clamp(0.0, 1.0) * 255.0).round() as u8;
                }
                let dst_alpha = existing[3] as f32 / 255.0;
                let out_alpha = alpha + dst_alpha * (1.0 - alpha);
                existing[3] = (out_alpha.clamp(0.0, 1.0) * 255.0).round() as u8;
            });
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
        Brush::LinearGradient(colors) => {
            let t = if rect.height.abs() <= f32::EPSILON {
                0.0
            } else {
                ((y - rect.y) / rect.height).clamp(0.0, 1.0)
            };
            color_to_rgba(interpolate_colors(colors, t))
        }
        Brush::RadialGradient {
            colors,
            center,
            radius,
        } => {
            let cx = rect.x + center.x;
            let cy = rect.y + center.y;
            let radius = (*radius).max(f32::EPSILON);
            let dx = x - cx;
            let dy = y - cy;
            let distance = (dx * dx + dy * dy).sqrt();
            let t = (distance / radius).clamp(0.0, 1.0);
            color_to_rgba(interpolate_colors(colors, t))
        }
    }
}

fn interpolate_colors(colors: &[Color], t: f32) -> Color {
    if colors.is_empty() {
        return Color(0.0, 0.0, 0.0, 0.0);
    }
    if colors.len() == 1 {
        return colors[0];
    }
    let clamped = t.clamp(0.0, 1.0);
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
