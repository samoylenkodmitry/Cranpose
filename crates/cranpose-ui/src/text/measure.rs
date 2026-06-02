use crate::text_layout_result::TextLayoutResult;
use cranpose_core::NodeId;
use std::borrow::Cow;
use std::cell::RefCell;
use std::collections::{hash_map::Entry, HashMap, VecDeque};
use std::hash::Hash;
use std::ops::Range;
use std::rc::Rc;

use super::layout_options::{TextLayoutOptions, TextOverflow};
use super::paragraph::{Hyphens, LineBreak};
use super::style::TextStyle;

const ELLIPSIS: &str = "\u{2026}";
const DEFAULT_FONT_SIZE_SP: f32 = 14.0;
const WRAP_EPSILON: f32 = 0.5;
const SCALE_DOWN_SEARCH_STEPS: usize = 14;
const AUTO_HYPHEN_MIN_SEGMENT_CHARS: usize = 2;
const AUTO_HYPHEN_MIN_TRAILING_CHARS: usize = 3;
const AUTO_HYPHEN_PREFERRED_TRAILING_CHARS: usize = 4;
const TEXT_SERVICE_CACHE_CAPACITY: usize = 256;

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
    pub text: crate::text::AnnotatedString,
    pub visual_style: TextStyle,
    pub metrics: TextMetrics,
    pub did_overflow: bool,
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
    const CHAR_WIDTH_RATIO: f32 = 0.6; // Width is 0.6 of Height

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
            .map(|(i, _)| i)
            .unwrap_or(line_text.len());

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
    node_id: Option<NodeId>,
    text_hash: u64,
    style_hash: u64,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
struct TextOptionsCacheKey {
    base: TextBaseCacheKey,
    options: TextLayoutOptions,
    max_width_bits: Option<u32>,
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
    measurer: RefCell<Rc<dyn TextMeasurer>>,
    metrics_cache: RefCell<BoundedTextCache<TextBaseCacheKey, TextMetrics>>,
    options_metrics_cache: RefCell<BoundedTextCache<TextOptionsCacheKey, TextMetrics>>,
    prepared_cache: RefCell<BoundedTextCache<TextOptionsCacheKey, PreparedTextLayout>>,
    layout_cache: RefCell<BoundedTextCache<TextBaseCacheKey, TextLayoutResult>>,
}

impl TextService {
    pub(crate) fn new() -> Self {
        Self::from_measurer(Rc::new(MonospacedTextMeasurer))
    }

