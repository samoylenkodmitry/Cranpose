use lru::LruCache;
use once_cell::sync::Lazy;
use rusttype::{point, Font, Scale};
use std::borrow::Borrow;
use std::hash::{Hash, Hasher};
use std::num::NonZeroUsize;
use std::rc::Rc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Mutex;

use cranpose_render_common::software_text_raster::rasterize_text_to_image_with_font;
use cranpose_render_common::text_hyphenation::choose_auto_hyphen_break as choose_shared_auto_hyphen_break;
use cranpose_ui::text::TextMotion;
use cranpose_ui::{Brush, TextMeasurer, TextMetrics};
use cranpose_ui_graphics::{BlendMode, Color, ColorFilter, Rect, TileMode};

use crate::scene::{ImageDraw, Scene, TextDraw};
use crate::style::point_in_resolved_rounded_rect;

static FONT: Lazy<Font<'static>> = Lazy::new(|| {
    let f = Font::try_from_bytes(include_bytes!(
        "../../../../apps/desktop-demo/assets/Roboto-Light.ttf"
    ) as &[u8])
    .expect("font");
    f
});
static REPORTED_UNSUPPORTED_PIXELS_BLEND_MODES: AtomicBool = AtomicBool::new(false);

fn is_blend_mode_supported(mode: BlendMode) -> bool {
    matches!(mode, BlendMode::SrcOver | BlendMode::DstOut)
}

pub struct CachedRusttypeTextMeasurer {
    cache: Mutex<TextMetricsCache>,
}

#[derive(Clone)]
struct TextKey {
    text: Rc<str>,
    font_size_bits: u32,
    style_hash: u64,
}

impl PartialEq for TextKey {
    fn eq(&self, other: &Self) -> bool {
        (Rc::ptr_eq(&self.text, &other.text) || *self.text == *other.text)
            && self.font_size_bits == other.font_size_bits
            && self.style_hash == other.style_hash
    }
}

impl Eq for TextKey {}

impl Hash for TextKey {
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.text.hash(state);
        self.font_size_bits.hash(state);
        self.style_hash.hash(state);
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

