use std::{
    borrow::Cow,
    cell::{Cell, RefCell},
    collections::{HashMap, VecDeque, hash_map::Entry},
    hash::Hash,
    ops::Range,
    rc::Rc,
};

use cranpose_core::NodeId;
use web_time::Instant;

use super::{
    layout_options::{TextLayoutOptions, TextOverflow},
    paragraph::{Hyphens, LineBreak},
    style::TextStyle,
};
use crate::{font_scale::FontScaleCurve, text_layout_result::TextLayoutResult};

const ELLIPSIS: &str = "\u{2026}";
const DEFAULT_FONT_SIZE_SP: f32 = 14.0;
const WRAP_EPSILON: f32 = 0.5;
const SCALE_DOWN_SEARCH_STEPS: usize = 14;
const AUTO_HYPHEN_MIN_SEGMENT_CHARS: usize = 2;
const AUTO_HYPHEN_MIN_TRAILING_CHARS: usize = 3;
const AUTO_HYPHEN_PREFERRED_TRAILING_CHARS: usize = 4;
const TEXT_SERVICE_CACHE_CAPACITY: usize = 8192;
const TEXT_LAYOUT_TELEMETRY_ENV: &str = "CRANPOSE_TEXT_LAYOUT_TELEMETRY";

fn text_layout_telemetry_enabled() -> bool {
    cranpose_core::env_flag!(TEXT_LAYOUT_TELEMETRY_ENV)
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct TextMetrics {
    pub width: f32,
    pub height: f32,
    /// Height of a single line of text
    pub line_height: f32,
    /// Number of lines in the text
    pub line_count: usize,
}

#[derive(Clone, Debug, PartialEq)]
pub struct PreparedTextLayout {
    /// Shared display text after wrapping and overflow have been resolved.
    pub text: Rc<crate::text::AnnotatedString>,
    pub visual_style: TextStyle,
    pub metrics: TextMetrics,
    pub did_overflow: bool,
}

#[derive(Clone, Debug, PartialEq)]
pub struct TextLinePrefixWidths {
    prefix_widths: Vec<f32>,
    separator_before: Vec<f32>,
    non_empty_overhang: f32,
}

impl TextLinePrefixWidths {
    pub fn from_parts(
        prefix_widths: Vec<f32>,
        separator_before: Vec<f32>,
        non_empty_overhang: f32,
    ) -> Option<Self> {
        if prefix_widths.is_empty() || prefix_widths.len() != separator_before.len() + 1 {
            return None;
        }
        if prefix_widths
            .iter()
            .chain(separator_before.iter())
            .any(|value| !value.is_finite())
        {
            return None;
        }
        let non_empty_overhang = non_empty_overhang.max(0.0);
        if !non_empty_overhang.is_finite() {
            return None;
        }
        Some(Self {
            prefix_widths,
            separator_before,
            non_empty_overhang,
        })
    }

    pub fn monospaced(char_count: usize, char_width: f32, letter_spacing: f32) -> Option<Self> {
        if !char_width.is_finite() || !letter_spacing.is_finite() {
            return None;
        }
        let char_width = char_width.max(0.0);
        let letter_spacing = letter_spacing.max(0.0);
        let mut prefix_widths = Vec::with_capacity(char_count + 1);
        let mut separator_before = Vec::with_capacity(char_count);
        let mut width = 0.0f32;
        prefix_widths.push(width);
        for _ in 0..char_count {
            separator_before.push(0.0);
            width += char_width + letter_spacing;
            prefix_widths.push(width);
        }
        Self::from_parts(prefix_widths, separator_before, 0.0)
    }

    pub fn char_count(&self) -> usize {
        self.separator_before.len()
    }

    pub fn width_for_char_range(&self, start: usize, end: usize) -> Option<f32> {
        if start > end || end > self.char_count() {
            return None;
        }
        if start == end {
            return Some(0.0);
        }
        let separator = self.separator_before.get(start).copied().unwrap_or(0.0);
        Some(
            (self.prefix_widths[end] - self.prefix_widths[start] - separator).max(0.0)
                + self.non_empty_overhang,
        )
    }
}

pub trait TextMeasurer: 'static {
    fn measure(&self, text: &crate::text::AnnotatedString, style: &TextStyle) -> TextMetrics;

    fn measure_for_node(
        &self,
        node_id: Option<NodeId>,
        text: &crate::text::AnnotatedString,
        style: &TextStyle,
    ) -> TextMetrics {
        let _ = node_id;
        self.measure(text, style)
    }

    fn measure_subsequence(
        &self,
        text: &crate::text::AnnotatedString,
        range: Range<usize>,
        style: &TextStyle,
    ) -> TextMetrics {
        self.measure(&text.subsequence(range), style)
    }

    fn measure_subsequence_for_node(
        &self,
        node_id: Option<NodeId>,
        text: &crate::text::AnnotatedString,
        range: Range<usize>,
        style: &TextStyle,
    ) -> TextMetrics {
        let _ = node_id;
        self.measure_subsequence(text, range, style)
    }

    fn measure_line_prefix_widths(
        &self,
        text: &crate::text::AnnotatedString,
        line_range: Range<usize>,
        style: &TextStyle,
    ) -> Option<TextLinePrefixWidths> {
        let _ = text;
        let _ = line_range;
        let _ = style;
        None
    }

    fn measure_line_width(
        &self,
        text: &crate::text::AnnotatedString,
        line_range: Range<usize>,
        style: &TextStyle,
    ) -> Option<f32> {
        let _ = text;
        let _ = line_range;
        let _ = style;
        None
    }

    fn line_height(&self, text: &crate::text::AnnotatedString, style: &TextStyle) -> f32 {
        self.measure(text, style).line_height
    }

    /// The tight glyph box of a text line inside its `line_height` slot:
    /// `(top_offset, height)` in logical units, where `height` is the font's
    /// natural ascent+descent extent and `top_offset` positions it within the
    /// slot (glyph rows are vertically centered). Selection chrome — the
    /// highlight, the caret and the finger handles — anchors to THIS box,
    /// not the full slot: with a paragraph line height above the natural one
    /// the reference shows gaps between highlighted lines and handles riding
    /// the glyphs. `None` means the box fills the slot.
    fn glyph_line_box(&self, style: &TextStyle) -> Option<(f32, f32)> {
        let _ = style;
        None
    }

    /// Distance from the top of a line slot down to that line's baseline, in
    /// logical units — the number the rasterizer places glyph origins at.
    ///
    /// Callers that position text by baseline (rather than by its box) need
    /// this; `None` means the measurer has no font metrics to answer with.
    fn first_baseline(&self, style: &TextStyle) -> Option<f32> {
        let _ = style;
        None
    }

    /// One line's whole box for a style: its height and its baseline, with no
    /// string to measure.
    ///
    /// A layout that stacks rows of a known style needs the row pitch before it
    /// has any text to put in them, and taking the height from one call and the
    /// baseline from another lets the two come from different rules. `None` when
    /// the measurer has no font metrics, same as [`Self::first_baseline`].
    fn line_box(&self, style: &TextStyle) -> Option<crate::text::LineBox> {
        let baseline = self.first_baseline(style)?;
        Some(crate::text::LineBox {
            height: self.line_height(&crate::text::AnnotatedString::default(), style),
            baseline,
        })
    }

    fn line_height_for_node(
        &self,
        node_id: Option<NodeId>,
        text: &crate::text::AnnotatedString,
        style: &TextStyle,
    ) -> f32 {
        let _ = node_id;
        self.line_height(text, style)
    }

    fn get_offset_for_position(
        &self,
        text: &crate::text::AnnotatedString,
        style: &TextStyle,
        x: f32,
        y: f32,
    ) -> usize;

    fn get_cursor_x_for_offset(
        &self,
        text: &crate::text::AnnotatedString,
        style: &TextStyle,
        offset: usize,
    ) -> f32;

    fn layout(&self, text: &crate::text::AnnotatedString, style: &TextStyle) -> TextLayoutResult;

    /// Returns an alternate break boundary for `Hyphens::Auto` when a greedy break
    /// split lands in the middle of a word.
    ///
    /// `segment_start_char` and `measured_break_char` are character-boundary indices
    /// in `line` (not byte offsets). Return `None` to delegate to fallback behavior.
    fn choose_auto_hyphen_break(
        &self,
        _line: &str,
        _style: &TextStyle,
        _segment_start_char: usize,
        _measured_break_char: usize,
    ) -> Option<usize> {
        None
    }

    fn measure_with_options(
        &self,
        text: &crate::text::AnnotatedString,
        style: &TextStyle,
        options: TextLayoutOptions,
        max_width: Option<f32>,
    ) -> TextMetrics {
        self.prepare_with_options(text, style, options, max_width)
            .metrics
    }

    fn measure_with_options_for_node(
        &self,
        node_id: Option<NodeId>,
        text: &crate::text::AnnotatedString,
        style: &TextStyle,
        options: TextLayoutOptions,
        max_width: Option<f32>,
    ) -> TextMetrics {
        self.prepare_with_options_for_node(node_id, text, style, options, max_width)
            .metrics
    }

    fn prepare_with_options(
        &self,
        text: &crate::text::AnnotatedString,
        style: &TextStyle,
        options: TextLayoutOptions,
        max_width: Option<f32>,
    ) -> PreparedTextLayout {
        self.prepare_with_options_fallback(text, style, options, max_width)
    }

    fn prepare_with_options_for_node(
        &self,
        node_id: Option<NodeId>,
        text: &crate::text::AnnotatedString,
        style: &TextStyle,
        options: TextLayoutOptions,
        max_width: Option<f32>,
    ) -> PreparedTextLayout {
        prepare_text_layout_with_measurer_for_node(self, node_id, text, style, options, max_width)
    }

    fn prepare_with_options_fallback(
        &self,
        text: &crate::text::AnnotatedString,
        style: &TextStyle,
        options: TextLayoutOptions,
        max_width: Option<f32>,
    ) -> PreparedTextLayout {
        prepare_text_layout_fallback(self, text, style, options, max_width)
    }
}

