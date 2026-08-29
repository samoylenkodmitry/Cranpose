//! Shared CPU [`TextMeasurer`] used by the software rasterizer backends
//! (pixels, vulkan). Both backends draw text by rasterizing glyphs directly
//! into a pixel buffer rather than through a GPU glyph atlas, so they share
//! one font-backed measurer plus its fallback-metrics path for when no font
//! is installed.

use std::{
    borrow::Borrow,
    hash::{Hash, Hasher},
    rc::Rc,
    sync::{Mutex, MutexGuard},
};

use cranpose_ui::{TextMeasurer, TextMetrics, text_layout_result::TextLayoutResult};

use crate::{
    bounded_lru_cache::BoundedLruCache,
    software_text_raster::{
        SoftwareTextFont, SoftwareTextFontSet, cursor_x_for_offset_with_font,
        layout_text_with_font, measure_text_with_font, text_offset_for_position_with_font,
    },
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
    map: BoundedLruCache<TextKey, TextMetrics>,
}

impl TextMetricsCache {
    fn new(capacity: usize) -> Self {
        Self {
            map: BoundedLruCache::with_capacity_at_least_one(capacity),
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

impl CachedFontTextMeasurer {
    pub fn with_text_resources(text_resources: SoftwareTextResources, capacity: usize) -> Self {
        Self {
            text_resources,
            cache: Mutex::new(TextMetricsCache::new(capacity)),
            hyphenation: HyphenationDictionaryStore::new(),
        }
    }

    fn lock_cache(&self) -> MutexGuard<'_, TextMetricsCache> {
        self.cache
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }
}

fn resolve_font_size(style: &cranpose_ui::text::TextStyle) -> f32 {
    style.resolve_font_size(14.0)
}

impl TextMeasurer for CachedFontTextMeasurer {
    fn measure(
        &self,
        text: &cranpose_ui::text::AnnotatedString,
        style: &cranpose_ui::text::TextStyle,
    ) -> TextMetrics {
        let text_str = text.text.as_str();
        let font_size = resolve_font_size(style);
        let style_hash = style.measurement_hash();
        self.lock_cache()
            .get_or_measure(text_str, font_size, style_hash, |value, size| {
                measure_text_impl(
                    value,
                    style,
                    size,
                    self.text_resources.fonts().resolve(style),
                )
            })
    }

    fn get_offset_for_position(
        &self,
        text: &cranpose_ui::text::AnnotatedString,
        style: &cranpose_ui::text::TextStyle,
        x: f32,
        _y: f32,
    ) -> usize {
        let text = text.text.as_str();
        if text.is_empty() {
            return 0;
        }

        let Some(font) = self.text_resources.fonts().resolve(style) else {
            let font_size = resolve_font_size(style);
            return TextLayoutResult::monospaced(
                text,
                fallback_char_width(font_size),
                fallback_line_height(font_size),
            )
            .get_offset_for_x(x);
        };

        text_offset_for_position_with_font(text, style, x, _y, font)
    }

    fn get_cursor_x_for_offset(
        &self,
        text: &cranpose_ui::text::AnnotatedString,
        style: &cranpose_ui::text::TextStyle,
        offset: usize,
    ) -> f32 {
        let text = text.text.as_str();
        let clamped_offset = offset.min(text.len());
        if clamped_offset == 0 {
            return 0.0;
        }

        let Some(font) = self.text_resources.fonts().resolve(style) else {
            return fallback_cursor_x_for_byte_offset(
                text,
                clamped_offset,
                resolve_font_size(style),
            );
        };

        cursor_x_for_offset_with_font(text, style, clamped_offset, font)
    }

    fn layout(
        &self,
        text: &cranpose_ui::text::AnnotatedString,
        style: &cranpose_ui::text::TextStyle,
    ) -> cranpose_ui::text_layout_result::TextLayoutResult {
        let font_size = resolve_font_size(style);
        let Some(font) = self.text_resources.fonts().resolve(style) else {
            return TextLayoutResult::monospaced(
                text.text.as_str(),
                fallback_char_width(font_size),
                fallback_line_height(font_size),
            );
        };

        layout_text_with_font(text.text.as_str(), style, font)
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

fn measure_text_impl(
    text: &str,
    style: &cranpose_ui::text::TextStyle,
    font_size: f32,
    font: Option<&SoftwareTextFont>,
) -> TextMetrics {
    let Some(font) = font else {
        return fallback_text_metrics(text, font_size);
    };

    measure_text_with_font(text, style, font_size, font)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fallback_text_metrics_cover_empty_and_multiline_text() {
        let empty = fallback_text_metrics("", 10.0);
        assert_eq!(empty.line_count, 1);
        assert_eq!(empty.width, 0.0);
        assert_eq!(empty.height, fallback_line_height(10.0));

        let multiline = fallback_text_metrics("ab\ncde", 10.0);
        assert_eq!(multiline.line_count, 2);
        assert_eq!(multiline.width, 3.0 * fallback_char_width(10.0));
        assert_eq!(multiline.height, 2.0 * fallback_line_height(10.0));
    }

    #[test]
    fn fallback_cursor_position_handles_non_boundary_byte_offsets() {
        let text = "éx";
        let width = fallback_char_width(12.0);
        assert_eq!(fallback_cursor_x_for_byte_offset(text, 0, 12.0), 0.0);
        assert_eq!(fallback_cursor_x_for_byte_offset(text, 1, 12.0), width);
        assert_eq!(
            fallback_cursor_x_for_byte_offset(text, text.len(), 12.0),
            width * 2.0
        );
    }

    #[test]
    fn cached_font_text_metrics_cache_recovers_after_poison() {
        let measurer =
            CachedFontTextMeasurer::with_text_resources(SoftwareTextResources::default(), 8);
        let text = cranpose_ui::text::AnnotatedString::from("Recovered software text");

        let poison_result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            let _guard = measurer
                .cache
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner());
            panic!("poison software text metrics cache for recovery test");
        }));

        assert!(poison_result.is_err());

        let metrics = measurer.measure(&text, &cranpose_ui::text::TextStyle::default());
        assert!(metrics.width > 0.0);
        assert!(metrics.height > 0.0);
    }
}