    fn get_or_measure<F>(
        &mut self,
        text: &str,
        font_size: f32,
        style_hash: u64,
        measure: F,
    ) -> TextMetrics
    where
        F: FnOnce(&str, f32) -> TextMetrics,
    {
        // Note: Borrow<str> lookup doesn't work well with composite key.
        // We construct key for lookup.
        let key = TextKey {
            text: Rc::from(text),
            font_size_bits: font_size.to_bits(),
            style_hash,
        };

        if let Some(metrics) = self.map.get(&key).copied() {
            return metrics;
        }

        let metrics = measure(text, font_size);
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

// Helper to resolve font size from style
fn resolve_font_size(style: &cranpose_ui::text::TextStyle) -> f32 {
    style.resolve_font_size(14.0)
}

fn resolve_line_height(style: &cranpose_ui::text::TextStyle, font_size: f32) -> f32 {
    style.resolve_line_height(14.0, font_size)
}

fn resolve_letter_spacing(style: &cranpose_ui::text::TextStyle, font_size: f32) -> f32 {
    let _ = font_size;
    style.resolve_letter_spacing(14.0)
}

impl TextMeasurer for CachedRusttypeTextMeasurer {
    fn measure(&self, text: &str, style: &cranpose_ui::text::TextStyle) -> TextMetrics {
        let font_size = resolve_font_size(style);
        let style_hash = style.measurement_hash();
        self.cache
            .lock()
            .expect("text metrics cache poisoned")
            .get_or_measure(text, font_size, style_hash, |value, size| {
                measure_text_impl(value, style, size)
            })
    }

    fn get_offset_for_position(
        &self,
        text: &str,
        style: &cranpose_ui::text::TextStyle,
        x: f32,
        _y: f32,
    ) -> usize {
        if text.is_empty() {
            return 0;
        }

        let font_size = resolve_font_size(style);
        let scale = Scale::uniform(font_size);
        let font = &*FONT;
        let v_metrics = font.v_metrics(scale);
        let origin = point(0.0, v_metrics.ascent);
        let line_height = resolve_line_height(style, (v_metrics.ascent - v_metrics.descent).ceil());

        let line_index = (_y / line_height).floor().max(0.0) as usize;
        let lines: Vec<&str> = text.split('\n').collect();
        let target_line = line_index.min(lines.len().saturating_sub(1));

        let mut line_start_byte = 0;
        for line in lines.iter().take(target_line) {
            line_start_byte += line.len() + 1;
        }

        let line_text = lines.get(target_line).unwrap_or(&"");
        if line_text.is_empty() {
            return line_start_byte;
        }

        // Find the glyph whose center is closest to x
        let mut best_offset = 0;
        let mut best_distance = f32::INFINITY;
        let mut current_byte_offset = 0;

        for c in line_text.chars() {
            // Get glyph position for this character
            let prefix = &line_text[..current_byte_offset];
            let mut glyph_x = 0.0f32;

            // Measure prefix width to get glyph start position
            for glyph in font.layout(prefix, scale, origin) {
                if let Some(bb) = glyph.pixel_bounding_box() {
                    glyph_x = bb.max.x as f32;
                }
            }

            // Get width of current character to find center
            let char_str = &line_text[current_byte_offset..current_byte_offset + c.len_utf8()];
            let char_width = {
                let mut w = 0.0f32;
                for glyph in font.layout(char_str, scale, origin) {
                    if let Some(bb) = glyph.pixel_bounding_box() {
                        w = (bb.max.x - bb.min.x) as f32;
                    }
                }
                w.max(font_size * 0.5) // Minimum width for whitespace
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
        let total_width = measure_text_impl(line_text, style, font_size).width;
        let end_dist = (x - total_width).abs();
        if end_dist < best_distance {
            best_offset = line_text.len();
        }

        line_start_byte + best_offset.min(line_text.len())
    }

    fn get_cursor_x_for_offset(
        &self,
        text: &str,
        style: &cranpose_ui::text::TextStyle,
        offset: usize,
    ) -> f32 {
        let clamped_offset = offset.min(text.len());
        if clamped_offset == 0 {
            return 0.0;
        }

        let font_size = resolve_font_size(style);
        // Measure text up to offset
        let prefix = &text[..clamped_offset];
        measure_text_impl(prefix, style, font_size).width
    }

    fn layout(
        &self,
        text: &str,
        style: &cranpose_ui::text::TextStyle,
    ) -> cranpose_ui::text_layout_result::TextLayoutResult {
        use cranpose_ui::text_layout_result::{LineLayout, TextLayoutResult};

        let font_size = resolve_font_size(style);
        let scale = Scale::uniform(font_size);
        let font = &*FONT;
        let v_metrics = font.v_metrics(scale);
        let natural_line_height = (v_metrics.ascent - v_metrics.descent).ceil();
        let line_height = resolve_line_height(style, natural_line_height);
        let letter_spacing = resolve_letter_spacing(style, font_size);
        let _origin = point(0.0, v_metrics.ascent);

        let mut glyph_x_positions = Vec::new();
        let mut char_to_byte = Vec::new();
        let mut lines = Vec::new();
        let mut current_x = 0.0f32;
        let mut line_start = 0;
        let mut y = 0.0f32;

        // Build glyph positions
        let mut iter = text.char_indices().peekable();
        while let Some((byte_offset, c)) = iter.next() {
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
                if let Some((_, next)) = iter.peek() {
                    if *next != '\n' {
                        current_x += letter_spacing;
                    }
                }
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

        let metrics = measure_text_impl(text, style, font_size);
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

    fn choose_auto_hyphen_break(
        &self,
        line: &str,
        style: &cranpose_ui::text::TextStyle,
        segment_start_char: usize,
        measured_break_char: usize,
    ) -> Option<usize> {
        choose_shared_auto_hyphen_break(line, style, segment_start_char, measured_break_char)
    }
}

fn measure_text_impl(
    text: &str,
    style: &cranpose_ui::text::TextStyle,
    font_size: f32,
) -> TextMetrics {
    let scale = Scale::uniform(font_size);
    let font = &*FONT;
    let v_metrics = font.v_metrics(scale);
    let natural_line_height = (v_metrics.ascent - v_metrics.descent).ceil();
    let line_height = resolve_line_height(style, natural_line_height);
    let letter_spacing = resolve_letter_spacing(style, font_size);

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
        let char_spacing = (line.chars().count().saturating_sub(1) as f32) * letter_spacing;
        let line_width = (line_width + char_spacing).max(0.0);
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

    let mut ordered_items =
        Vec::with_capacity(scene.shapes.len() + scene.images.len() + scene.texts.len());
    for (index, shape) in scene.shapes.iter().enumerate() {
        ordered_items.push((shape.z_index, RenderItem::Shape(index)));
    }
    for (index, image) in scene.images.iter().enumerate() {
        ordered_items.push((image.z_index, RenderItem::Image(index)));
    }
    for (index, text) in scene.texts.iter().enumerate() {
        ordered_items.push((text.z_index, RenderItem::Text(index)));
    }
    ordered_items.sort_by_key(|(z, _)| *z);

    for (_, item) in ordered_items {
        match item {
            RenderItem::Shape(index) => draw_shape(frame, width, height, &scene.shapes[index]),
            RenderItem::Image(index) => draw_image(frame, width, height, &scene.images[index]),
            RenderItem::Text(index) => draw_text(frame, width, height, &scene.texts[index]),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum RenderItem {
    Shape(usize),
    Image(usize),
    Text(usize),
}

fn draw_shape(frame: &mut [u8], width: u32, height: u32, draw: &crate::scene::DrawShape) {
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
            blend_pixel(&mut frame[idx..idx + 4], sample, draw.blend_mode);
        }
    }
}

fn draw_image(frame: &mut [u8], width: u32, height: u32, draw: &ImageDraw) {
    if draw.alpha <= 0.0 || draw.rect.width <= 0.0 || draw.rect.height <= 0.0 {
        return;
    }

    let clip_bounds = match clip_rect_to_bounds(draw.rect, draw.clip, width, height) {
        Some(bounds) => bounds,
        None => return,
    };

    let img_width = draw.image.width();
    let img_height = draw.image.height();
    if img_width == 0 || img_height == 0 {
        return;
    }
    let src_pixels = draw.image.pixels();

    // Source region: either a sub-rect or the full image
    let (sr_x, sr_y, sr_w, sr_h) = if let Some(sr) = draw.src_rect {
        (sr.x, sr.y, sr.width, sr.height)
    } else {
        (0.0, 0.0, img_width as f32, img_height as f32)
    };

    for py in clip_bounds.min_y..clip_bounds.max_y {
        for px in clip_bounds.min_x..clip_bounds.max_x {
            let sample_x = px as f32 + 0.5;
            let sample_y = py as f32 + 0.5;
            let u = ((sample_x - draw.rect.x) / draw.rect.width).clamp(0.0, 1.0);
            let v = ((sample_y - draw.rect.y) / draw.rect.height).clamp(0.0, 1.0);

            let src_x = ((sr_x + u * sr_w).floor() as i32).clamp(0, img_width as i32 - 1);
            let src_y = ((sr_y + v * sr_h).floor() as i32).clamp(0, img_height as i32 - 1);
            let src_idx = ((src_y as u32 * img_width + src_x as u32) * 4) as usize;

            let mut sample = [
                src_pixels[src_idx] as f32 / 255.0,
                src_pixels[src_idx + 1] as f32 / 255.0,
                src_pixels[src_idx + 2] as f32 / 255.0,
                src_pixels[src_idx + 3] as f32 / 255.0,
            ];

            if let Some(filter) = draw.color_filter {
                sample = apply_color_filter(sample, filter);
            }

            sample[3] *= draw.alpha.clamp(0.0, 1.0);
            if sample[3] <= 0.0 {
                continue;
            }

            let dst_idx = ((py as u32 * width + px as u32) * 4) as usize;
            blend_pixel(&mut frame[dst_idx..dst_idx + 4], sample, draw.blend_mode);
        }
    }
}

fn draw_text(frame: &mut [u8], width: u32, height: u32, draw: &TextDraw) {
    let text_scale = draw.scale.max(0.0);
    if text_scale == 0.0 {
        return;
    }

    let static_text_motion = draw
        .text_style
        .paragraph_style
        .text_motion
        .unwrap_or(TextMotion::Static)
        == TextMotion::Static;

    let raster_rect = if static_text_motion {
        Rect {
            x: draw.rect.x.round(),
            y: draw.rect.y.round(),
            width: draw.rect.width,
            height: draw.rect.height,
        }
    } else {
        draw.rect
    };

    let Some(image) = rasterize_text_to_image_with_font(
        draw.text.as_ref(),
        raster_rect,
        &draw.text_style,
        draw.color,
        draw.font_size,
        text_scale,
        &FONT,
    ) else {
        return;
    };

    blit_rasterized_text_image(frame, width, height, raster_rect, draw.clip, &image);
}

fn blit_rasterized_text_image(
    frame: &mut [u8],
    width: u32,
    height: u32,
    rect: Rect,
    clip: Option<Rect>,
    image: &cranpose_ui_graphics::ImageBitmap,
) {
    if rect.width <= 0.0 || rect.height <= 0.0 {
        return;
    }
    let clip_bounds = match clip_rect_to_bounds(rect, clip, width, height) {
        Some(bounds) => bounds,
        None => return,
    };

    let img_width = image.width();
    let img_height = image.height();
    if img_width == 0 || img_height == 0 {
        return;
    }
    let src_pixels = image.pixels();

    for py in clip_bounds.min_y..clip_bounds.max_y {
        for px in clip_bounds.min_x..clip_bounds.max_x {
            let sample_x = px as f32 + 0.5;
            let sample_y = py as f32 + 0.5;
            let u = ((sample_x - rect.x) / rect.width).clamp(0.0, 1.0);
            let v = ((sample_y - rect.y) / rect.height).clamp(0.0, 1.0);

            let src_x = (u * (img_width - 1) as f32).round() as u32;
            let src_y = (v * (img_height - 1) as f32).round() as u32;
            let src_idx = ((src_y * img_width + src_x) * 4) as usize;
            let src = [
                src_pixels[src_idx] as f32 / 255.0,
                src_pixels[src_idx + 1] as f32 / 255.0,
                src_pixels[src_idx + 2] as f32 / 255.0,
                src_pixels[src_idx + 3] as f32 / 255.0,
            ];
            if src[3] <= 0.0 {
                continue;
            }

            let dst_idx = ((py as u32 * width + px as u32) * 4) as usize;
            blend_pixel(&mut frame[dst_idx..dst_idx + 4], src, BlendMode::SrcOver);
        }
    }
}

fn blend_pixel(dst: &mut [u8], src: [f32; 4], blend_mode: BlendMode) {
    let resolved_blend_mode = if is_blend_mode_supported(blend_mode) {
        blend_mode
    } else {
        if !REPORTED_UNSUPPORTED_PIXELS_BLEND_MODES.swap(true, Ordering::Relaxed) {
            log::warn!(
                "Pixels renderer currently supports BlendMode::SrcOver and BlendMode::DstOut; falling back to SrcOver for unsupported modes"
            );
        }
        BlendMode::SrcOver
    };

    let src_alpha = src[3].clamp(0.0, 1.0);
    if src_alpha <= 0.0 {
        return;
    }
    let dst_r = dst[0] as f32 / 255.0;
    let dst_g = dst[1] as f32 / 255.0;
    let dst_b = dst[2] as f32 / 255.0;
    let dst_a = dst[3] as f32 / 255.0;

    let (out_r, out_g, out_b, out_a) = match resolved_blend_mode {
        BlendMode::DstOut => {
            let keep = 1.0 - src_alpha;
            (dst_r * keep, dst_g * keep, dst_b * keep, dst_a * keep)
        }
        BlendMode::SrcOver => (
            src[0].clamp(0.0, 1.0) * src_alpha + dst_r * (1.0 - src_alpha),
            src[1].clamp(0.0, 1.0) * src_alpha + dst_g * (1.0 - src_alpha),
            src[2].clamp(0.0, 1.0) * src_alpha + dst_b * (1.0 - src_alpha),
            src_alpha + dst_a * (1.0 - src_alpha),
        ),
        _ => unreachable!("unsupported blend modes are resolved before blending"),
    };

    dst[0] = (out_r.clamp(0.0, 1.0) * 255.0).round() as u8;
    dst[1] = (out_g.clamp(0.0, 1.0) * 255.0).round() as u8;
    dst[2] = (out_b.clamp(0.0, 1.0) * 255.0).round() as u8;
    dst[3] = (out_a.clamp(0.0, 1.0) * 255.0).round() as u8;
}

fn apply_color_filter(sample: [f32; 4], filter: ColorFilter) -> [f32; 4] {
    filter.apply_rgba(sample)
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
            // Map [-PI, PI] to [0, 1]
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

    fn count_non_background_pixels(frame: &[u8], width: u32, height: u32) -> usize {
        count_non_background_pixels_in_band(frame, width, 0, height)
    }

    fn render_single_text_frame(
        style: cranpose_ui::TextStyle,
        color: Color,
        x: f32,
    ) -> (u32, u32, Vec<u8>) {
        let mut scene = Scene::new();
        scene.push_text(
            11,
            Rect {
                x,
                y: 16.0,
                width: 320.0,
                height: 90.0,
            },
            Rc::from("MMMMMMMM"),
            color,
            style,
            64.0,
            1.0,
            cranpose_ui::TextLayoutOptions::default(),
            None,
        );

        let width = 360;
        let height = 140;
        let mut frame = vec![0u8; (width * height * 4) as usize];
        draw_scene(&mut frame, width, height, &scene);
        (width, height, frame)
    }

    fn average_ink_rgb(
        frame: &[u8],
        width: u32,
        x_min: u32,
        x_max: u32,
        y_min: u32,
        y_max: u32,
    ) -> Option<[f32; 3]> {
        let mut sum_r = 0.0f32;
        let mut sum_g = 0.0f32;
        let mut sum_b = 0.0f32;
        let mut count = 0usize;

        for y in y_min..y_max {
            for x in x_min..x_max {
                let idx = ((y * width + x) * 4) as usize;
                let px = &frame[idx..idx + 4];
                if px == [18, 18, 24, 255] {
                    continue;
                }
                sum_r += px[0] as f32 / 255.0;
                sum_g += px[1] as f32 / 255.0;
                sum_b += px[2] as f32 / 255.0;
                count += 1;
            }
        }

        if count == 0 {
            return None;
        }
        Some([
            sum_r / count as f32,
            sum_g / count as f32,
            sum_b / count as f32,
        ])
    }

    fn count_non_background_pixels_in_band(
        frame: &[u8],
        width: u32,
        y_min_inclusive: u32,
        y_max_exclusive: u32,
    ) -> usize {
        let mut count = 0usize;
        for y in y_min_inclusive..y_max_exclusive {
            for x in 0..width {
                let idx = ((y * width + x) * 4) as usize;
                let px = &frame[idx..idx + 4];
                if px != [18, 18, 24, 255] {
                    count += 1;
                }
            }
        }
        count
    }

    #[test]
    fn blend_mode_support_matrix_is_explicit() {
        assert!(is_blend_mode_supported(BlendMode::SrcOver));
        assert!(is_blend_mode_supported(BlendMode::DstOut));
        assert!(!is_blend_mode_supported(BlendMode::Clear));
        assert!(!is_blend_mode_supported(BlendMode::Multiply));
    }

    #[test]
    fn mirror_tile_mode_reflects_second_interval() {
        assert_eq!(normalize_gradient_t(1.25, TileMode::Mirror), Some(0.75));
        assert_eq!(normalize_gradient_t(1.75, TileMode::Mirror), Some(0.25));
    }

    #[test]
    fn multiline_text_renders_second_line_pixels() {
        let mut scene = Scene::new();
        scene.push_text(
            1,
            Rect {
                x: 8.0,
                y: 8.0,
                width: 180.0,
                height: 80.0,
            },
            Rc::from("Dynamic\nModifiers"),
            Color::WHITE,
            cranpose_ui::TextStyle::default(),
            14.0,
            1.0,
            cranpose_ui::TextLayoutOptions::default(),
            None,
        );

        let width = 220;
        let height = 100;
        let mut frame = vec![0u8; (width * height * 4) as usize];
        draw_scene(&mut frame, width, height, &scene);

        let first_line_ink = count_non_background_pixels_in_band(&frame, width, 8, 32);
        let second_line_ink = count_non_background_pixels_in_band(&frame, width, 32, 64);
        assert!(first_line_ink > 50, "expected first line to render");
        assert!(
            second_line_ink > 30,
            "expected second line ink, got {second_line_ink}"
        );
    }

    #[test]
    fn text_clip_bounds_prevent_drawing_outside_scroll_window() {
        let mut scene = Scene::new();
        scene.push_text(
            2,
            Rect {
                x: 8.0,
                y: 40.0,
                width: 180.0,
                height: 24.0,
            },
            Rc::from("Clipped Text"),
            Color::WHITE,
            cranpose_ui::TextStyle::default(),
            14.0,
            1.0,
            cranpose_ui::TextLayoutOptions::default(),
            Some(Rect {
                x: 0.0,
                y: 0.0,
                width: 220.0,
                height: 20.0,
            }),
        );

        let width = 220;
        let height = 100;
        let mut frame = vec![0u8; (width * height * 4) as usize];
        draw_scene(&mut frame, width, height, &scene);

        let total_ink = count_non_background_pixels_in_band(&frame, width, 0, height);
        assert_eq!(
            total_ink, 0,
            "text should be fully clipped but rendered {total_ink} ink pixels"
        );
    }

    #[test]
    fn gradient_brush_contract_requires_visible_color_transition() {
        let style = cranpose_ui::TextStyle {
            span_style: cranpose_ui::SpanStyle {
                brush: Some(Brush::linear_gradient_range(
                    vec![Color(1.0, 0.0, 0.0, 1.0), Color(0.0, 0.0, 1.0, 1.0)],
                    cranpose_ui_graphics::Point::new(0.0, 0.0),
                    cranpose_ui_graphics::Point::new(320.0, 0.0),
                )),
                ..Default::default()
            },
            ..Default::default()
        };

        let (width, _height, frame) = render_single_text_frame(style, Color::WHITE, 12.0);
        let left = average_ink_rgb(&frame, width, 20, 150, 20, 120).expect("left ink");
        let right = average_ink_rgb(&frame, width, 200, 340, 20, 120).expect("right ink");

        assert!(
            left[0] > left[2] * 1.15,
            "left side should be red-dominant for horizontal gradient, got {left:?}"
        );
        assert!(
            right[2] > right[0] * 1.15,
            "right side should be blue-dominant for horizontal gradient, got {right:?}"
        );
    }

    #[test]
    fn draw_style_stroke_contract_changes_raster_output() {
        let fill_style = cranpose_ui::TextStyle::default();
        let stroke_style = cranpose_ui::TextStyle {
            span_style: cranpose_ui::SpanStyle {
                draw_style: Some(cranpose_ui::text::TextDrawStyle::Stroke { width: 6.0 }),
                ..Default::default()
            },
            ..Default::default()
        };

        let (width, height, fill_frame) = render_single_text_frame(fill_style, Color::WHITE, 12.0);
        let (_, _, stroke_frame) = render_single_text_frame(stroke_style, Color::WHITE, 12.0);
        let fill_ink = count_non_background_pixels(&fill_frame, width, height);
        let stroke_ink = count_non_background_pixels(&stroke_frame, width, height);

        assert_ne!(
            fill_frame, stroke_frame,
            "Fill and Stroke text must not rasterize identically"
        );
        assert!(
            fill_ink.abs_diff(stroke_ink) > 250,
            "Fill/Stroke ink coverage should differ; fill={fill_ink}, stroke={stroke_ink}"
        );
    }

    #[test]
    fn shadow_blur_radius_contract_changes_raster_output() {
        let base_shadow = cranpose_ui::text::Shadow {
            color: Color(0.0, 0.0, 0.0, 0.85),
            offset: cranpose_ui_graphics::Point::new(6.0, 4.0),
            blur_radius: 0.0,
        };
        let zero_blur_style = cranpose_ui::TextStyle {
            span_style: cranpose_ui::SpanStyle {
                shadow: Some(base_shadow),
                ..Default::default()
            },
            ..Default::default()
        };
        let blurred_style = cranpose_ui::TextStyle {
            span_style: cranpose_ui::SpanStyle {
                shadow: Some(cranpose_ui::text::Shadow {
                    blur_radius: 10.0,
                    ..base_shadow
                }),
                ..Default::default()
            },
            ..Default::default()
        };

        let (_, _, zero_frame) = render_single_text_frame(zero_blur_style, Color::WHITE, 12.0);
        let (_, _, blur_frame) = render_single_text_frame(blurred_style, Color::WHITE, 12.0);

        assert_ne!(
            zero_frame, blur_frame,
            "Changing shadow blur radius must change rendered output"
        );
    }

    #[test]
    fn text_motion_contract_changes_raster_output() {
        let static_style = cranpose_ui::TextStyle {
            paragraph_style: cranpose_ui::ParagraphStyle {
                text_motion: Some(cranpose_ui::text::TextMotion::Static),
                ..Default::default()
            },
            ..Default::default()
        };
        let animated_style = cranpose_ui::TextStyle {
            paragraph_style: cranpose_ui::ParagraphStyle {
                text_motion: Some(cranpose_ui::text::TextMotion::Animated),
                ..Default::default()
            },
            ..Default::default()
        };

        let (_, _, static_frame) = render_single_text_frame(static_style, Color::WHITE, 12.35);
        let (_, _, animated_frame) = render_single_text_frame(animated_style, Color::WHITE, 12.35);

        assert_ne!(
            static_frame, animated_frame,
            "TextMotion::Static and TextMotion::Animated should not rasterize identically"
        );
    }
}