    pub(crate) fn from_measurer(measurer: Rc<dyn TextMeasurer>) -> Self {
        Self {
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
        let key = text_base_cache_key(node_id, text, style);
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
        let key = text_options_cache_key(node_id, text, style, options.normalized(), max_width);
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
        let key = text_options_cache_key(node_id, text, style, options.normalized(), max_width);
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
            .insert(key, prepared.metrics);
        prepared
    }

    pub(crate) fn layout(
        &self,
        text: &crate::text::AnnotatedString,
        style: &TextStyle,
    ) -> TextLayoutResult {
        let key = text_base_cache_key(None, text, style);
        if let Some(layout) = self.layout_cache.borrow().get(&key) {
            return layout;
        }
        let layout = self.with_measurer(|m| m.layout(text, style));
        self.layout_cache.borrow_mut().insert(key, layout.clone());
        layout
    }

    fn clear_caches(&self) {
        self.metrics_cache.borrow_mut().clear();
        self.options_metrics_cache.borrow_mut().clear();
        self.prepared_cache.borrow_mut().clear();
        self.layout_cache.borrow_mut().clear();
    }
}

fn text_base_cache_key(
    node_id: Option<NodeId>,
    text: &crate::text::AnnotatedString,
    style: &TextStyle,
) -> TextBaseCacheKey {
    TextBaseCacheKey {
        node_id,
        text_hash: text.render_hash(),
        style_hash: style.measurement_hash(),
    }
}

fn text_options_cache_key(
    node_id: Option<NodeId>,
    text: &crate::text::AnnotatedString,
    style: &TextStyle,
    options: TextLayoutOptions,
    max_width: Option<f32>,
) -> TextOptionsCacheKey {
    TextOptionsCacheKey {
        base: text_base_cache_key(node_id, text, style),
        options: options.normalized(),
        max_width_bits: normalize_max_width(max_width).map(f32::to_bits),
    }
}

pub fn set_text_measurer<M: TextMeasurer>(measurer: M) {
    crate::render_state::set_current_text_measurer(Rc::new(measurer));
}

pub fn measure_text(text: &crate::text::AnnotatedString, style: &TextStyle) -> TextMetrics {
    crate::render_state::with_text_service(|service| service.measure(None, text, style))
}

pub fn measure_text_for_node(
    node_id: Option<NodeId>,
    text: &crate::text::AnnotatedString,
    style: &TextStyle,
) -> TextMetrics {
    crate::render_state::with_text_service(|service| service.measure(node_id, text, style))
}

pub fn measure_text_with_options(
    text: &crate::text::AnnotatedString,
    style: &TextStyle,
    options: TextLayoutOptions,
    max_width: Option<f32>,
) -> TextMetrics {
    crate::render_state::with_text_service(|service| {
        service.measure_with_options(None, text, style, options.normalized(), max_width)
    })
}

pub fn measure_text_with_options_for_node(
    node_id: Option<NodeId>,
    text: &crate::text::AnnotatedString,
    style: &TextStyle,
    options: TextLayoutOptions,
    max_width: Option<f32>,
) -> TextMetrics {
    crate::render_state::with_text_service(|service| {
        service.measure_with_options(node_id, text, style, options.normalized(), max_width)
    })
}

pub fn prepare_text_layout(
    text: &crate::text::AnnotatedString,
    style: &TextStyle,
    options: TextLayoutOptions,
    max_width: Option<f32>,
) -> PreparedTextLayout {
    crate::render_state::with_text_service(|service| {
        service.prepare_with_options(None, text, style, options.normalized(), max_width)
    })
}

pub fn prepare_text_layout_for_node(
    node_id: Option<NodeId>,
    text: &crate::text::AnnotatedString,
    style: &TextStyle,
    options: TextLayoutOptions,
    max_width: Option<f32>,
) -> PreparedTextLayout {
    crate::render_state::with_text_service(|service| {
        service.prepare_with_options(node_id, text, style, options.normalized(), max_width)
    })
}

pub fn get_offset_for_position(
    text: &crate::text::AnnotatedString,
    style: &TextStyle,
    x: f32,
    y: f32,
) -> usize {
    crate::render_state::with_text_measurer(|m| m.get_offset_for_position(text, style, x, y))
}

pub fn get_cursor_x_for_offset(
    text: &crate::text::AnnotatedString,
    style: &TextStyle,
    offset: usize,
) -> f32 {
    crate::render_state::with_text_measurer(|m| m.get_cursor_x_for_offset(text, style, offset))
}

pub fn layout_text(text: &crate::text::AnnotatedString, style: &TextStyle) -> TextLayoutResult {
    crate::render_state::with_text_service(|service| service.layout(text, style))
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

    let mut line_ranges = split_line_ranges(text.text.as_str());
    if let Some(width_limit) = wrap_width {
        let mut wrapped_ranges = Vec::with_capacity(line_ranges.len());
        for line_range in line_ranges.drain(..) {
            let wrapped_line_ranges = wrap_line_to_width(
                measurer,
                text,
                line_range,
                style,
                width_limit,
                line_break_mode,
                hyphens_mode,
            );
            wrapped_ranges.extend(wrapped_line_ranges);
        }
        line_ranges = wrapped_ranges;
    }

    let mut did_overflow = false;
    let mut visible_lines: Vec<DisplayLine> = line_ranges
        .into_iter()
        .map(DisplayLine::from_source_range)
        .collect();

    if opts.overflow != TextOverflow::Visible && visible_lines.len() > opts.max_lines {
        did_overflow = true;
        visible_lines.truncate(opts.max_lines);
        if let Some(last_line) = visible_lines.last_mut() {
            let overflowed = apply_line_overflow(
                measurer,
                last_line.display_text(text),
                style,
                max_width,
                opts,
                true,
                true,
            );
            last_line.apply_display_text(text, overflowed);
        }
    }

    if let Some(width_limit) = max_width {
        let single_line_ellipsis = opts.max_lines == 1 || !opts.soft_wrap;
        let visible_len = visible_lines.len();
        for (line_index, line) in visible_lines.iter_mut().enumerate() {
            let width = line.measure_width(measurer, node_id, text, style);
            if width > width_limit + WRAP_EPSILON {
                if opts.overflow == TextOverflow::Visible {
                    continue;
                }
                did_overflow = true;
                let overflowed = apply_line_overflow(
                    measurer,
                    line.display_text(text),
                    style,
                    Some(width_limit),
                    opts,
                    line_index + 1 == visible_len,
                    single_line_ellipsis,
                );
                line.apply_display_text(text, overflowed);
            }
        }
    }

    let display_annotated = build_display_annotated(text, &visible_lines);
    debug_assert_eq!(
        display_annotated.text,
        join_display_line_text(text, &visible_lines)
    );
    let line_height = measurer
        .measure_for_node(node_id, text, style)
        .line_height
        .max(0.0);
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
    let width = if opts.overflow == TextOverflow::Visible {
        measured_width
    } else if let Some(width_limit) = max_width {
        measured_width.min(width_limit)
    } else {
        measured_width
    };

    PreparedTextLayout {
        text: display_annotated,
        visual_style: style.clone(),
        metrics: TextMetrics {
            width,
            height: layout_line_count as f32 * line_height,
            line_height,
            line_count: layout_line_count,
        },
        did_overflow,
    }
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
        1.0,
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
        min_scale,
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
            mid,
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
    font_scale: f32,
) -> PreparedTextLayout {
    let visual_style = scale_text_style_font_sizes(style, font_scale);
    let visual_text = scale_annotated_font_sizes(text, font_scale);
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
    factor: f32,
) -> Cow<'_, crate::text::AnnotatedString> {
    if is_identity_scale(factor) || !annotated_text_needs_scaling(text) {
        return Cow::Borrowed(text);
    }

    let mut scaled = text.clone();
    for span in &mut scaled.span_styles {
        span.item = scale_span_style_font_sizes(&span.item, factor, None);
    }
    Cow::Owned(scaled)
}

fn scale_text_style_font_sizes(style: &TextStyle, factor: f32) -> TextStyle {
    if is_identity_scale(factor) {
        return style.clone();
    }

    let mut scaled = style.clone();
    scaled.span_style =
        scale_span_style_font_sizes(&style.span_style, factor, Some(DEFAULT_FONT_SIZE_SP));
    scaled.paragraph_style.line_height =
        scale_text_unit_sp(scaled.paragraph_style.line_height, factor);
    if let Some(mut indent) = scaled.paragraph_style.text_indent {
        indent.first_line = scale_text_unit_sp(indent.first_line, factor);
        indent.rest_line = scale_text_unit_sp(indent.rest_line, factor);
        scaled.paragraph_style.text_indent = Some(indent);
    }
    scaled
}