#[derive(Default)]
struct MonospacedTextMeasurer;

impl MonospacedTextMeasurer {
    const DEFAULT_SIZE: f32 = 14.0;
    const CHAR_WIDTH_RATIO: f32 = 0.6;

    fn get_metrics(style: &TextStyle) -> (f32, f32) {
        let font_size = style.resolve_font_size(Self::DEFAULT_SIZE);
        let line_height = style.resolve_line_height(Self::DEFAULT_SIZE, font_size);
        let letter_spacing = style.resolve_letter_spacing(Self::DEFAULT_SIZE).max(0.0);
        (
            (font_size * Self::CHAR_WIDTH_RATIO) + letter_spacing,
            line_height,
        )
    }
}

impl TextMeasurer for MonospacedTextMeasurer {
    fn measure(&self, text: &crate::text::AnnotatedString, style: &TextStyle) -> TextMetrics {
        let (char_width, line_height) = Self::get_metrics(style);

        let lines: Vec<&str> = text.text.split('\n').collect();
        let line_count = lines.len().max(1);

        let width = lines
            .iter()
            .map(|line| line.chars().count() as f32 * char_width)
            .fold(0.0_f32, f32::max);

        TextMetrics {
            width,
            height: line_count as f32 * line_height,
            line_height,
            line_count,
        }
    }

    fn measure_subsequence(
        &self,
        text: &crate::text::AnnotatedString,
        range: Range<usize>,
        style: &TextStyle,
    ) -> TextMetrics {
        let (char_width, line_height) = Self::get_metrics(style);
        let slice = &text.text[range];
        let line_count = slice.split('\n').count().max(1);
        let width = slice
            .split('\n')
            .map(|line| line.chars().count() as f32 * char_width)
            .fold(0.0_f32, f32::max);

        TextMetrics {
            width,
            height: line_count as f32 * line_height,
            line_height,
            line_count,
        }
    }

    fn measure_line_prefix_widths(
        &self,
        text: &crate::text::AnnotatedString,
        line_range: Range<usize>,
        style: &TextStyle,
    ) -> Option<TextLinePrefixWidths> {
        let font_size = style.resolve_font_size(Self::DEFAULT_SIZE);
        let letter_spacing = style.resolve_letter_spacing(Self::DEFAULT_SIZE);
        TextLinePrefixWidths::monospaced(
            text.text[line_range].chars().count(),
            font_size * Self::CHAR_WIDTH_RATIO,
            letter_spacing,
        )
    }

    fn measure_line_width(
        &self,
        text: &crate::text::AnnotatedString,
        line_range: Range<usize>,
        style: &TextStyle,
    ) -> Option<f32> {
        Some(self.measure_subsequence(text, line_range, style).width)
    }

    fn line_height(&self, _text: &crate::text::AnnotatedString, style: &TextStyle) -> f32 {
        let (_, line_height) = Self::get_metrics(style);
        line_height
    }

    fn get_offset_for_position(
        &self,
        text: &crate::text::AnnotatedString,
        style: &TextStyle,
        x: f32,
        y: f32,
    ) -> usize {
        let (char_width, line_height) = Self::get_metrics(style);

        if text.text.is_empty() {
            return 0;
        }

        let line_index = (y / line_height).floor().max(0.0) as usize;
        let lines: Vec<&str> = text.text.split('\n').collect();
        let target_line = line_index.min(lines.len().saturating_sub(1));

        let mut line_start_byte = 0;
        for line in lines.iter().take(target_line) {
            line_start_byte += line.len() + 1;
        }

        let line_text = lines.get(target_line).unwrap_or(&"");
        let char_index = (x / char_width).round() as usize;
        let line_char_count = line_text.chars().count();
        let clamped_index = char_index.min(line_char_count);

        let offset_in_line = line_text
            .char_indices()
            .nth(clamped_index)
            .map_or(line_text.len(), |(i, _)| i);

        line_start_byte + offset_in_line
    }

    fn get_cursor_x_for_offset(
        &self,
        text: &crate::text::AnnotatedString,
        style: &TextStyle,
        offset: usize,
    ) -> f32 {
        let (char_width, _) = Self::get_metrics(style);

        let clamped_offset = offset.min(text.text.len());
        let char_count = text.text[..clamped_offset].chars().count();
        char_count as f32 * char_width
    }

