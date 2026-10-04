//! Shared CPU [`TextMeasurer`] used by the software rasterizer backends
//! (pixels, vulkan). Both backends draw text by rasterizing glyphs directly
//! into a pixel buffer rather than through a GPU glyph atlas, so they share
//! one font-backed measurer plus its fallback-metrics path for when no font
//! is installed.

use std::{
    hash::{Hash, Hasher},
    sync::{Mutex, MutexGuard, PoisonError},
};

use cranpose_core::collections::pass_aged::PassAgedCache;
use cranpose_ui::{TextMeasurer, TextMetrics, text_layout_result::TextLayoutResult};

use crate::{
    software_text_raster::{
        SoftwareTextFontSet, annotated_cursor_x_for_offset, annotated_offset_for_position,
        layout_annotated_text_with_font_set, measure_annotated_text_with_font_set,
        visit_annotated_line_boxes,
    },
    text_cache_key::{TextCacheKey, TextProbe},
    text_hyphenation::HyphenationDictionaryStore,
};

/// Renderer-owned text resources: the resolved font used for software
/// rasterization and measurement, if any.
#[derive(Clone)]
pub struct SoftwareTextResources {
    fonts: SoftwareTextFontSet,
}

impl SoftwareTextResources {
    pub fn default_font() -> Self {
        Self {
            fonts: SoftwareTextFontSet::from_fonts_or_default(&[]),
        }
    }

    pub fn fonts(&self) -> &SoftwareTextFontSet {
        &self.fonts
    }
}

impl Default for SoftwareTextResources {
    fn default() -> Self {
        Self::default_font()
    }
}

pub fn fallback_char_width(font_size: f32) -> f32 {
    font_size.max(1.0) * 0.55
}

pub fn fallback_line_height(font_size: f32) -> f32 {
    font_size.max(1.0) * 1.2
}

pub fn fallback_text_metrics(text: &str, font_size: f32) -> TextMetrics {
    let line_height = fallback_line_height(font_size);
    let mut line_count = 0usize;
    let mut max_chars = 0usize;
    for line in text.split('\n') {
        line_count += 1;
        max_chars = max_chars.max(line.chars().count());
    }
    let line_count = line_count.max(1);
    TextMetrics {
        width: max_chars as f32 * fallback_char_width(font_size),
        height: line_count as f32 * line_height,
        line_height,
        line_count,
    }
}

pub fn fallback_cursor_x_for_byte_offset(text: &str, byte_offset: usize, font_size: f32) -> f32 {
    let clamped = byte_offset.min(text.len());
    let char_count = if clamped == text.len() {
        text.chars().count()
    } else {
        text.char_indices()
            .take_while(|(index, _)| *index < clamped)
            .count()
    };
    char_count as f32 * fallback_char_width(font_size)
}

pub struct CachedFontTextMeasurer {
    text_resources: SoftwareTextResources,
    cache: Mutex<TextMetricsCache>,
    hyphenation: HyphenationDictionaryStore,
}

/// A measurement's parameters besides its text: the font size's bits and
/// the style hash.
type TextMetricsParams = (u32, u64);

struct TextMetricsCache {
    map: PassAgedCache<TextCacheKey<TextMetricsParams>, TextMetrics>,
}

impl TextMetricsCache {
    fn new(capacity: usize) -> Self {
        Self {
            map: PassAgedCache::with_capacity_at_least_one(capacity),
        }
    }

    /// The cached metrics of `text`, measured by `measure` on a miss. A hit
    /// borrows `text`; only a miss copies it into the key it stores.
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
        let probe = TextProbe::new(text, (font_size.to_bits(), style_hash));
        if let Some(metrics) = self.map.get(probe.key()).copied() {
            return metrics;
        }

        let metrics = measure(text, font_size);
        self.map.push(probe.to_owned_key(), metrics);
        metrics
    }
}