fn scale_span_style_font_sizes(
    style: &crate::text::SpanStyle,
    factor: f32,
    default_font_size_sp: Option<f32>,
) -> crate::text::SpanStyle {
    let mut scaled = style.clone();
    scaled.font_size = match (style.font_size, default_font_size_sp) {
        (crate::text::TextUnit::Unspecified, Some(default_size)) => {
            crate::text::TextUnit::Sp(default_size * factor)
        }
        (unit, Some(_)) => scale_text_unit_sp_and_em(unit, factor),
        (unit, None) => scale_text_unit_sp(unit, factor),
    };
    scaled.letter_spacing = scale_text_unit_sp(scaled.letter_spacing, factor);
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

fn scale_text_unit_sp(unit: crate::text::TextUnit, factor: f32) -> crate::text::TextUnit {
    match unit {
        crate::text::TextUnit::Sp(value) if value.is_finite() => {
            crate::text::TextUnit::Sp(value * factor)
        }
        other => other,
    }
}

fn scale_text_unit_sp_and_em(unit: crate::text::TextUnit, factor: f32) -> crate::text::TextUnit {
    match unit {
        crate::text::TextUnit::Sp(value) if value.is_finite() => {
            crate::text::TextUnit::Sp(value * factor)
        }
        crate::text::TextUnit::Em(value) if value.is_finite() => {
            crate::text::TextUnit::Em(value * factor)
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

fn is_identity_scale(factor: f32) -> bool {
    (factor - 1.0).abs() <= f32::EPSILON
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
}

impl DisplayLine {
    fn from_source_range(source_range: Range<usize>) -> Self {
        Self {
            source_range,
            text: DisplayLineText::Source,
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
            DisplayLineText::Source => {
                measurer
                    .measure_subsequence_for_node(node_id, source, self.source_range.clone(), style)
                    .width
            }
            DisplayLineText::Remapped(annotated) => {
                measurer.measure_for_node(node_id, annotated, style).width
            }
        }
    }

    fn apply_display_text(&mut self, source: &crate::text::AnnotatedString, display_text: String) {
        let source_text = &source.text[self.source_range.clone()];
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

            if let Some(start) = range_start.take() {
                if start < range_end {
                    remapped.push(crate::text::RangeStyle {
                        item: style.item.clone(),
                        range: start..range_end,
                    });
                }
            }
        }

        if let Some(start) = range_start.take() {
            if start < range_end {
                remapped.push(crate::text::RangeStyle {
                    item: style.item.clone(),
                    range: start..range_end,
                });
            }
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

fn absolute_range(base: &Range<usize>, relative: Range<usize>) -> Range<usize> {
    (base.start + relative.start)..(base.start + relative.end)
}

fn single_line_range(range: Range<usize>) -> Vec<Range<usize>> {
    std::iter::once(range).collect()
}

fn wrap_line_to_width<M: TextMeasurer + ?Sized>(
    measurer: &M,
    text: &crate::text::AnnotatedString,
    line_range: Range<usize>,
    style: &TextStyle,
    max_width: f32,
    line_break: LineBreak,
    hyphens: Hyphens,
) -> Vec<Range<usize>> {
    let line_text = &text.text[line_range.clone()];
    if line_text.is_empty() {
        return single_line_range(line_range.start..line_range.start);
    }

    if matches!(line_break, LineBreak::Heading | LineBreak::Paragraph)
        && line_text.chars().any(char::is_whitespace)
    {
        if let Some(balanced) = wrap_line_with_word_balance(
            measurer,
            text,
            line_range.clone(),
            style,
            max_width,
            line_break,
        ) {
            return balanced;
        }
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
) -> Vec<Range<usize>> {
    let line_text = &text.text[line_range.clone()];
    let boundaries = char_boundaries(line_text);
    let mut wrapped = Vec::new();
    let mut start_idx = 0usize;

    while start_idx < boundaries.len() - 1 {
        let mut low = start_idx + 1;
        let mut high = boundaries.len() - 1;
        let mut best = start_idx + 1;

        while low <= high {
            let mid = (low + high) / 2;
            let segment_range = absolute_range(&line_range, boundaries[start_idx]..boundaries[mid]);
            let width = measurer
                .measure_subsequence(text, segment_range, style)
                .width;
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

        let segment_start = boundaries[start_idx];
        let mut segment_end = boundaries[effective_wrap_idx];
        if wrap_idx != best {
            segment_end = trim_segment_end_whitespace(line_text, segment_start, segment_end);
        }
        wrapped.push(absolute_range(&line_range, segment_start..segment_end));

        start_idx = if wrap_idx != best {
            skip_leading_whitespace(line_text, &boundaries, wrap_idx)
        } else {
            effective_wrap_idx
        };
    }

    if wrapped.is_empty() {
        wrapped.push(line_range.start..line_range.start);
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
) -> Option<Vec<Range<usize>>> {
    let line_text = &text.text[line_range.clone()];
    let boundaries = char_boundaries(line_text);
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
            let segment_range = absolute_range(&line_range, start_byte..trimmed_end);
            let segment_width = measurer
                .measure_subsequence(text, segment_range, style)
                .width;
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
        wrapped.push(absolute_range(&line_range, start_byte..trimmed_end));
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

    for idx in (start_idx + 1..best).rev() {
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
    if let Some(candidate) = measurer.choose_auto_hyphen_break(line, style, start_idx, break_idx) {
        if is_valid_auto_hyphen_break(line, boundaries, start_idx, break_idx, candidate) {
            return candidate;
        }
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

fn apply_line_overflow<M: TextMeasurer + ?Sized>(
    measurer: &M,
    line: &str,
    style: &TextStyle,
    max_width: Option<f32>,
    options: TextLayoutOptions,
    is_last_visible_line: bool,
    single_line_ellipsis: bool,
) -> String {
    if options.overflow == TextOverflow::Clip || !is_last_visible_line {
        return line.to_string();
    }

    let Some(width_limit) = max_width else {
        return match options.overflow {
            TextOverflow::Ellipsis => format!("{line}{ELLIPSIS}"),
            TextOverflow::StartEllipsis => format!("{ELLIPSIS}{line}"),
            TextOverflow::MiddleEllipsis => format!("{ELLIPSIS}{line}"),
            TextOverflow::Clip | TextOverflow::Visible | TextOverflow::ScaleDown { .. } => {
                line.to_string()
            }
        };
    };

    match options.overflow {
        TextOverflow::Clip | TextOverflow::Visible => line.to_string(),
        TextOverflow::Ellipsis => fit_end_ellipsis(measurer, line, style, width_limit),
        TextOverflow::StartEllipsis => {
            if single_line_ellipsis {
                fit_start_ellipsis(measurer, line, style, width_limit)
            } else {
                line.to_string()
            }
        }
        TextOverflow::MiddleEllipsis => {
            if single_line_ellipsis {
                fit_middle_ellipsis(measurer, line, style, width_limit)
            } else {
                line.to_string()
            }
        }
        TextOverflow::ScaleDown { .. } => line.to_string(),
    }
}

fn fit_end_ellipsis<M: TextMeasurer + ?Sized>(
    measurer: &M,
    line: &str,
    style: &TextStyle,
    max_width: f32,
) -> String {
    if measurer
        .measure(&crate::text::AnnotatedString::from(line), style)
        .width
        <= max_width + WRAP_EPSILON
    {
        return line.to_string();
    }

    let ellipsis_width = measurer
        .measure(&crate::text::AnnotatedString::from(ELLIPSIS), style)
        .width;
    if ellipsis_width > max_width + WRAP_EPSILON {
        return String::new();
    }

    let boundaries = char_boundaries(line);
    let mut low = 0usize;
    let mut high = boundaries.len() - 1;
    let mut best = 0usize;

    while low <= high {
        let mid = (low + high) / 2;
        let prefix = &line[..boundaries[mid]];
        let candidate = format!("{prefix}{ELLIPSIS}");
        let width = measurer
            .measure(
                &crate::text::AnnotatedString::from(candidate.as_str()),
                style,
            )
            .width;
        if width <= max_width + WRAP_EPSILON {
            best = mid;
            low = mid + 1;
        } else if mid == 0 {
            break;
        } else {
            high = mid - 1;
        }
    }

    format!("{}{}", &line[..boundaries[best]], ELLIPSIS)
}

fn fit_start_ellipsis<M: TextMeasurer + ?Sized>(
    measurer: &M,
    line: &str,
    style: &TextStyle,
    max_width: f32,
) -> String {
    if measurer
        .measure(&crate::text::AnnotatedString::from(line), style)
        .width
        <= max_width + WRAP_EPSILON
    {
        return line.to_string();
    }

    let ellipsis_width = measurer
        .measure(&crate::text::AnnotatedString::from(ELLIPSIS), style)
        .width;
    if ellipsis_width > max_width + WRAP_EPSILON {
        return String::new();
    }

    let boundaries = char_boundaries(line);
    let mut low = 0usize;
    let mut high = boundaries.len() - 1;
    let mut best = boundaries.len() - 1;

    while low <= high {
        let mid = (low + high) / 2;
        let suffix = &line[boundaries[mid]..];
        let candidate = format!("{ELLIPSIS}{suffix}");
        let width = measurer
            .measure(
                &crate::text::AnnotatedString::from(candidate.as_str()),
                style,
            )
            .width;
        if width <= max_width + WRAP_EPSILON {
            best = mid;
            if mid == 0 {
                break;
            }
            high = mid - 1;
        } else {
            low = mid + 1;
        }
    }

    format!("{ELLIPSIS}{}", &line[boundaries[best]..])
}

fn fit_middle_ellipsis<M: TextMeasurer + ?Sized>(
    measurer: &M,
    line: &str,
    style: &TextStyle,
    max_width: f32,
) -> String {
    if measurer
        .measure(&crate::text::AnnotatedString::from(line), style)
        .width
        <= max_width + WRAP_EPSILON
    {
        return line.to_string();
    }

    let ellipsis_width = measurer
        .measure(&crate::text::AnnotatedString::from(ELLIPSIS), style)
        .width;
    if ellipsis_width > max_width + WRAP_EPSILON {
        return String::new();
    }

    let boundaries = char_boundaries(line);
    let total_chars = boundaries.len().saturating_sub(1);
    for keep in (0..=total_chars).rev() {
        let keep_start = keep.div_ceil(2);
        let keep_end = keep / 2;
        let start = &line[..boundaries[keep_start]];
        let end_start = boundaries[total_chars.saturating_sub(keep_end)];
        let end = &line[end_start..];
        let candidate = format!("{start}{ELLIPSIS}{end}");
        if measurer
            .measure(
                &crate::text::AnnotatedString::from(candidate.as_str()),
                style,
            )
            .width
            <= max_width + WRAP_EPSILON
        {
            return candidate;
        }
    }

    ELLIPSIS.to_string()
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
mod tests {
    use super::*;
    use crate::text::{Hyphens, LineBreak, ParagraphStyle, TextUnit};
    use crate::text_layout_result::TextLayoutResult;
    use std::cell::Cell;

    struct ContractBreakMeasurer {
        retreat: usize,
    }

    impl TextMeasurer for ContractBreakMeasurer {
        fn measure(&self, text: &crate::text::AnnotatedString, style: &TextStyle) -> TextMetrics {
            MonospacedTextMeasurer.measure(
                &crate::text::AnnotatedString::from(text.text.as_str()),
                style,
            )
        }

        fn get_offset_for_position(
            &self,
            text: &crate::text::AnnotatedString,
            style: &TextStyle,
            x: f32,
            y: f32,
        ) -> usize {
            MonospacedTextMeasurer.get_offset_for_position(
                &crate::text::AnnotatedString::from(text.text.as_str()),
                style,
                x,
                y,
            )
        }

        fn get_cursor_x_for_offset(
            &self,
            text: &crate::text::AnnotatedString,
            style: &TextStyle,
            offset: usize,
        ) -> f32 {
            MonospacedTextMeasurer.get_cursor_x_for_offset(
                &crate::text::AnnotatedString::from(text.text.as_str()),
                style,
                offset,
            )
        }

        fn layout(
            &self,
            text: &crate::text::AnnotatedString,
            style: &TextStyle,
        ) -> TextLayoutResult {
            MonospacedTextMeasurer.layout(
                &crate::text::AnnotatedString::from(text.text.as_str()),
                style,
            )
        }

        fn choose_auto_hyphen_break(
            &self,
            _line: &str,
            _style: &TextStyle,
            _segment_start_char: usize,
            measured_break_char: usize,
        ) -> Option<usize> {
            measured_break_char.checked_sub(self.retreat)
        }
    }

    struct CountingTextMeasurer {
        measure_calls: Rc<Cell<usize>>,
        layout_calls: Rc<Cell<usize>>,
    }

    impl CountingTextMeasurer {
        fn new(measure_calls: Rc<Cell<usize>>, layout_calls: Rc<Cell<usize>>) -> Self {
            Self {
                measure_calls,
                layout_calls,
            }
        }
    }

    impl TextMeasurer for CountingTextMeasurer {
        fn measure(&self, text: &crate::text::AnnotatedString, style: &TextStyle) -> TextMetrics {
            self.measure_calls.set(self.measure_calls.get() + 1);
            MonospacedTextMeasurer.measure(text, style)
        }

        fn get_offset_for_position(
            &self,
            text: &crate::text::AnnotatedString,
            style: &TextStyle,
            x: f32,
            y: f32,
        ) -> usize {
            MonospacedTextMeasurer.get_offset_for_position(text, style, x, y)
        }

        fn get_cursor_x_for_offset(
            &self,
            text: &crate::text::AnnotatedString,
            style: &TextStyle,
            offset: usize,
        ) -> f32 {
            MonospacedTextMeasurer.get_cursor_x_for_offset(text, style, offset)
        }

        fn layout(
            &self,
            text: &crate::text::AnnotatedString,
            style: &TextStyle,
        ) -> TextLayoutResult {
            self.layout_calls.set(self.layout_calls.get() + 1);
            MonospacedTextMeasurer.layout(text, style)
        }
    }

    #[test]
    fn text_service_routes_measurement_through_current_measurer() {
        let _app_context = crate::render_state::app_context_test_scope();
        let service = TextService::from_measurer(Rc::new(MonospacedTextMeasurer));
        let text = crate::text::AnnotatedString::from("abc");
        let style = TextStyle::default();

        let metrics = service.with_measurer(|measurer| measurer.measure(&text, &style));

        assert!(metrics.width > 0.0);
        assert!(metrics.height > 0.0);
    }

    #[test]
    fn text_service_caches_metrics_and_layouts_per_context() {
        let _app_context = crate::render_state::app_context_test_scope();
        let measure_calls = Rc::new(Cell::new(0));
        let layout_calls = Rc::new(Cell::new(0));
        let service = TextService::from_measurer(Rc::new(CountingTextMeasurer::new(
            Rc::clone(&measure_calls),
            Rc::clone(&layout_calls),
        )));
        let text = crate::text::AnnotatedString::from("cached text");
        let style = TextStyle::default();

        let first_metrics = service.measure(Some(7), &text, &style);
        let second_metrics = service.measure(Some(7), &text, &style);
        let first_layout = service.layout(&text, &style);
        let second_layout = service.layout(&text, &style);

        assert_eq!(first_metrics, second_metrics);
        assert_eq!(first_layout.width, second_layout.width);
        assert_eq!(measure_calls.get(), 1);
        assert_eq!(layout_calls.get(), 1);
    }

    #[test]
    fn text_service_clears_caches_when_measurer_changes() {
        let _app_context = crate::render_state::app_context_test_scope();
        let first_measure_calls = Rc::new(Cell::new(0));
        let second_measure_calls = Rc::new(Cell::new(0));
        let layout_calls = Rc::new(Cell::new(0));
        let service = TextService::from_measurer(Rc::new(CountingTextMeasurer::new(
            Rc::clone(&first_measure_calls),
            Rc::clone(&layout_calls),
        )));
        let text = crate::text::AnnotatedString::from("cached text");
        let style = TextStyle::default();

        let _ = service.measure(None, &text, &style);
        let _ = service.measure(None, &text, &style);
        service.set_measurer(Rc::new(CountingTextMeasurer::new(
            Rc::clone(&second_measure_calls),
            Rc::clone(&layout_calls),
        )));
        let _ = service.measure(None, &text, &style);

        assert_eq!(first_measure_calls.get(), 1);
        assert_eq!(second_measure_calls.get(), 1);
    }

    fn style_with_line_break(line_break: LineBreak) -> TextStyle {
        TextStyle {
            span_style: crate::text::SpanStyle {
                font_size: TextUnit::Sp(10.0),
                ..Default::default()
            },
            paragraph_style: ParagraphStyle {
                line_break,
                ..Default::default()
            },
        }
    }

    fn style_with_hyphens(hyphens: Hyphens) -> TextStyle {
        TextStyle {
            span_style: crate::text::SpanStyle {
                font_size: TextUnit::Sp(10.0),
                ..Default::default()
            },
            paragraph_style: ParagraphStyle {
                hyphens,
                ..Default::default()
            },
        }
    }

    fn assert_f32_close(actual: f32, expected: f32) {
        assert!(
            (actual - expected).abs() <= 0.01,
            "actual={actual}, expected={expected}"
        );
    }

    #[test]
    fn text_layout_options_wraps_and_limits_lines() {
        let _app_context = crate::render_state::app_context_test_scope();
        let style = TextStyle {
            span_style: crate::text::SpanStyle {
                font_size: TextUnit::Sp(10.0),
                ..Default::default()
            },
            ..Default::default()
        };
        let options = TextLayoutOptions {
            overflow: TextOverflow::Clip,
            soft_wrap: true,
            max_lines: 2,
            min_lines: 1,
        };

        let prepared = prepare_text_layout(
            &crate::text::AnnotatedString::from("A B C D E F"),
            &style,
            options,
            Some(24.0), // roughly 4 chars in monospaced fallback
        );

        assert!(prepared.did_overflow);
        assert!(prepared.metrics.line_count <= 2);
    }

    #[test]
    fn text_layout_options_end_ellipsis_applies() {
        let _app_context = crate::render_state::app_context_test_scope();
        let style = TextStyle {
            span_style: crate::text::SpanStyle {
                font_size: TextUnit::Sp(10.0),
                ..Default::default()
            },
            ..Default::default()
        };
        let options = TextLayoutOptions {
            overflow: TextOverflow::Ellipsis,
            soft_wrap: false,
            max_lines: 1,
            min_lines: 1,
        };

        let prepared = prepare_text_layout(
            &crate::text::AnnotatedString::from("Long long line"),
            &style,
            options,
            Some(20.0),
        );
        assert!(prepared.did_overflow);
        assert!(prepared.text.text.contains(ELLIPSIS));
    }

    #[test]
    fn text_layout_options_visible_keeps_full_text() {
        let _app_context = crate::render_state::app_context_test_scope();
        let style = TextStyle {
            span_style: crate::text::SpanStyle {
                font_size: TextUnit::Sp(10.0),
                ..Default::default()
            },
            ..Default::default()
        };
        let options = TextLayoutOptions {
            overflow: TextOverflow::Visible,
            soft_wrap: false,
            max_lines: 1,
            min_lines: 1,
        };

        let input = "This should remain unchanged";
        let prepared = prepare_text_layout(
            &crate::text::AnnotatedString::from(input),
            &style,
            options,
            Some(10.0),
        );
        assert_eq!(prepared.text.text, input);
    }

    #[test]
    fn text_layout_options_respects_min_lines() {
        let _app_context = crate::render_state::app_context_test_scope();
        let style = TextStyle {
            span_style: crate::text::SpanStyle {
                font_size: TextUnit::Sp(10.0),
                ..Default::default()
            },
            ..Default::default()
        };
        let options = TextLayoutOptions {
            overflow: TextOverflow::Clip,
            soft_wrap: true,
            max_lines: 4,
            min_lines: 3,
        };

        let prepared = prepare_text_layout(
            &crate::text::AnnotatedString::from("short"),
            &style,
            options,
            Some(100.0),
        );
        assert_eq!(prepared.metrics.line_count, 3);
    }

    #[test]
    fn text_layout_options_middle_ellipsis_for_single_line() {
        let _app_context = crate::render_state::app_context_test_scope();
        let style = TextStyle {
            span_style: crate::text::SpanStyle {
                font_size: TextUnit::Sp(10.0),
                ..Default::default()
            },
            ..Default::default()
        };
        let options = TextLayoutOptions {
            overflow: TextOverflow::MiddleEllipsis,
            soft_wrap: false,
            max_lines: 1,
            min_lines: 1,
        };

        let prepared = prepare_text_layout(
            &crate::text::AnnotatedString::from("abcdefghijk"),
            &style,
            options,
            Some(24.0),
        );
        assert!(prepared.text.text.contains(ELLIPSIS));
        assert!(prepared.did_overflow);
    }

    #[test]
    fn text_layout_options_scale_down_fits_without_rewriting_text() {
        let _app_context = crate::render_state::app_context_test_scope();
        let style = TextStyle {
            span_style: crate::text::SpanStyle {
                font_size: TextUnit::Sp(20.0),
                ..Default::default()
            },
            ..Default::default()
        };
        let options = TextLayoutOptions {
            overflow: TextOverflow::ScaleDown {
                min_font_size_sp: 10.0,
            },
            soft_wrap: false,
            max_lines: 1,
            min_lines: 1,
        };

        let prepared = prepare_text_layout(
            &crate::text::AnnotatedString::from("ABCDE"),
            &style,
            options,
            Some(36.0),
        );

        assert_eq!(prepared.text.text, "ABCDE");
        assert!(prepared.metrics.width <= 36.0 + WRAP_EPSILON);
        assert!(!prepared.did_overflow);
        let visual_font_size = prepared.visual_style.resolve_font_size(14.0);
        assert!(visual_font_size < 20.0);
        assert!(visual_font_size >= 10.0);
    }

    #[test]
    fn text_layout_options_scale_down_scales_root_shadow() {
        let _app_context = crate::render_state::app_context_test_scope();
        let style = TextStyle {
            span_style: crate::text::SpanStyle {
                font_size: TextUnit::Sp(20.0),
                shadow: Some(crate::text::Shadow {
                    color: crate::modifier::Color(0.0, 0.0, 0.0, 1.0),
                    offset: crate::modifier::Point::new(8.0, 4.0),
                    blur_radius: 6.0,
                }),
                ..Default::default()
            },
            ..Default::default()
        };
        let options = TextLayoutOptions {
            overflow: TextOverflow::ScaleDown {
                min_font_size_sp: 10.0,
            },
            soft_wrap: false,
            max_lines: 1,
            min_lines: 1,
        };

        let prepared = prepare_text_layout(
            &crate::text::AnnotatedString::from("ABCDE"),
            &style,
            options,
            Some(36.0),
        );

        let font_scale = prepared.visual_style.resolve_font_size(14.0) / 20.0;
        let shadow = prepared
            .visual_style
            .span_style
            .shadow
            .expect("scaled style should retain shadow");
        assert_f32_close(shadow.offset.x, 8.0 * font_scale);
        assert_f32_close(shadow.offset.y, 4.0 * font_scale);
        assert_f32_close(shadow.blur_radius, 6.0 * font_scale);
    }

    #[test]
    fn text_layout_options_scale_down_stops_at_minimum_and_clips() {
        let _app_context = crate::render_state::app_context_test_scope();
        let style = TextStyle {
            span_style: crate::text::SpanStyle {
                font_size: TextUnit::Sp(20.0),
                ..Default::default()
            },
            ..Default::default()
        };
        let options = TextLayoutOptions {
            overflow: TextOverflow::ScaleDown {
                min_font_size_sp: 10.0,
            },
            soft_wrap: false,
            max_lines: 1,
            min_lines: 1,
        };

        let prepared = prepare_text_layout(
            &crate::text::AnnotatedString::from("ABCDEFGHIJ"),
            &style,
            options,
            Some(12.0),
        );

        assert_eq!(prepared.text.text, "ABCDEFGHIJ");
        assert!(prepared.did_overflow);
        assert_eq!(prepared.metrics.width, 12.0);
        assert_eq!(prepared.visual_style.resolve_font_size(14.0), 10.0);
    }

    #[test]
    fn scale_annotated_font_sizes_borrows_when_spans_need_no_scaling() {
        let _app_context = crate::render_state::app_context_test_scope();
        let plain = crate::text::AnnotatedString::from("plain");
        assert!(matches!(
            scale_annotated_font_sizes(&plain, 0.5),
            std::borrow::Cow::Borrowed(_)
        ));

        let colored = crate::text::annotated_string::Builder::new()
            .push_style(crate::text::SpanStyle {
                color: Some(crate::modifier::Color(1.0, 0.0, 0.0, 1.0)),
                ..Default::default()
            })
            .append("colored")
            .pop()
            .to_annotated_string();
        assert!(matches!(
            scale_annotated_font_sizes(&colored, 0.5),
            std::borrow::Cow::Borrowed(_)
        ));
    }

    #[test]
    fn scale_annotated_font_sizes_scales_span_shadow_geometry() {
        let _app_context = crate::render_state::app_context_test_scope();
        let text = crate::text::annotated_string::Builder::new()
            .push_style(crate::text::SpanStyle {
                shadow: Some(crate::text::Shadow {
                    color: crate::modifier::Color(0.0, 0.0, 0.0, 1.0),
                    offset: crate::modifier::Point::new(6.0, 2.0),
                    blur_radius: 4.0,
                }),
                ..Default::default()
            })
            .append("shadow")
            .pop()
            .to_annotated_string();

        let scaled = scale_annotated_font_sizes(&text, 0.5);
        let std::borrow::Cow::Owned(scaled) = scaled else {
            panic!("shadowed span should be scaled into owned text");
        };
        let shadow = scaled.span_styles[0]
            .item
            .shadow
            .expect("scaled span should retain shadow");
        assert_f32_close(shadow.offset.x, 3.0);
        assert_f32_close(shadow.offset.y, 1.0);
        assert_f32_close(shadow.blur_radius, 2.0);
    }

    #[test]
    fn text_layout_options_does_not_wrap_on_tiny_width_delta() {
        let _app_context = crate::render_state::app_context_test_scope();
        let style = TextStyle {
            span_style: crate::text::SpanStyle {
                font_size: TextUnit::Sp(10.0),
                ..Default::default()
            },
            ..Default::default()
        };
        let options = TextLayoutOptions {
            overflow: TextOverflow::Clip,
            soft_wrap: true,
            max_lines: usize::MAX,
            min_lines: 1,
        };

        let text = "if counter % 2 == 0";
        let exact_width = measure_text(&crate::text::AnnotatedString::from(text), &style).width;
        let prepared = prepare_text_layout(
            &crate::text::AnnotatedString::from(text),
            &style,
            options,
            Some(exact_width - 0.1),
        );

        assert!(
            !prepared.text.text.contains('\n'),
            "unexpected line split: {:?}",
            prepared.text
        );
    }

    #[test]
    fn line_break_mode_changes_wrap_strategy_contract() {
        let _app_context = crate::render_state::app_context_test_scope();
        let text = "This is an example text";
        let options = TextLayoutOptions {
            overflow: TextOverflow::Clip,
            soft_wrap: true,
            max_lines: usize::MAX,
            min_lines: 1,
        };

        let simple = prepare_text_layout(
            &crate::text::AnnotatedString::from(text),
            &style_with_line_break(LineBreak::Simple),
            options,
            Some(120.0),
        );
        let heading = prepare_text_layout(
            &crate::text::AnnotatedString::from(text),
            &style_with_line_break(LineBreak::Heading),
            options,
            Some(120.0),
        );
        let paragraph = prepare_text_layout(
            &crate::text::AnnotatedString::from(text),
            &style_with_line_break(LineBreak::Paragraph),
            options,
            Some(50.0),
        );

        assert_eq!(
            simple.text.text.lines().collect::<Vec<_>>(),
            vec!["This is an example", "text"]
        );
        assert_eq!(
            heading.text.text.lines().collect::<Vec<_>>(),
            vec!["This is an", "example text"]
        );
        assert_eq!(
            paragraph.text.text.lines().collect::<Vec<_>>(),
            vec!["This", "is an", "example", "text"]
        );
    }

    #[test]
    fn hyphens_mode_changes_wrap_strategy_contract() {
        let _app_context = crate::render_state::app_context_test_scope();
        let text = "Transformation";
        let options = TextLayoutOptions {
            overflow: TextOverflow::Clip,
            soft_wrap: true,
            max_lines: usize::MAX,
            min_lines: 1,
        };

        let auto = prepare_text_layout(
            &crate::text::AnnotatedString::from(text),
            &style_with_hyphens(Hyphens::Auto),
            options,
            Some(24.0),
        );
        let none = prepare_text_layout(
            &crate::text::AnnotatedString::from(text),
            &style_with_hyphens(Hyphens::None),
            options,
            Some(24.0),
        );

        assert_eq!(
            auto.text.text.lines().collect::<Vec<_>>(),
            vec!["Tran", "sfor", "ma", "tion"]
        );
        assert_eq!(
            none.text.text.lines().collect::<Vec<_>>(),
            vec!["Tran", "sfor", "mati", "on"]
        );
        assert!(
            !auto.text.text.contains('-'),
            "automatic hyphenation should influence breaks without mutating source text content"
        );
    }

    #[test]
    fn hyphens_auto_uses_measurer_hyphen_contract_when_valid() {
        let _app_context = crate::render_state::app_context_test_scope();
        let text = "Transformation";
        let style = style_with_hyphens(Hyphens::Auto);
        let options = TextLayoutOptions {
            overflow: TextOverflow::Clip,
            soft_wrap: true,
            max_lines: usize::MAX,
            min_lines: 1,
        };

        let prepared = prepare_text_layout_fallback(
            &ContractBreakMeasurer { retreat: 1 },
            &crate::text::AnnotatedString::from(text),
            &style,
            options,
            Some(24.0),
        );

        assert_eq!(
            prepared.text.text.lines().collect::<Vec<_>>(),
            vec!["Tra", "nsf", "orm", "ati", "on"]
        );
    }

    #[test]
    fn hyphens_auto_falls_back_when_measurer_hyphen_contract_is_invalid() {
        let _app_context = crate::render_state::app_context_test_scope();
        let text = "Transformation";
        let style = style_with_hyphens(Hyphens::Auto);
        let options = TextLayoutOptions {
            overflow: TextOverflow::Clip,
            soft_wrap: true,
            max_lines: usize::MAX,
            min_lines: 1,
        };

        let prepared = prepare_text_layout_fallback(
            &ContractBreakMeasurer { retreat: 10 },
            &crate::text::AnnotatedString::from(text),
            &style,
            options,
            Some(24.0),
        );

        assert_eq!(
            prepared.text.text.lines().collect::<Vec<_>>(),
            vec!["Tran", "sfor", "ma", "tion"]
        );
    }

    #[test]
    fn transformed_text_keeps_span_ranges_within_display_bounds() {
        let _app_context = crate::render_state::app_context_test_scope();
        let style = TextStyle {
            span_style: crate::text::SpanStyle {
                font_size: TextUnit::Sp(10.0),
                ..Default::default()
            },
            ..Default::default()
        };
        let options = TextLayoutOptions {
            overflow: TextOverflow::Ellipsis,
            soft_wrap: false,
            max_lines: 1,
            min_lines: 1,
        };
        let annotated = crate::text::AnnotatedString::builder()
            .push_style(crate::text::SpanStyle {
                font_weight: Some(crate::text::FontWeight::BOLD),
                ..Default::default()
            })
            .append("Styled overflow text sample")
            .pop()
            .to_annotated_string();

        let prepared = prepare_text_layout(&annotated, &style, options, Some(40.0));
        assert!(prepared.did_overflow);
        for span in &prepared.text.span_styles {
            assert!(span.range.start < span.range.end);
            assert!(span.range.end <= prepared.text.text.len());
            assert!(prepared.text.text.is_char_boundary(span.range.start));
            assert!(prepared.text.text.is_char_boundary(span.range.end));
        }
    }

    #[test]
    fn wrapped_text_splits_styles_around_inserted_newlines() {
        let _app_context = crate::render_state::app_context_test_scope();
        let style = TextStyle {
            span_style: crate::text::SpanStyle {
                font_size: TextUnit::Sp(10.0),
                ..Default::default()
            },
            ..Default::default()
        };
        let options = TextLayoutOptions {
            overflow: TextOverflow::Clip,
            soft_wrap: true,
            max_lines: usize::MAX,
            min_lines: 1,
        };
        let annotated = crate::text::AnnotatedString::builder()
            .push_style(crate::text::SpanStyle {
                text_decoration: Some(crate::text::TextDecoration::UNDERLINE),
                ..Default::default()
            })
            .append("Wrapped style text example")
            .pop()
            .to_annotated_string();

        let prepared = prepare_text_layout(&annotated, &style, options, Some(32.0));
        assert!(prepared.text.text.contains('\n'));
        assert!(!prepared.text.span_styles.is_empty());
        for span in &prepared.text.span_styles {
            assert!(span.range.end <= prepared.text.text.len());
        }
    }

    #[test]
    fn mixed_font_size_segments_wrap_without_truncation() {
        let _app_context = crate::render_state::app_context_test_scope();
        let style = TextStyle {
            span_style: crate::text::SpanStyle {
                font_size: TextUnit::Sp(14.0),
                ..Default::default()
            },
            ..Default::default()
        };
        let options = TextLayoutOptions {
            overflow: TextOverflow::Clip,
            soft_wrap: true,
            max_lines: usize::MAX,
            min_lines: 1,
        };
        let annotated = crate::text::AnnotatedString::builder()
            .append("You can also ")
            .push_style(crate::text::SpanStyle {
                font_size: TextUnit::Sp(22.0),
                ..Default::default()
            })
            .append("change font size")
            .pop()
            .append(" dynamically mid-sentence!")
            .to_annotated_string();

        let prepared = prepare_text_layout(&annotated, &style, options, Some(260.0));
        assert!(prepared.text.text.contains('\n'));
        assert!(prepared.text.text.contains("mid-sentence!"));
        assert!(!prepared.did_overflow);
    }
}