    fn layout(&self, text: &crate::text::AnnotatedString, style: &TextStyle) -> TextLayoutResult {
        let (char_width, line_height) = Self::get_metrics(style);
        TextLayoutResult::monospaced(&text.text, char_width, line_height)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
struct TextBaseCacheKey {
    text_hash: u64,
    style_hash: u64,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
struct TextOptionsCacheKey {
    base: TextBaseCacheKey,
    options: TextLayoutOptions,
    max_width_bits: Option<u32>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
struct TextPreparedCacheKey {
    base: TextOptionsCacheKey,
    visual_hash: u64,
}

struct BoundedTextCache<K, V> {
    capacity: usize,
    entries: HashMap<K, V>,
    order: VecDeque<K>,
}

impl<K, V> BoundedTextCache<K, V>
where
    K: Clone + Eq + Hash,
    V: Clone,
{
    fn new(capacity: usize) -> Self {
        Self {
            capacity,
            entries: HashMap::new(),
            order: VecDeque::new(),
        }
    }

    fn clear(&mut self) {
        self.entries.clear();
        self.order.clear();
    }

    fn get(&self, key: &K) -> Option<V> {
        self.entries.get(key).cloned()
    }

    fn insert(&mut self, key: K, value: V) {
        match self.entries.entry(key.clone()) {
            Entry::Occupied(mut entry) => {
                entry.insert(value);
                return;
            }
            Entry::Vacant(_) => {}
        }
        if self.entries.len() == self.capacity {
            while let Some(evicted) = self.order.pop_front() {
                if self.entries.remove(&evicted).is_some() {
                    break;
                }
            }
        }
        self.order.push_back(key.clone());
        self.entries.insert(key, value);
    }
}

pub(crate) struct TextService {
    generation: Cell<u64>,
    measurer: RefCell<Rc<dyn TextMeasurer>>,
    metrics_cache: RefCell<BoundedTextCache<TextBaseCacheKey, TextMetrics>>,
    options_metrics_cache: RefCell<BoundedTextCache<TextOptionsCacheKey, TextMetrics>>,
    prepared_cache: RefCell<BoundedTextCache<TextPreparedCacheKey, PreparedTextLayout>>,
    layout_cache: RefCell<BoundedTextCache<TextBaseCacheKey, TextLayoutResult>>,
}

impl TextService {
    pub(crate) fn new() -> Self {
        Self::from_measurer(Rc::new(MonospacedTextMeasurer))
    }

    pub(crate) fn from_measurer(measurer: Rc<dyn TextMeasurer>) -> Self {
        Self {
            generation: Cell::new(1),
            measurer: RefCell::new(measurer),
            metrics_cache: RefCell::new(BoundedTextCache::new(TEXT_SERVICE_CACHE_CAPACITY)),
            options_metrics_cache: RefCell::new(BoundedTextCache::new(TEXT_SERVICE_CACHE_CAPACITY)),
            prepared_cache: RefCell::new(BoundedTextCache::new(TEXT_SERVICE_CACHE_CAPACITY)),
            layout_cache: RefCell::new(BoundedTextCache::new(TEXT_SERVICE_CACHE_CAPACITY)),
        }
    }

    pub(crate) fn set_measurer(&self, measurer: Rc<dyn TextMeasurer>) {
        *self.measurer.borrow_mut() = measurer;
        self.clear_caches();
    }

    pub(crate) fn generation(&self) -> u64 {
        self.generation.get()
    }

    pub(crate) fn current_measurer(&self) -> Rc<dyn TextMeasurer> {
        Rc::clone(&self.measurer.borrow())
    }

    pub(crate) fn with_measurer<R>(&self, f: impl FnOnce(&dyn TextMeasurer) -> R) -> R {
        let measurer = self.current_measurer();
        f(&*measurer)
    }

    pub(crate) fn measure(
        &self,
        node_id: Option<NodeId>,
        text: &crate::text::AnnotatedString,
        style: &TextStyle,
    ) -> TextMetrics {
        let key = text_base_cache_key(text, style);
        if let Some(metrics) = self.metrics_cache.borrow().get(&key) {
            return metrics;
        }
        let metrics = self.with_measurer(|m| m.measure_for_node(node_id, text, style));
        self.metrics_cache.borrow_mut().insert(key, metrics);
        metrics
    }

    pub(crate) fn measure_with_options(
        &self,
        node_id: Option<NodeId>,
        text: &crate::text::AnnotatedString,
        style: &TextStyle,
        options: TextLayoutOptions,
        max_width: Option<f32>,
    ) -> TextMetrics {
        let key = text_options_cache_key(text, style, options.normalized(), max_width);
        if let Some(metrics) = self.options_metrics_cache.borrow().get(&key) {
            return metrics;
        }
        let metrics = self.with_measurer(|m| {
            m.measure_with_options_for_node(node_id, text, style, options.normalized(), max_width)
        });
        self.options_metrics_cache.borrow_mut().insert(key, metrics);
        metrics
    }

    pub(crate) fn prepare_with_options(
        &self,
        node_id: Option<NodeId>,
        text: &crate::text::AnnotatedString,
        style: &TextStyle,
        options: TextLayoutOptions,
        max_width: Option<f32>,
    ) -> PreparedTextLayout {
        let metrics_key = text_options_cache_key(text, style, options.normalized(), max_width);
        let key = TextPreparedCacheKey {
            base: metrics_key,
            visual_hash: style.render_hash(),
        };
        if let Some(prepared) = self.prepared_cache.borrow().get(&key) {
            return prepared;
        }
        let prepared = self.with_measurer(|m| {
            m.prepare_with_options_for_node(node_id, text, style, options.normalized(), max_width)
        });
        self.prepared_cache
            .borrow_mut()
            .insert(key, prepared.clone());
        self.options_metrics_cache
            .borrow_mut()
            .insert(metrics_key, prepared.metrics);
        prepared
    }

    pub(crate) fn layout(
        &self,
        text: &crate::text::AnnotatedString,
        style: &TextStyle,
    ) -> TextLayoutResult {
        let key = text_base_cache_key(text, style);
        if let Some(layout) = self.layout_cache.borrow().get(&key) {
            return layout;
        }
        let layout = self.with_measurer(|m| m.layout(text, style));
        self.layout_cache.borrow_mut().insert(key, layout.clone());
        layout
    }

    fn clear_caches(&self) {
        self.generation
            .set(self.generation.get().wrapping_add(1).max(1));
        self.metrics_cache.borrow_mut().clear();
        self.options_metrics_cache.borrow_mut().clear();
        self.prepared_cache.borrow_mut().clear();
        self.layout_cache.borrow_mut().clear();
    }
}

fn text_base_cache_key(text: &crate::text::AnnotatedString, style: &TextStyle) -> TextBaseCacheKey {
    TextBaseCacheKey {
        text_hash: text.render_hash(),
        style_hash: style.measurement_hash(),
    }
}

fn text_options_cache_key(
    text: &crate::text::AnnotatedString,
    style: &TextStyle,
    options: TextLayoutOptions,
    max_width: Option<f32>,
) -> TextOptionsCacheKey {
    TextOptionsCacheKey {
        base: text_base_cache_key(text, style),
        options: options.normalized(),
        max_width_bits: normalize_max_width(max_width).map(f32::to_bits),
    }
}

pub fn set_text_measurer<M: TextMeasurer>(measurer: M) {
    crate::render_state::set_current_text_measurer(Rc::new(measurer));
}

pub(crate) fn current_text_generation() -> u64 {
    crate::render_state::with_text_service(TextService::generation)
}

pub fn measure_text(text: &crate::text::AnnotatedString, style: &TextStyle) -> TextMetrics {
    with_system_font_scale(text, style, |text, style| {
        crate::render_state::with_text_service(|service| service.measure(None, text, style))
    })
}

pub(crate) fn measure_resolved_text(
    text: &crate::text::AnnotatedString,
    style: &TextStyle,
) -> TextMetrics {
    crate::render_state::with_text_service(|service| service.measure(None, text, style))
}

pub(crate) fn resolved_first_baseline(style: &TextStyle) -> Option<f32> {
    crate::render_state::with_text_service(|service| {
        service.with_measurer(|m| m.first_baseline(style))
    })
}

pub(crate) fn resolved_line_box(style: &TextStyle) -> Option<crate::text::LineBox> {
    crate::render_state::current_app_context()?;
    crate::render_state::with_text_service(|service| service.with_measurer(|m| m.line_box(style)))
}

/// The tight glyph box `(top_offset, height)` of a `style` text line inside
/// its line slot (see [`TextMeasurer::glyph_line_box`]). Falls back to the
/// full slot when the active measurer has no font metrics.
pub fn glyph_line_box(style: &TextStyle, line_height: f32) -> (f32, f32) {
    let style = scale_text_style_font_sizes(style, crate::current_font_scale_curve());
    crate::render_state::with_text_service(|service| {
        service.with_measurer(|m| m.glyph_line_box(&style))
    })
    .map_or((0.0, line_height), |(off, h)| {
        (off.min(line_height), h.min(line_height))
    })
}

/// Distance from the top of a `style` line slot down to its baseline (see
/// [`TextMeasurer::first_baseline`]). `None` when the active measurer carries
/// no font metrics.
pub fn first_baseline(style: &TextStyle) -> Option<f32> {
    let style = scale_text_style_font_sizes(style, crate::current_font_scale_curve());
    crate::render_state::with_text_service(|service| {
        service.with_measurer(|m| m.first_baseline(&style))
    })
}

pub fn measure_text_for_node(
    node_id: Option<NodeId>,
    text: &crate::text::AnnotatedString,
    style: &TextStyle,
) -> TextMetrics {
    with_system_font_scale(text, style, |text, style| {
        crate::render_state::with_text_service(|service| service.measure(node_id, text, style))
    })
}

pub fn measure_text_with_options(
    text: &crate::text::AnnotatedString,
    style: &TextStyle,
    options: TextLayoutOptions,
    max_width: Option<f32>,
) -> TextMetrics {
    with_system_font_scale(text, style, |text, style| {
        crate::render_state::with_text_service(|service| {
            service.measure_with_options(None, text, style, options.normalized(), max_width)
        })
    })
}

pub fn measure_text_with_options_for_node(
    node_id: Option<NodeId>,
    text: &crate::text::AnnotatedString,
    style: &TextStyle,
    options: TextLayoutOptions,
    max_width: Option<f32>,
) -> TextMetrics {
    with_system_font_scale(text, style, |text, style| {
        crate::render_state::with_text_service(|service| {
            service.measure_with_options(node_id, text, style, options.normalized(), max_width)
        })
    })
}

pub fn prepare_text_layout(
    text: &crate::text::AnnotatedString,
    style: &TextStyle,
    options: TextLayoutOptions,
    max_width: Option<f32>,
) -> PreparedTextLayout {
    with_system_font_scale(text, style, |text, style| {
        crate::render_state::with_text_service(|service| {
            service.prepare_with_options(None, text, style, options.normalized(), max_width)
        })
    })
}

pub fn prepare_text_layout_for_node(
    node_id: Option<NodeId>,
    text: &crate::text::AnnotatedString,
    style: &TextStyle,
    options: TextLayoutOptions,
    max_width: Option<f32>,
) -> PreparedTextLayout {
    with_system_font_scale(text, style, |text, style| {
        crate::render_state::with_text_service(|service| {
            service.prepare_with_options(node_id, text, style, options.normalized(), max_width)
        })
    })
}

pub fn get_offset_for_position(
    text: &crate::text::AnnotatedString,
    style: &TextStyle,
    x: f32,
    y: f32,
) -> usize {
    with_system_font_scale(text, style, |text, style| {
        crate::render_state::with_text_measurer(|m| m.get_offset_for_position(text, style, x, y))
    })
}

/// Byte offset nearest the local content position (`x`, `y`), **wrap-aware** —
/// the single hit-test every editable-text pointer path uses (tap-to-place,
/// drag-select, and selection-handle drag).
///
/// It is the inverse of the drawn caret and [`wrapped_line_ranges`]: `y` selects
/// the VISUAL (wrapped) line (`floor(y / line_height)`), then `x` picks the
/// nearest char boundary WITHIN that line (delegated to the measurer with `y`
/// forced to 0). `x`/`y` must already be in text space (padding- and
/// pan-adjusted). The plain [`get_offset_for_position`] maps `y` through the
/// measurer's logical `\n` layout, so on wrapped text it lands on the wrong line
/// (an error that grows with each wrapped line above the finger). With
/// `wrap_width == None` (single-line fields) this reduces to the one logical
/// line.
pub fn offset_for_position_wrapped(
    text: &str,
    style: &TextStyle,
    node_id: Option<NodeId>,
    wrap_width: Option<f32>,
    line_height: f32,
    x: f32,
    y: f32,
) -> usize {
    if text.is_empty() {
        return 0;
    }
    let annotated = crate::text::AnnotatedString::from(text);
    let line_ranges = wrapped_line_ranges(
        node_id,
        &annotated,
        style,
        TextLayoutOptions::default(),
        wrap_width,
    );
    if line_ranges.is_empty() {
        return 0;
    }
    let line_idx = if line_height > 0.0 {
        (y / line_height).floor().max(0.0) as usize
    } else {
        0
    }
    .min(line_ranges.len() - 1);
    let range = &line_ranges[line_idx];
    let line = &text[range.start..range.end];
    let within = get_offset_for_position(&crate::text::AnnotatedString::from(line), style, x, 0.0);
    range.start + within.min(line.len())
}

pub fn get_cursor_x_for_offset(
    text: &crate::text::AnnotatedString,
    style: &TextStyle,
    offset: usize,
) -> f32 {
    with_system_font_scale(text, style, |text, style| {
        crate::render_state::with_text_measurer(|m| m.get_cursor_x_for_offset(text, style, offset))
    })
}

pub fn layout_text(text: &crate::text::AnnotatedString, style: &TextStyle) -> TextLayoutResult {
    with_system_font_scale(text, style, |text, style| {
        crate::render_state::with_text_service(|service| service.layout(text, style))
    })
}

/// Returns the source-text byte range covered by each **visual** (wrapped) line
/// when `text` is laid out at `max_width` with `options`, matching the wrapping
/// the renderer performs. Each range excludes the trailing `\n`. With
/// `max_width == None` (or soft-wrap disabled) this is just the logical
/// `\n`-delimited lines.
///
/// The text field uses this to place its caret and selection handles on the
/// correct visual line for wrapped text: the in-content caret otherwise counts
/// only logical `\n` lines, so a caret on a wrapped line's second visual line is
/// drawn on the first (and its x, being the whole logical-line prefix width,
/// runs off the right edge and is clipped), while typing and the magnifier land
/// on the correct spot.
pub fn wrapped_line_ranges(
    node_id: Option<NodeId>,
    text: &crate::text::AnnotatedString,
    style: &TextStyle,
    options: TextLayoutOptions,
    max_width: Option<f32>,
) -> Vec<Range<usize>> {
    with_system_font_scale(text, style, |text, style| {
        crate::render_state::with_text_measurer(|m| {
            wrapped_line_ranges_with_measurer(m, node_id, text, style, options, max_width)
        })
    })
}

fn wrapped_line_ranges_with_measurer<M: TextMeasurer + ?Sized>(
    measurer: &M,
    _node_id: Option<NodeId>,
    text: &crate::text::AnnotatedString,
    style: &TextStyle,
    options: TextLayoutOptions,
    max_width: Option<f32>,
) -> Vec<Range<usize>> {
    let opts = options.normalized();
    let max_width = normalize_max_width(max_width);
    let wrap_width = (opts.soft_wrap && opts.overflow != TextOverflow::Visible)
        .then_some(max_width)
        .flatten();
    let line_break_mode = style
        .paragraph_style
        .line_break
        .take_or_else(|| LineBreak::Simple);
    let hyphens_mode = style.paragraph_style.hyphens.take_or_else(|| Hyphens::None);

    let line_ranges = split_line_ranges(text.text.as_str());
    let Some(width_limit) = wrap_width else {
        return line_ranges;
    };
    let mut ranges = Vec::with_capacity(line_ranges.len());
    for line_range in line_ranges {
        for display_line in wrap_line_to_width(
            measurer,
            text,
            line_range,
            style,
            width_limit,
            line_break_mode,
            hyphens_mode,
        ) {
            ranges.push(display_line.source_range.clone());
        }
    }
    ranges
}

fn prepare_text_layout_fallback<M: TextMeasurer + ?Sized>(
    measurer: &M,
    text: &crate::text::AnnotatedString,
    style: &TextStyle,
    options: TextLayoutOptions,
    max_width: Option<f32>,
) -> PreparedTextLayout {
    prepare_text_layout_with_measurer_for_node(measurer, None, text, style, options, max_width)
}

pub fn prepare_text_layout_with_measurer_for_node<M: TextMeasurer + ?Sized>(
    measurer: &M,
    node_id: Option<NodeId>,
    text: &crate::text::AnnotatedString,
    style: &TextStyle,
    options: TextLayoutOptions,
    max_width: Option<f32>,
) -> PreparedTextLayout {
    let telemetry = text_layout_telemetry_enabled();
    let total_start = telemetry.then(Instant::now);
    let opts = options.normalized();
    let max_width = normalize_max_width(max_width);
    if let Some(min_font_size_sp) = opts.overflow.scale_down_min_font_size_sp() {
        return prepare_scale_down_text_layout(
            measurer,
            node_id,
            text,
            style,
            opts,
            max_width,
            min_font_size_sp,
        );
    }

    let wrap_width = (opts.soft_wrap && opts.overflow != TextOverflow::Visible)
        .then_some(max_width)
        .flatten();
    let line_break_mode = style
        .paragraph_style
        .line_break
        .take_or_else(|| LineBreak::Simple);
    let hyphens_mode = style.paragraph_style.hyphens.take_or_else(|| Hyphens::None);

    let wrap_start = telemetry.then(Instant::now);
    let line_ranges = split_line_ranges(text.text.as_str());
    let source_line_count = line_ranges.len();
    let mut visible_lines: Vec<DisplayLine>;
    if let Some(width_limit) = wrap_width {
        visible_lines = Vec::with_capacity(line_ranges.len());
        for line_range in line_ranges {
            let wrapped_lines = wrap_line_to_width(
                measurer,
                text,
                line_range,
                style,
                width_limit,
                line_break_mode,
                hyphens_mode,
            );
            visible_lines.extend(wrapped_lines);
        }
    } else {
        visible_lines = line_ranges
            .into_iter()
            .map(DisplayLine::from_source_range)
            .collect();
    }
    let wrap_ms = wrap_start.map(|start| start.elapsed().as_secs_f64() * 1000.0);

    let overflow_start = telemetry.then(Instant::now);
    let did_overflow = apply_overflow(
        measurer,
        node_id,
        text,
        style,
        opts,
        max_width,
        &mut visible_lines,
    );
    let overflow_ms = overflow_start.map(|start| start.elapsed().as_secs_f64() * 1000.0);

    let build_start = telemetry.then(Instant::now);
    let display_annotated = build_display_annotated(text, &visible_lines);
    debug_assert_eq!(
        display_annotated.text,
        join_display_line_text(text, &visible_lines)
    );
    let build_ms = build_start.map(|start| start.elapsed().as_secs_f64() * 1000.0);

    let metrics_start = telemetry.then(Instant::now);
    let line_height = measurer.line_height_for_node(node_id, text, style).max(0.0);
    let display_line_count = visible_lines.len().max(1);
    let layout_line_count = display_line_count.max(opts.min_lines);

    let measured_width = if visible_lines.is_empty() {
        0.0
    } else {
        visible_lines
            .iter()
            .map(|line| line.measure_width(measurer, node_id, text, style))
            .fold(0.0_f32, f32::max)
    };
    let metrics_ms = metrics_start.map(|start| start.elapsed().as_secs_f64() * 1000.0);
    let width = if opts.overflow == TextOverflow::Visible {
        measured_width
    } else if let Some(width_limit) = max_width {
        measured_width.min(width_limit)
    } else {
        measured_width
    };

    let prepared = PreparedTextLayout {
        text: Rc::new(display_annotated),
        visual_style: style.clone(),
        metrics: TextMetrics {
            width,
            height: layout_line_count as f32 * line_height,
            line_height,
            line_count: layout_line_count,
        },
        did_overflow,
    };

    if let Some(start) = total_start {
        eprintln!(
            "[text-layout-telemetry] bytes={} spans={} source_lines={} display_lines={} wrap={} max_width={:?} wrap_ms={:.2} overflow_ms={:.2} build_ms={:.2} metrics_ms={:.2} total_ms={:.2}",
            text.text.len(),
            text.span_styles.len(),
            source_line_count,
            display_line_count,
            wrap_width.is_some(),
            max_width,
            wrap_ms.unwrap_or(0.0),
            overflow_ms.unwrap_or(0.0),
            build_ms.unwrap_or(0.0),
            metrics_ms.unwrap_or(0.0),
            start.elapsed().as_secs_f64() * 1000.0,
        );
    }

    prepared
}

fn prepare_scale_down_text_layout<M: TextMeasurer + ?Sized>(
    measurer: &M,
    node_id: Option<NodeId>,
    text: &crate::text::AnnotatedString,
    style: &TextStyle,
    options: TextLayoutOptions,
    max_width: Option<f32>,
    min_font_size_sp: f32,
) -> PreparedTextLayout {
    let clipped_options = TextLayoutOptions {
        overflow: TextOverflow::Clip,
        ..options
    }
    .normalized();

    let full_size = prepare_scaled_text_layout(
        measurer,
        node_id,
        text,
        style,
        clipped_options,
        max_width,
        FontScaleCurve::linear(1.0),
    );
    let Some(width_limit) = max_width else {
        return full_size;
    };
    if !full_size.did_overflow {
        return full_size;
    }

    let base_font_size = style.resolve_font_size(DEFAULT_FONT_SIZE_SP);
    if !base_font_size.is_finite() || base_font_size <= 0.0 {
        return full_size;
    }
    let min_scale = (min_font_size_sp.min(base_font_size) / base_font_size).clamp(0.0, 1.0);
    if min_scale >= 1.0 {
        return full_size;
    }

    let min_size = prepare_scaled_text_layout(
        measurer,
        node_id,
        text,
        style,
        clipped_options,
        Some(width_limit),
        FontScaleCurve::linear(min_scale),
    );
    if min_size.did_overflow {
        return min_size;
    }

    let mut low = min_scale;
    let mut high = 1.0;
    let mut best = min_size;
    for _ in 0..SCALE_DOWN_SEARCH_STEPS {
        let mid = (low + high) * 0.5;
        let candidate = prepare_scaled_text_layout(
            measurer,
            node_id,
            text,
            style,
            clipped_options,
            Some(width_limit),
            FontScaleCurve::linear(mid),
        );
        if candidate.did_overflow {
            high = mid;
        } else {
            low = mid;
            best = candidate;
        }
    }

    best
}

fn prepare_scaled_text_layout<M: TextMeasurer + ?Sized>(
    measurer: &M,
    node_id: Option<NodeId>,
    text: &crate::text::AnnotatedString,
    style: &TextStyle,
    options: TextLayoutOptions,
    max_width: Option<f32>,
    shrink: FontScaleCurve,
) -> PreparedTextLayout {
    let visual_style = scale_text_style_font_sizes(style, shrink);
    let visual_text = scale_annotated_font_sizes(text, shrink);
    prepare_text_layout_with_measurer_for_node(
        measurer,
        node_id,
        visual_text.as_ref(),
        &visual_style,
        options,
        max_width,
    )
}

fn scale_annotated_font_sizes(
    text: &crate::text::AnnotatedString,
    curve: FontScaleCurve,
) -> Cow<'_, crate::text::AnnotatedString> {
    if curve.is_identity() || !annotated_text_needs_scaling(text) {
        return Cow::Borrowed(text);
    }

    let mut scaled = text.clone();
    for span in &mut scaled.span_styles {
        span.item = scale_span_style_font_sizes(&span.item, curve, None);
    }
    Cow::Owned(scaled)
}

fn scale_text_style_font_sizes(style: &TextStyle, curve: FontScaleCurve) -> TextStyle {
    if curve.is_identity() {
        return style.clone();
    }

    let mut scaled = style.clone();
    scaled.span_style =
        scale_span_style_font_sizes(&style.span_style, curve, Some(DEFAULT_FONT_SIZE_SP));
    scaled.paragraph_style.line_height =
        scale_text_unit_sp(scaled.paragraph_style.line_height, curve);
    if let Some(mut indent) = scaled.paragraph_style.text_indent {
        indent.first_line = scale_text_unit_sp(indent.first_line, curve);
        indent.rest_line = scale_text_unit_sp(indent.rest_line, curve);
        scaled.paragraph_style.text_indent = Some(indent);
    }
    scaled
}

fn with_system_font_scale<R>(
    text: &crate::text::AnnotatedString,
    style: &TextStyle,
    block: impl FnOnce(&crate::text::AnnotatedString, &TextStyle) -> R,
) -> R {
    let curve = crate::current_font_scale_curve();
    let visual_style = scale_text_style_font_sizes(style, curve);
    let visual_text = scale_annotated_font_sizes(text, curve);
    block(visual_text.as_ref(), &visual_style)
}

fn scale_span_style_font_sizes(
    style: &crate::text::SpanStyle,
    curve: FontScaleCurve,
    default_font_size_sp: Option<f32>,
) -> crate::text::SpanStyle {
    let factor = curve.scale();
    let mut scaled = style.clone();
    scaled.font_size = match (style.font_size, default_font_size_sp) {
        (crate::text::TextUnit::Unspecified, Some(default_size)) => {
            crate::text::TextUnit::Sp(curve.sp_to_dp(default_size))
        }
        (unit, Some(_)) => scale_text_unit_sp_and_em(unit, curve),
        (unit, None) => scale_text_unit_sp(unit, curve),
    };
    scaled.letter_spacing = scale_text_unit_sp(scaled.letter_spacing, curve);
    if let Some(mut shadow) = scaled.shadow {
        shadow.offset.x = scale_finite_dimension(shadow.offset.x, factor);
        shadow.offset.y = scale_finite_dimension(shadow.offset.y, factor);
        shadow.blur_radius = scale_finite_dimension(shadow.blur_radius, factor);
        scaled.shadow = Some(shadow);
    }
    if let Some(crate::text::TextDrawStyle::Stroke { width }) = scaled.draw_style {
        scaled.draw_style = Some(crate::text::TextDrawStyle::Stroke {
            width: width * factor,
        });
    }
    scaled
}

fn annotated_text_needs_scaling(text: &crate::text::AnnotatedString) -> bool {
    text.span_styles
        .iter()
        .any(|span| span_style_needs_scaling(&span.item))
}

fn span_style_needs_scaling(style: &crate::text::SpanStyle) -> bool {
    matches!(style.font_size, crate::text::TextUnit::Sp(value) if value.is_finite())
        || matches!(style.letter_spacing, crate::text::TextUnit::Sp(value) if value.is_finite())
        || matches!(
            style.draw_style,
            Some(crate::text::TextDrawStyle::Stroke { .. })
        )
        || style.shadow.is_some()
}

fn scale_text_unit_sp(unit: crate::text::TextUnit, curve: FontScaleCurve) -> crate::text::TextUnit {
    match unit {
        crate::text::TextUnit::Sp(value) if value.is_finite() => {
            crate::text::TextUnit::Sp(curve.sp_to_dp(value))
        }
        other => other,
    }
}

fn scale_text_unit_sp_and_em(
    unit: crate::text::TextUnit,
    curve: FontScaleCurve,
) -> crate::text::TextUnit {
    match unit {
        crate::text::TextUnit::Sp(_) => scale_text_unit_sp(unit, curve),
        crate::text::TextUnit::Em(value) if value.is_finite() => {
            crate::text::TextUnit::Em(value * curve.scale())
        }
        other => other,
    }
}

fn scale_finite_dimension(value: f32, factor: f32) -> f32 {
    if value.is_finite() {
        value * factor
    } else {
        value
    }
}

#[derive(Clone, Debug)]
enum DisplayLineText {
    Source,
    Remapped(crate::text::AnnotatedString),
}

#[derive(Clone, Debug)]
struct DisplayLine {
    source_range: Range<usize>,
    text: DisplayLineText,
    measured_width: Option<f32>,
}

impl DisplayLine {
    fn from_source_range(source_range: Range<usize>) -> Self {
        Self {
            source_range,
            text: DisplayLineText::Source,
            measured_width: None,
        }
    }

    fn from_measured_source_range(source_range: Range<usize>, measured_width: f32) -> Self {
        Self {
            source_range,
            text: DisplayLineText::Source,
            measured_width: measured_width
                .is_finite()
                .then_some(measured_width.max(0.0)),
        }
    }

    fn display_text<'a>(&'a self, source: &'a crate::text::AnnotatedString) -> &'a str {
        match &self.text {
            DisplayLineText::Source => &source.text[self.source_range.clone()],
            DisplayLineText::Remapped(annotated) => annotated.text.as_str(),
        }
    }

    fn measure_width<M: TextMeasurer + ?Sized>(
        &self,
        measurer: &M,
        node_id: Option<NodeId>,
        source: &crate::text::AnnotatedString,
        style: &TextStyle,
    ) -> f32 {
        match &self.text {
            DisplayLineText::Source => self.measured_width.unwrap_or_else(|| {
                measurer
                    .measure_subsequence_for_node(node_id, source, self.source_range.clone(), style)
                    .width
            }),
            DisplayLineText::Remapped(annotated) => {
                measurer.measure_for_node(node_id, annotated, style).width
            }
        }
    }

    fn apply_display_text(&mut self, source: &crate::text::AnnotatedString, display_text: String) {
        let source_text = &source.text[self.source_range.clone()];
        self.measured_width = None;
        self.text = if source_text == display_text {
            DisplayLineText::Source
        } else {
            DisplayLineText::Remapped(remap_annotated_subsequence_for_display(
                source,
                self.source_range.clone(),
                display_text.as_str(),
            ))
        };
    }

    fn extend_to_paragraph_end(&mut self, source: &crate::text::AnnotatedString) {
        let start = self.source_range.start;
        let end = source.text[start..]
            .find('\n')
            .map_or(source.text.len(), |offset| start + offset);
        self.source_range = start..end;
        self.text = DisplayLineText::Source;
        self.measured_width = None;
    }

    fn ellipsize<M: TextMeasurer + ?Sized>(
        &mut self,
        measurer: &M,
        source: &crate::text::AnnotatedString,
        style: &TextStyle,
        max_width: Option<f32>,
        placement: EllipsisPlacement,
    ) {
        let ellipsized = fit_ellipsis(
            measurer,
            self.display_text(source),
            style,
            max_width,
            placement,
        );
        self.apply_display_text(source, ellipsized);
    }
}

fn split_line_ranges(text: &str) -> Vec<Range<usize>> {
    if text.is_empty() {
        return single_line_range(0..0);
    }

    let mut ranges = Vec::new();
    let mut start = 0usize;
    for (idx, ch) in text.char_indices() {
        if ch == '\n' {
            ranges.push(start..idx);
            start = idx + ch.len_utf8();
        }
    }
    ranges.push(start..text.len());
    ranges
}

fn build_display_annotated(
    source: &crate::text::AnnotatedString,
    lines: &[DisplayLine],
) -> crate::text::AnnotatedString {
    if lines.is_empty() {
        return crate::text::AnnotatedString::from("");
    }

    let mut builder = crate::text::AnnotatedString::builder();
    for (idx, line) in lines.iter().enumerate() {
        builder = match &line.text {
            DisplayLineText::Source => {
                builder.append_annotated_subsequence(source, line.source_range.clone())
            }
            DisplayLineText::Remapped(annotated) => builder.append_annotated(annotated),
        };
        if idx + 1 < lines.len() {
            builder = builder.append("\n");
        }
    }
    builder.to_annotated_string()
}

fn join_display_line_text(source: &crate::text::AnnotatedString, lines: &[DisplayLine]) -> String {
    let mut text = String::new();
    for (idx, line) in lines.iter().enumerate() {
        text.push_str(line.display_text(source));
        if idx + 1 < lines.len() {
            text.push('\n');
        }
    }
    text
}

fn trim_segment_end_whitespace(line: &str, start: usize, mut end: usize) -> usize {
    while end > start {
        let Some((idx, ch)) = line[start..end].char_indices().next_back() else {
            break;
        };
        if ch.is_whitespace() {
            end = start + idx;
        } else {
            break;
        }
    }
    end
}

fn remap_annotated_subsequence_for_display(
    source: &crate::text::AnnotatedString,
    source_range: Range<usize>,
    display_text: &str,
) -> crate::text::AnnotatedString {
    let source_text = &source.text[source_range.clone()];
    if source_text == display_text {
        return source.subsequence(source_range);
    }

    let display_chars = map_display_chars_to_source(source_text, display_text);
    crate::text::AnnotatedString {
        text: display_text.to_string(),
        span_styles: remap_subsequence_range_styles(
            &source.span_styles,
            source_range.clone(),
            &display_chars,
        ),
        paragraph_styles: remap_subsequence_range_styles(
            &source.paragraph_styles,
            source_range.clone(),
            &display_chars,
        ),
        string_annotations: remap_subsequence_range_styles(
            &source.string_annotations,
            source_range.clone(),
            &display_chars,
        ),
        link_annotations: remap_subsequence_range_styles(
            &source.link_annotations,
            source_range,
            &display_chars,
        ),
    }
}

#[derive(Clone, Copy)]
struct DisplayCharMap {
    display_start: usize,
    display_end: usize,
    source_start: Option<usize>,
}

fn map_display_chars_to_source(source: &str, display: &str) -> Vec<DisplayCharMap> {
    let source_chars: Vec<(usize, char)> = source.char_indices().collect();
    let mut source_index = 0usize;
    let mut maps = Vec::with_capacity(display.chars().count());

    for (display_start, display_char) in display.char_indices() {
        let display_end = display_start + display_char.len_utf8();
        let mut source_start = None;
        while source_index < source_chars.len() {
            let (candidate_start, candidate_char) = source_chars[source_index];
            source_index += 1;
            if candidate_char == display_char {
                source_start = Some(candidate_start);
                break;
            }
        }
        maps.push(DisplayCharMap {
            display_start,
            display_end,
            source_start,
        });
    }

    maps
}

fn remap_subsequence_range_styles<T: Clone>(
    styles: &[crate::text::RangeStyle<T>],
    source_range: Range<usize>,
    display_chars: &[DisplayCharMap],
) -> Vec<crate::text::RangeStyle<T>> {
    let mut remapped = Vec::new();

    for style in styles {
        let overlap_start = style.range.start.max(source_range.start);
        let overlap_end = style.range.end.min(source_range.end);
        if overlap_start >= overlap_end {
            continue;
        }
        let local_source_range =
            (overlap_start - source_range.start)..(overlap_end - source_range.start);
        let mut range_start = None;
        let mut range_end = 0usize;

        for map in display_chars {
            let in_range = map.source_start.is_some_and(|source_start| {
                source_start >= local_source_range.start && source_start < local_source_range.end
            });

            if in_range {
                if range_start.is_none() {
                    range_start = Some(map.display_start);
                }
                range_end = map.display_end;
                continue;
            }

            if let Some(start) = range_start.take()
                && start < range_end
            {
                remapped.push(crate::text::RangeStyle {
                    item: style.item.clone(),
                    range: start..range_end,
                });
            }
        }

        if let Some(start) = range_start.take()
            && start < range_end
        {
            remapped.push(crate::text::RangeStyle {
                item: style.item.clone(),
                range: start..range_end,
            });
        }
    }

    remapped
}

fn normalize_max_width(max_width: Option<f32>) -> Option<f32> {
    match max_width {
        Some(width) if width.is_finite() && width > 0.0 => Some(width),
        _ => None,
    }
}

fn absolute_range_from_start(base_start: usize, relative: Range<usize>) -> Range<usize> {
    (base_start + relative.start)..(base_start + relative.end)
}

fn boundary_index_for_byte(boundaries: &[usize], byte_offset: usize) -> usize {
    boundaries
        .binary_search(&byte_offset)
        .unwrap_or_else(|index| index.min(boundaries.len().saturating_sub(1)))
}

fn single_line_range(range: Range<usize>) -> Vec<Range<usize>> {
    std::iter::once(range).collect()
}

struct LineMeasureContext<'a, M: TextMeasurer + ?Sized> {
    measurer: &'a M,
    text: &'a crate::text::AnnotatedString,
    style: &'a TextStyle,
    line_start: usize,
    prefix_widths: Option<TextLinePrefixWidths>,
}

impl<'a, M: TextMeasurer + ?Sized> LineMeasureContext<'a, M> {
    fn new(
        measurer: &'a M,
        text: &'a crate::text::AnnotatedString,
        line_range: &Range<usize>,
        style: &'a TextStyle,
        boundary_count: usize,
    ) -> Self {
        let expected_chars = boundary_count.saturating_sub(1);
        let prefix_widths = measurer
            .measure_line_prefix_widths(text, line_range.clone(), style)
            .filter(|widths| widths.char_count() == expected_chars);
        Self {
            measurer,
            text,
            style,
            line_start: line_range.start,
            prefix_widths,
        }
    }

    fn measure_char_range(&self, boundaries: &[usize], start_idx: usize, end_idx: usize) -> f32 {
        if let Some(width) = self.prefix_width_for_char_range(start_idx, end_idx) {
            return width;
        }
        let segment_range =
            absolute_range_from_start(self.line_start, boundaries[start_idx]..boundaries[end_idx]);
        self.measurer
            .measure_subsequence(self.text, segment_range, self.style)
            .width
    }

    fn prefix_width_for_char_range(&self, start_idx: usize, end_idx: usize) -> Option<f32> {
        if let Some(prefix_widths) = &self.prefix_widths
            && let Some(width) = prefix_widths.width_for_char_range(start_idx, end_idx)
        {
            return Some(width);
        }
        None
    }

    fn display_line_for_char_range(
        &self,
        boundaries: &[usize],
        start_idx: usize,
        end_idx: usize,
    ) -> DisplayLine {
        let source_range =
            absolute_range_from_start(self.line_start, boundaries[start_idx]..boundaries[end_idx]);
        let measured_width = self.measure_char_range(boundaries, start_idx, end_idx);
        DisplayLine::from_measured_source_range(source_range, measured_width)
    }
}

fn wrap_line_to_width<M: TextMeasurer + ?Sized>(
    measurer: &M,
    text: &crate::text::AnnotatedString,
    line_range: Range<usize>,
    style: &TextStyle,
    max_width: f32,
    line_break: LineBreak,
    hyphens: Hyphens,
) -> Vec<DisplayLine> {
    let line_text = &text.text[line_range.clone()];
    if line_text.is_empty() {
        return vec![DisplayLine::from_source_range(
            line_range.start..line_range.start,
        )];
    }

    if let Some(measured_width) = measurer.measure_line_width(text, line_range.clone(), style)
        && measured_width <= max_width + WRAP_EPSILON
    {
        return vec![DisplayLine::from_measured_source_range(
            line_range,
            measured_width,
        )];
    }

    if matches!(line_break, LineBreak::Heading | LineBreak::Paragraph)
        && line_text.chars().any(char::is_whitespace)
        && let Some(balanced) = wrap_line_with_word_balance(
            measurer,
            text,
            line_range.clone(),
            style,
            max_width,
            line_break,
        )
    {
        return balanced;
    }

    wrap_line_greedy(
        measurer, text, line_range, style, max_width, line_break, hyphens,
    )
}

fn wrap_line_greedy<M: TextMeasurer + ?Sized>(
    measurer: &M,
    text: &crate::text::AnnotatedString,
    line_range: Range<usize>,
    style: &TextStyle,
    max_width: f32,
    line_break: LineBreak,
    hyphens: Hyphens,
) -> Vec<DisplayLine> {
    let line_text = &text.text[line_range.clone()];
    let boundaries = char_boundaries(line_text);
    let measure_context =
        LineMeasureContext::new(measurer, text, &line_range, style, boundaries.len());
    if let Some(measured_width) =
        measure_context.prefix_width_for_char_range(0, boundaries.len() - 1)
        && measured_width <= max_width + WRAP_EPSILON
    {
        return vec![DisplayLine::from_measured_source_range(
            line_range,
            measured_width,
        )];
    }
    let mut wrapped = Vec::new();
    let mut start_idx = 0usize;

    while start_idx < boundaries.len() - 1 {
        let mut low = start_idx + 1;
        let mut high = boundaries.len() - 1;
        let mut best = start_idx + 1;

        while low <= high {
            let mid = (low + high) / 2;
            let width = measure_context.measure_char_range(&boundaries, start_idx, mid);
            if width <= max_width + WRAP_EPSILON || mid == start_idx + 1 {
                best = mid;
                low = mid + 1;
            } else {
                if mid == 0 {
                    break;
                }
                high = mid - 1;
            }
        }

        let wrap_idx = choose_wrap_break(line_text, &boundaries, start_idx, best, line_break);
        let mut effective_wrap_idx = wrap_idx;
        let can_hyphenate = hyphens == Hyphens::Auto
            && wrap_idx == best
            && best < boundaries.len() - 1
            && is_break_inside_word(line_text, &boundaries, wrap_idx);
        if can_hyphenate {
            effective_wrap_idx = resolve_auto_hyphen_break(
                measurer,
                line_text,
                style,
                &boundaries,
                start_idx,
                wrap_idx,
            );
        }

        let broke_at_word_boundary = effective_wrap_idx > start_idx
            && line_text[boundaries[effective_wrap_idx - 1]..boundaries[effective_wrap_idx]]
                .chars()
                .all(char::is_whitespace);
        let segment_start = boundaries[start_idx];
        let mut segment_end = boundaries[effective_wrap_idx];
        if wrap_idx != best || broke_at_word_boundary {
            segment_end = trim_segment_end_whitespace(line_text, segment_start, segment_end);
        }
        let segment_end_idx = boundary_index_for_byte(&boundaries, segment_end);
        wrapped.push(measure_context.display_line_for_char_range(
            &boundaries,
            start_idx,
            segment_end_idx,
        ));

        start_idx = if wrap_idx != best || broke_at_word_boundary {
            skip_leading_whitespace(line_text, &boundaries, wrap_idx)
        } else {
            effective_wrap_idx
        };
    }

    if wrapped.is_empty() {
        wrapped.push(DisplayLine::from_source_range(
            line_range.start..line_range.start,
        ));
    }

    wrapped
}

fn wrap_line_with_word_balance<M: TextMeasurer + ?Sized>(
    measurer: &M,
    text: &crate::text::AnnotatedString,
    line_range: Range<usize>,
    style: &TextStyle,
    max_width: f32,
    line_break: LineBreak,
) -> Option<Vec<DisplayLine>> {
    let line_text = &text.text[line_range.clone()];
    let boundaries = char_boundaries(line_text);
    let measure_context =
        LineMeasureContext::new(measurer, text, &line_range, style, boundaries.len());
    if let Some(measured_width) =
        measure_context.prefix_width_for_char_range(0, boundaries.len() - 1)
        && measured_width <= max_width + WRAP_EPSILON
    {
        return Some(vec![DisplayLine::from_measured_source_range(
            line_range,
            measured_width,
        )]);
    }
    let breakpoints = collect_word_breakpoints(line_text, &boundaries);
    if breakpoints.len() <= 2 {
        return None;
    }

    let node_count = breakpoints.len();
    let mut best_cost = vec![f32::INFINITY; node_count];
    let mut next_index = vec![None; node_count];
    best_cost[node_count - 1] = 0.0;

    for start in (0..node_count - 1).rev() {
        for end in start + 1..node_count {
            let start_byte = boundaries[breakpoints[start]];
            let end_byte = boundaries[breakpoints[end]];
            let trimmed_end = trim_segment_end_whitespace(line_text, start_byte, end_byte);
            if trimmed_end <= start_byte {
                continue;
            }
            let segment_start_idx = breakpoints[start];
            let segment_end_idx = boundary_index_for_byte(&boundaries, trimmed_end);
            let segment_width =
                measure_context.measure_char_range(&boundaries, segment_start_idx, segment_end_idx);
            if segment_width > max_width + WRAP_EPSILON {
                continue;
            }
            if !best_cost[end].is_finite() {
                continue;
            }
            let slack = (max_width - segment_width).max(0.0);
            let is_last = end == node_count - 1;
            let segment_cost = match line_break {
                LineBreak::Heading => slack * slack,
                LineBreak::Paragraph => {
                    if is_last {
                        slack * slack * 0.16
                    } else {
                        slack * slack
                    }
                }
                LineBreak::Simple | LineBreak::Unspecified => slack * slack,
            };
            let candidate = segment_cost + best_cost[end];
            if candidate < best_cost[start] {
                best_cost[start] = candidate;
                next_index[start] = Some(end);
            }
        }
    }

    let mut wrapped = Vec::new();
    let mut current = 0usize;
    while current < node_count - 1 {
        let next = next_index[current]?;
        let start_byte = boundaries[breakpoints[current]];
        let end_byte = boundaries[breakpoints[next]];
        let trimmed_end = trim_segment_end_whitespace(line_text, start_byte, end_byte);
        if trimmed_end <= start_byte {
            return None;
        }
        let segment_start_idx = breakpoints[current];
        let segment_end_idx = boundary_index_for_byte(&boundaries, trimmed_end);
        wrapped.push(measure_context.display_line_for_char_range(
            &boundaries,
            segment_start_idx,
            segment_end_idx,
        ));
        current = next;
    }

    if wrapped.is_empty() {
        return None;
    }

    Some(wrapped)
}

fn collect_word_breakpoints(line: &str, boundaries: &[usize]) -> Vec<usize> {
    let mut points = vec![0usize];
    for idx in 1..boundaries.len() - 1 {
        let prev = &line[boundaries[idx - 1]..boundaries[idx]];
        let current = &line[boundaries[idx]..boundaries[idx + 1]];
        if prev.chars().all(char::is_whitespace) && !current.chars().all(char::is_whitespace) {
            points.push(idx);
        }
    }
    let end = boundaries.len() - 1;
    if points.last().copied() != Some(end) {
        points.push(end);
    }
    points
}

fn choose_wrap_break(
    line: &str,
    boundaries: &[usize],
    start_idx: usize,
    best: usize,
    _line_break: LineBreak,
) -> usize {
    if best >= boundaries.len() - 1 {
        return best;
    }

    if best <= start_idx + 1 {
        return best;
    }

    for idx in (start_idx + 1..=best).rev() {
        let prev = &line[boundaries[idx - 1]..boundaries[idx]];
        if prev.chars().all(char::is_whitespace) {
            return idx;
        }
    }
    best
}

fn is_break_inside_word(line: &str, boundaries: &[usize], break_idx: usize) -> bool {
    if break_idx == 0 || break_idx >= boundaries.len() - 1 {
        return false;
    }
    let prev = &line[boundaries[break_idx - 1]..boundaries[break_idx]];
    let next = &line[boundaries[break_idx]..boundaries[break_idx + 1]];
    !prev.chars().all(char::is_whitespace) && !next.chars().all(char::is_whitespace)
}

fn resolve_auto_hyphen_break<M: TextMeasurer + ?Sized>(
    measurer: &M,
    line: &str,
    style: &TextStyle,
    boundaries: &[usize],
    start_idx: usize,
    break_idx: usize,
) -> usize {
    if let Some(candidate) = measurer.choose_auto_hyphen_break(line, style, start_idx, break_idx)
        && is_valid_auto_hyphen_break(line, boundaries, start_idx, break_idx, candidate)
    {
        return candidate;
    }
    choose_auto_hyphen_break_fallback(boundaries, start_idx, break_idx)
}

fn is_valid_auto_hyphen_break(
    line: &str,
    boundaries: &[usize],
    start_idx: usize,
    break_idx: usize,
    candidate_idx: usize,
) -> bool {
    let end_idx = boundaries.len().saturating_sub(1);
    candidate_idx > start_idx
        && candidate_idx < end_idx
        && candidate_idx <= break_idx
        && candidate_idx >= start_idx + AUTO_HYPHEN_MIN_SEGMENT_CHARS
        && is_break_inside_word(line, boundaries, candidate_idx)
}

fn choose_auto_hyphen_break_fallback(
    boundaries: &[usize],
    start_idx: usize,
    break_idx: usize,
) -> usize {
    let end_idx = boundaries.len().saturating_sub(1);
    if break_idx >= end_idx {
        return break_idx;
    }
    let trailing_len = end_idx.saturating_sub(break_idx);
    if trailing_len > 2 || break_idx <= start_idx + AUTO_HYPHEN_MIN_SEGMENT_CHARS {
        return break_idx;
    }

    let min_break = start_idx + AUTO_HYPHEN_MIN_SEGMENT_CHARS;
    let max_break = break_idx.saturating_sub(1);
    if min_break > max_break {
        return break_idx;
    }

    let mut best_break = break_idx;
    let mut best_penalty = usize::MAX;
    for idx in min_break..=max_break {
        let candidate_trailing_len = end_idx.saturating_sub(idx);
        let candidate_prefix_len = idx.saturating_sub(start_idx);
        if candidate_prefix_len < AUTO_HYPHEN_MIN_SEGMENT_CHARS
            || candidate_trailing_len < AUTO_HYPHEN_MIN_TRAILING_CHARS
        {
            continue;
        }

        let penalty = candidate_trailing_len.abs_diff(AUTO_HYPHEN_PREFERRED_TRAILING_CHARS);
        if penalty < best_penalty {
            best_penalty = penalty;
            best_break = idx;
            if penalty == 0 {
                break;
            }
        }
    }
    best_break
}

fn skip_leading_whitespace(line: &str, boundaries: &[usize], mut idx: usize) -> usize {
    while idx < boundaries.len() - 1 {
        let ch = &line[boundaries[idx]..boundaries[idx + 1]];
        if !ch.chars().all(char::is_whitespace) {
            break;
        }
        idx += 1;
    }
    idx
}

fn measured_width<M: TextMeasurer + ?Sized>(measurer: &M, text: &str, style: &TextStyle) -> f32 {
    measurer
        .measure(&crate::text::AnnotatedString::from(text), style)
        .width
}

fn apply_overflow<M: TextMeasurer + ?Sized>(
    measurer: &M,
    node_id: Option<NodeId>,
    text: &crate::text::AnnotatedString,
    style: &TextStyle,
    options: TextLayoutOptions,
    max_width: Option<f32>,
    visible_lines: &mut Vec<DisplayLine>,
) -> bool {
    if options.overflow == TextOverflow::Visible {
        return false;
    }
    let ellipsis = EllipsisPlacement::for_options(options);
    let mut did_overflow = false;
    if visible_lines.len() > options.max_lines {
        did_overflow = true;
        visible_lines.truncate(options.max_lines);
        if let (Some(placement), Some(last_line)) = (ellipsis, visible_lines.last_mut()) {
            last_line.extend_to_paragraph_end(text);
            last_line.ellipsize(measurer, text, style, max_width, placement);
        }
    }

    let Some(width_limit) = max_width else {
        return did_overflow;
    };
    let visible_len = visible_lines.len();
    for (line_index, line) in visible_lines.iter_mut().enumerate() {
        if line.measure_width(measurer, node_id, text, style) <= width_limit + WRAP_EPSILON {
            continue;
        }
        did_overflow = true;
        if line_index + 1 == visible_len
            && let Some(placement) = ellipsis
        {
            line.ellipsize(measurer, text, style, max_width, placement);
        }
    }
    did_overflow
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum EllipsisPlacement {
    End,
    Start,
    Middle,
}

impl EllipsisPlacement {
    fn for_options(options: TextLayoutOptions) -> Option<Self> {
        let single_line = options.max_lines == 1 || !options.soft_wrap;
        match options.overflow {
            TextOverflow::Ellipsis => Some(Self::End),
            TextOverflow::StartEllipsis if single_line => Some(Self::Start),
            TextOverflow::MiddleEllipsis if single_line => Some(Self::Middle),
            TextOverflow::StartEllipsis
            | TextOverflow::MiddleEllipsis
            | TextOverflow::Clip
            | TextOverflow::Visible
            | TextOverflow::ScaleDown { .. } => None,
        }
    }

    fn elide(self, line: &str, boundaries: &[usize], kept_chars: usize) -> String {
        let char_count = boundaries.len() - 1;
        match self {
            Self::End => format!("{}{ELLIPSIS}", &line[..boundaries[kept_chars]]),
            Self::Start => format!("{ELLIPSIS}{}", &line[boundaries[char_count - kept_chars]..]),
            Self::Middle => format!(
                "{}{ELLIPSIS}{}",
                &line[..boundaries[kept_chars.div_ceil(2)]],
                &line[boundaries[char_count - kept_chars / 2]..]
            ),
        }
    }
}

fn fit_ellipsis<M: TextMeasurer + ?Sized>(
    measurer: &M,
    line: &str,
    style: &TextStyle,
    max_width: Option<f32>,
    placement: EllipsisPlacement,
) -> String {
    let width_limit = max_width.unwrap_or(f32::INFINITY);
    let fits =
        |candidate: &str| measured_width(measurer, candidate, style) <= width_limit + WRAP_EPSILON;
    if placement != EllipsisPlacement::End && fits(line) {
        return line.to_string();
    }
    if !fits(ELLIPSIS) {
        return String::new();
    }

    let boundaries = char_boundaries(line);
    let mut fitting = 0usize;
    let mut overflowing = boundaries.len();
    while fitting + 1 < overflowing {
        let kept_chars = fitting + (overflowing - fitting) / 2;
        if fits(&placement.elide(line, &boundaries, kept_chars)) {
            fitting = kept_chars;
        } else {
            overflowing = kept_chars;
        }
    }
    placement.elide(line, &boundaries, fitting)
}

fn char_boundaries(text: &str) -> Vec<usize> {
    let mut out = Vec::with_capacity(text.chars().count() + 1);
    out.push(0);
    for (idx, _) in text.char_indices() {
        if idx != 0 {
            out.push(idx);
        }
    }
    out.push(text.len());
    out
}

#[cfg(test)]
#[path = "tests/measure_tests.rs"]
mod tests;