impl CachedFontTextMeasurer {
    pub fn with_text_resources(text_resources: SoftwareTextResources, capacity: usize) -> Self {
        Self {
            text_resources,
            cache: Mutex::new(TextMetricsCache::new(capacity)),
            hyphenation: HyphenationDictionaryStore::new(),
        }
    }

    fn lock_cache(&self) -> MutexGuard<'_, TextMetricsCache> {
        self.cache.lock().unwrap_or_else(PoisonError::into_inner)
    }
}

fn resolve_font_size(style: &cranpose_ui::text::TextStyle) -> f32 {
    style.resolve_font_size(14.0)
}

impl TextMeasurer for CachedFontTextMeasurer {
    fn begin_layout_pass(&self) {
        self.lock_cache().map.begin_pass(|_| {});
    }

    fn glyph_line_box(&self, style: &cranpose_ui::text::TextStyle) -> Option<(f32, f32)> {
        let font = self.text_resources.fonts().resolve(style)?;
        Some(crate::software_text_raster::font_glyph_line_box(
            style, font,
        ))
    }

    fn first_baseline(&self, style: &cranpose_ui::text::TextStyle) -> Option<f32> {
        Some(self.line_box(style)?.baseline)
    }

    fn line_box(&self, style: &cranpose_ui::text::TextStyle) -> Option<cranpose_ui::text::LineBox> {
        let font = self.text_resources.fonts().resolve(style)?;
        Some(crate::software_text_raster::font_line_box(
            style,
            font,
            resolve_font_size(style),
        ))
    }

    fn visit_line_boxes(
        &self,
        text: &cranpose_ui::text::AnnotatedString,
        style: &cranpose_ui::text::TextStyle,
        visit: &mut dyn FnMut(cranpose_ui::text::LineBox),
    ) -> Option<()> {
        visit_annotated_line_boxes(text, style, self.text_resources.fonts(), visit)
    }

    fn measure(
        &self,
        text: &cranpose_ui::text::AnnotatedString,
        style: &cranpose_ui::text::TextStyle,
    ) -> TextMetrics {
        let text_str = text.text.as_str();
        let font_size = resolve_font_size(style);
        let mut style_hash = style.measurement_hash();
        if !text.span_styles.is_empty() {
            let mut hasher = cranpose_ui_graphics::FxHasher::default();
            style_hash.hash(&mut hasher);
            text.span_styles_hash().hash(&mut hasher);
            style_hash = hasher.finish();
        }
        self.lock_cache()
            .get_or_measure(text_str, font_size, style_hash, |_, size| {
                measure_annotated_text_with_font_set(text, style, size, self.text_resources.fonts())
            })
    }

    fn get_offset_for_position(
        &self,
        text: &cranpose_ui::text::AnnotatedString,
        style: &cranpose_ui::text::TextStyle,
        x: f32,
        y: f32,
    ) -> usize {
        annotated_offset_for_position(text, style, x, y, self.text_resources.fonts())
    }

    fn get_cursor_x_for_offset(
        &self,
        text: &cranpose_ui::text::AnnotatedString,
        style: &cranpose_ui::text::TextStyle,
        offset: usize,
    ) -> f32 {
        annotated_cursor_x_for_offset(text, style, offset, self.text_resources.fonts())
    }

    fn layout(
        &self,
        text: &cranpose_ui::text::AnnotatedString,
        style: &cranpose_ui::text::TextStyle,
    ) -> TextLayoutResult {
        layout_annotated_text_with_font_set(text, style, self.text_resources.fonts())
    }

    fn choose_auto_hyphen_break(
        &self,
        line: &str,
        style: &cranpose_ui::text::TextStyle,
        segment_start_char: usize,
        measured_break_char: usize,
    ) -> Option<usize> {
        self.hyphenation.choose_auto_hyphen_break(
            line,
            style,
            segment_start_char,
            measured_break_char,
        )
    }
}

#[cfg(test)]
#[path = "tests/text_measure_tests.rs"]
mod tests;
