use crate::font::DEFAULT_LINE_HEIGHT;
use compose_ui_graphics::Size;
use glyphon::{Attrs, Buffer, FontSystem, Metrics, Shaping};

pub const TEXT_CACHE_INITIAL_CAPACITY: usize = 128;
pub const TEXT_CACHE_MAX_CAPACITY: usize = 4096;

#[derive(Clone, Hash, PartialEq, Eq)]
pub struct TextCacheKey {
    text: String,
    scale_key: u32,
}

impl TextCacheKey {
    pub fn new(text: &str, scale: f32) -> Self {
        let scaled = (scale * 1000.0).round().max(0.0);
        let scale_key = scaled.min(u32::MAX as f32) as u32;
        Self {
            text: text.to_string(),
            scale_key,
        }
    }

    pub fn text(&self) -> &str {
        &self.text
    }

    pub fn scale_key(&self) -> u32 {
        self.scale_key
    }
}

#[derive(Default, Clone, Copy, Debug)]
pub struct LayoutMetrics {
    pub size: Size,
}

pub struct CachedTextBuffer {
    pub buffer: Buffer,
    metrics: Metrics,
    scale_key: u32,
    height: f32,
    text: String,
    layout: LayoutMetrics,
    uses_fallback: bool,
}

impl CachedTextBuffer {
    pub fn new(
        font_system: &mut FontSystem,
        metrics: Metrics,
        scale_key: u32,
        height: f32,
        text: &str,
        attrs: Attrs,
        fallback_attrs: Option<Attrs>,
    ) -> Self {
        let mut buffer = Buffer::new(font_system, metrics);
        buffer.set_size(font_system, f32::MAX, height);
        let mut cached = Self {
            buffer,
            metrics,
            scale_key,
            height,
            text: text.to_string(),
            layout: LayoutMetrics::default(),
            uses_fallback: false,
        };
        let mut glyphs = cached.reflow_text(font_system, text, attrs);
        if glyphs == 0 {
            if let Some(fallback) = fallback_attrs {
                if fallback != attrs {
                    glyphs = cached.reflow_text(font_system, text, fallback);
                    cached.uses_fallback = glyphs > 0;
                }
            }
        }
        if glyphs == 0 {
            cached.uses_fallback = false;
        }
        cached
    }

    pub fn ensure(
        &mut self,
        font_system: &mut FontSystem,
        metrics: Metrics,
        scale_key: u32,
        height: f32,
        text: &str,
        primary_attrs: Attrs,
        fallback_attrs: Option<Attrs>,
    ) -> bool {
        const HEIGHT_EPSILON: f32 = 0.5;

        let mut reshaped = false;
        let mut needs_reflow = false;

        if self.scale_key != scale_key || self.metrics != metrics {
            self.buffer.set_metrics(font_system, metrics);
            self.metrics = metrics;
            self.scale_key = scale_key;
            needs_reflow = true;
        }

        if (height - self.height).abs() > HEIGHT_EPSILON {
            self.buffer.set_size(font_system, f32::MAX, height);
            self.height = height;
            needs_reflow = true;
        }

        if self.text != text {
            needs_reflow = true;
        }

        if needs_reflow {
            let mut glyphs = {
                let first_attrs = if self.uses_fallback {
                    fallback_attrs.unwrap_or(primary_attrs)
                } else {
                    primary_attrs
                };
                let glyphs = self.reflow_text(font_system, text, first_attrs);
                self.uses_fallback = first_attrs != primary_attrs && glyphs > 0;
                glyphs
            };

            if glyphs == 0 {
                if let Some(fallback) = fallback_attrs {
                    if fallback != primary_attrs {
                        glyphs = self.reflow_text(font_system, text, fallback);
                        self.uses_fallback = glyphs > 0;
                    }
                } else {
                    self.uses_fallback = false;
                }
            }

            if glyphs == 0 {
                self.uses_fallback = false;
            }

            reshaped = true;
        }

        reshaped
    }

    pub fn layout_metrics(&self) -> LayoutMetrics {
        self.layout
    }

    pub fn uses_fallback(&self) -> bool {
        self.uses_fallback
    }

    fn glyph_count(&self) -> usize {
        let mut glyphs = 0usize;
        for run in self.buffer.layout_runs() {
            glyphs += run.glyphs.len();
        }
        glyphs
    }

    fn reflow_text(&mut self, font_system: &mut FontSystem, text: &str, attrs: Attrs) -> usize {
        self.buffer
            .set_text(font_system, text, attrs, Shaping::Advanced);
        self.buffer.shape_until_scroll(font_system);
        self.text.clear();
        self.text.push_str(text);
        self.update_layout_metrics();
        self.glyph_count()
    }

    fn update_layout_metrics(&mut self) {
        let mut max_width = 0.0f32;
        let mut total_lines = 0usize;
        let mut last_line = None;

        for run in self.buffer.layout_runs() {
            if last_line != Some(run.line_i) {
                total_lines += 1;
                last_line = Some(run.line_i);
            }
            max_width = max_width.max(run.line_w);
        }

        let line_height = self.metrics.line_height;
        let total_height = if total_lines == 0 {
            0.0
        } else {
            total_lines as f32 * line_height
        };

        if total_lines == 0 {
            // For empty buffers we still want a sensible height.
            self.layout.size = Size {
                width: 0.0,
                height: DEFAULT_LINE_HEIGHT,
            };
        } else {
            self.layout.size = Size {
                width: max_width,
                height: total_height,
            };
        }
    }
}

pub fn text_metrics_from_buffer(buffer: &CachedTextBuffer) -> compose_ui::TextMetrics {
    compose_ui::TextMetrics {
        width: buffer.layout_metrics().size.width,
        height: buffer.layout_metrics().size.height,
    }
}

pub fn grow_text_cache(cache: &mut lru::LruCache<TextCacheKey, Box<CachedTextBuffer>>) {
    let current_cap = cache.cap().get();
    if current_cap >= TEXT_CACHE_MAX_CAPACITY {
        return;
    }

    let mut new_cap = (current_cap * 2).max(TEXT_CACHE_INITIAL_CAPACITY);
    if new_cap > TEXT_CACHE_MAX_CAPACITY {
        new_cap = TEXT_CACHE_MAX_CAPACITY;
    }

    if let Some(capacity) = std::num::NonZeroUsize::new(new_cap) {
        cache.resize(capacity);
    }
}
