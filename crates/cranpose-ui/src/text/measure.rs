use std::{
    borrow::Cow,
    cell::{Cell, RefCell},
    hash::Hash,
    ops::Range,
    rc::Rc,
    sync::Arc,
};

use cranpose_core::{NodeId, collections::pass_aged::PassAgedCache};
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
/// Prepared layouts kept across nodes. Each node keeps its own, so this
/// serves the items a list composes again, a few screens of them; an entry
/// holds a whole visual style, and a screen laid out again at a new width
/// every frame fills every entry with widths it never asks for again.
const TEXT_PREPARED_CACHE_CAPACITY: usize = 1024;
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

#[derive(Clone, Debug)]
pub struct PreparedTextLayout {
    /// Shared display text after wrapping and overflow have been resolved.
    pub text: Rc<crate::text::AnnotatedString>,
    /// The style the text draws in, shared with every render node drawn
    /// from this layout.
    pub visual_style: std::sync::Arc<TextStyle>,
    pub metrics: TextMetrics,
    /// The first and last drawn baselines, excluding blank height added by `min_lines`.
    pub alignment_lines: cranpose_ui_layout::AlignmentLines,
    pub did_overflow: bool,
    /// `text` as a renderer draws it, converted on first use: see
    /// [`PreparedTextLayout::render_text`].
    pub render_text: std::cell::OnceCell<std::sync::Arc<crate::text::RenderString>>,
    /// The max widths the layout's greedy wrap breaks the same lines at and
    /// cuts its ellipsis at the same character, when it wrapped: `None` when
    /// it did not, broke lines another way, or a line overflowed its width.
    pub(crate) wrap_hold: Option<WrapHold>,
}

/// The max widths a greedy wrap breaks a text's lines the same at: every
/// width `w` with `fits <= w + WRAP_EPSILON < pulls_up`. Below `fits` a break
/// no longer fits; from `pulls_up` a line takes up its next word.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct WrapHold {
    fits: f32,
    pulls_up: f32,
}

impl WrapHold {
    /// Every width, before any line narrows it.
    const ANY: Self = Self {
        fits: f32::NEG_INFINITY,
        pulls_up: f32::INFINITY,
    };

    fn narrow(&mut self, fits: f32, pulls_up: f32) {
        self.fits = self.fits.max(fits);
        self.pulls_up = self.pulls_up.min(pulls_up);
    }

    fn holds(self, width: f32) -> bool {
        self.fits <= width + WRAP_EPSILON && width + WRAP_EPSILON < self.pulls_up
    }

    /// Narrows `hold` to the widths a line `width` wide fits whole at.
    fn fit_whole(hold: &mut Option<Self>, width: f32) {
        if let Some(hold) = hold {
            hold.narrow(width, f32::INFINITY);
        }
    }

    /// `hold` narrowed to the widths its layout reports `measured_width` at,
    /// and kept only when it holds `max_width`, the width it was made at: a
    /// layout wider than its limit reports the limit.
    fn settle(hold: Option<Self>, measured_width: f32, max_width: Option<f32>) -> Option<Self> {
        let mut hold = hold?;
        hold.narrow(measured_width + WRAP_EPSILON, f32::INFINITY);
        max_width.filter(|width| hold.holds(*width)).map(|_| hold)
    }
}

impl PreparedTextLayout {
    /// The display text as a renderer draws it. Converted once for the layout
    /// and shared after, so every frame and scene rebuild drawing this layout
    /// hands over the same allocation.
    pub fn render_text(&self) -> std::sync::Arc<crate::text::RenderString> {
        if let Some(converted) = self.render_text.get() {
            return std::sync::Arc::clone(converted);
        }
        let converted = std::sync::Arc::new(self.text.render_string());
        let _ = self.render_text.set(std::sync::Arc::clone(&converted));
        converted
    }
}

impl PartialEq for PreparedTextLayout {
    fn eq(&self, other: &Self) -> bool {
        self.text == other.text
            && self.visual_style == other.visual_style
            && self.metrics == other.metrics
            && self.alignment_lines == other.alignment_lines
            && self.did_overflow == other.did_overflow
    }
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

    /// Called as each layout pass starts. A measurer that caches
    /// measurements can age them by passes and drop the ones recent passes
    /// did not use.
    fn begin_layout_pass(&self) {}

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

    /// The widths of every prefix of one line.
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

    /// One line's whole box for a style: its advance, its baseline and what a
    /// paragraph of it gives back at its edges, with no string to measure.
    ///
    /// A layout that stacks rows of a known style needs the row pitch before it
    /// has any text to put in them, and a paragraph's height and first baseline
    /// both depend on the edges. `None` when the measurer has no font metrics.
    fn line_box(&self, style: &TextStyle) -> Option<crate::text::LineBox> {
        let _ = style;
        None
    }

    /// Visits each displayed line's resolved font box, including styled spans.
    /// The text must already contain its wrapping newlines. Returns `None` when
    /// the measurer does not provide these metrics; no callbacks run in that case.
    /// The default handles plain text and defers annotated spans to `line_height`.
    fn visit_line_boxes(
        &self,
        text: &crate::text::AnnotatedString,
        style: &TextStyle,
        visit: &mut dyn FnMut(crate::text::LineBox),
    ) -> Option<()> {
        if !text.span_styles.is_empty() {
            return None;
        }
        let line_box = self.line_box(style)?;
        for _ in text.text.split('\n') {
            visit(line_box);
        }
        Some(())
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
        prepare_text_layout_with_measurer_for_node(self, node_id, text, style, options, max_width)
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

    /// Lays `text` out in `style` for a node that holds both: a layout that
    /// leaves them as they are shares them instead of copying them.
    fn prepare_with_options_for_node(
        &self,
        node_id: Option<NodeId>,
        text: &Rc<crate::text::AnnotatedString>,
        style: &Arc<TextStyle>,
        options: TextLayoutOptions,
        max_width: Option<f32>,
    ) -> PreparedTextLayout {
        prepare_layout(
            self,
            node_id,
            LayoutSource::Shared { text, style },
            options,
            max_width,
        )
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

pub(crate) struct TextService {
    generation: Cell<u64>,
    measurer: RefCell<Rc<dyn TextMeasurer>>,
    metrics_cache: RefCell<PassAgedCache<TextBaseCacheKey, TextMetrics>>,
    options_metrics_cache: RefCell<PassAgedCache<TextOptionsCacheKey, TextMetrics>>,
    prepared_cache: RefCell<PassAgedCache<TextPreparedCacheKey, Rc<PreparedTextLayout>>>,
    layout_cache: RefCell<PassAgedCache<TextBaseCacheKey, TextLayoutResult>>,
}

impl TextService {
    pub(crate) fn new() -> Self {
        Self::from_measurer(Rc::new(MonospacedTextMeasurer))
    }

    pub(crate) fn from_measurer(measurer: Rc<dyn TextMeasurer>) -> Self {
        Self {
            generation: Cell::new(1),
            measurer: RefCell::new(measurer),
            metrics_cache: RefCell::new(PassAgedCache::with_capacity_at_least_one(
                TEXT_SERVICE_CACHE_CAPACITY,
            )),
            options_metrics_cache: RefCell::new(PassAgedCache::with_capacity_at_least_one(
                TEXT_SERVICE_CACHE_CAPACITY,
            )),
            prepared_cache: RefCell::new(PassAgedCache::with_capacity_at_least_one(
                TEXT_PREPARED_CACHE_CAPACITY,
            )),
            layout_cache: RefCell::new(PassAgedCache::with_capacity_at_least_one(
                TEXT_SERVICE_CACHE_CAPACITY,
            )),
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
        if let Some(key) = key
            && let Some(metrics) = self.metrics_cache.borrow_mut().get(&key).copied()
        {
            return metrics;
        }
        let metrics = self.with_measurer(|m| m.measure_for_node(node_id, text, style));
        if let Some(key) = key {
            self.metrics_cache.borrow_mut().push(key, metrics);
        }
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
        if let Some(key) = key {
            if let Some(metrics) = self.options_metrics_cache.borrow_mut().get(&key).copied() {
                return metrics;
            }
            let prepared_key = TextPreparedCacheKey {
                base: key,
                visual_hash: style.render_hash(),
            };
            if let Some(prepared) = self.prepared_cache.borrow_mut().get(&prepared_key) {
                return prepared.metrics;
            }
        }
        let metrics = self.with_measurer(|m| {
            m.measure_with_options_for_node(node_id, text, style, options.normalized(), max_width)
        });
        if let Some(key) = key {
            self.options_metrics_cache.borrow_mut().push(key, metrics);
        }
        metrics
    }

    /// The layout of `text` at `max_width`, shared with the cache: a layout
    /// animated through widths misses on every frame, and copying each one
    /// into and out of the cache cost more than laying it out.
    pub(crate) fn prepare_with_options(
        &self,
        node_id: Option<NodeId>,
        text: &Rc<crate::text::AnnotatedString>,
        style: &Arc<TextStyle>,
        options: TextLayoutOptions,
        max_width: Option<f32>,
    ) -> Rc<PreparedTextLayout> {
        let key =
            text_options_cache_key(text, style, options.normalized(), max_width).map(|base| {
                TextPreparedCacheKey {
                    base,
                    visual_hash: style.render_hash(),
                }
            });
        if let Some(key) = key
            && let Some(prepared) = self.prepared_cache.borrow_mut().get(&key).map(Rc::clone)
        {
            return prepared;
        }
        let prepared = Rc::new(self.with_measurer(|m| {
            m.prepare_with_options_for_node(node_id, text, style, options.normalized(), max_width)
        }));
        if let Some(key) = key {
            self.prepared_cache
                .borrow_mut()
                .push(key, Rc::clone(&prepared));
        }
        prepared
    }

    pub(crate) fn layout(
        &self,
        text: &crate::text::AnnotatedString,
        style: &TextStyle,
    ) -> TextLayoutResult {
        let key = text_base_cache_key(text, style);
        if let Some(key) = key
            && let Some(layout) = self.layout_cache.borrow_mut().get(&key).cloned()
        {
            return layout;
        }
        let layout = self.with_measurer(|m| m.layout(text, style));
        if let Some(key) = key {
            self.layout_cache.borrow_mut().push(key, layout.clone());
        }
        layout
    }

    /// Starts a layout pass: the measurer and these caches drop what recent
    /// passes did not use.
    pub(crate) fn begin_layout_pass(&self) {
        self.with_measurer(TextMeasurer::begin_layout_pass);
        self.metrics_cache.borrow_mut().begin_pass(drop);
        self.options_metrics_cache.borrow_mut().begin_pass(drop);
        self.prepared_cache.borrow_mut().begin_pass(drop);
        self.layout_cache.borrow_mut().begin_pass(drop);
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

fn text_base_cache_key(
    text: &crate::text::AnnotatedString,
    style: &TextStyle,
) -> Option<TextBaseCacheKey> {
    (text.string_annotations.is_empty() && text.link_annotations.is_empty()).then(|| {
        TextBaseCacheKey {
            text_hash: text.render_hash(),
            style_hash: style.measurement_hash(),
        }
    })
}

fn text_options_cache_key(
    text: &crate::text::AnnotatedString,
    style: &TextStyle,
    options: TextLayoutOptions,
    max_width: Option<f32>,
) -> Option<TextOptionsCacheKey> {
    Some(TextOptionsCacheKey {
        base: text_base_cache_key(text, style)?,
        options: options.normalized(),
        max_width_bits: normalize_max_width(max_width).map(f32::to_bits),
    })
}

pub fn set_text_measurer<M: TextMeasurer>(measurer: M) {
    crate::render_state::set_current_text_measurer(Rc::new(measurer));
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
    let style = system_scaled_style(style);
    crate::render_state::with_text_service(|service| {
        service.with_measurer(|m| m.glyph_line_box(&style))
    })
    .map_or((0.0, line_height), |(off, h)| {
        (off.min(line_height), h.min(line_height))
    })
}

/// The paragraph line box of `style` (see [`TextMeasurer::line_box`]): its
/// advance, its baseline and what a paragraph gives back at its edges. `None`
/// when the active measurer carries no font metrics.
pub fn text_line_box(style: &TextStyle) -> Option<crate::text::LineBox> {
    let style = system_scaled_style(style);
    crate::render_state::with_text_service(|service| service.with_measurer(|m| m.line_box(&style)))
}

/// Distance from the top of a `style` line slot down to its baseline (see
/// [`TextMeasurer::first_baseline`]). `None` when the active measurer carries
/// no font metrics.
pub fn first_baseline(style: &TextStyle) -> Option<f32> {
    let style = system_scaled_style(style);
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
    Rc::unwrap_or_clone(prepare_text_layout_for_node(
        None,
        &Rc::new(text.clone()),
        &Arc::new(style.clone()),
        options,
        max_width,
    ))
}

/// Lays out `text` in `style` for `node_id`. The layout shares the text and
/// style when it leaves them as they are.
pub fn prepare_text_layout_for_node(
    node_id: Option<NodeId>,
    text: &Rc<crate::text::AnnotatedString>,
    style: &Arc<TextStyle>,
    options: TextLayoutOptions,
    max_width: Option<f32>,
) -> Rc<PreparedTextLayout> {
    let prepare = |text: &Rc<crate::text::AnnotatedString>, style: &Arc<TextStyle>| {
        crate::render_state::with_text_service(|service| {
            service.prepare_with_options(node_id, text, style, options.normalized(), max_width)
        })
    };
    let Some(curve) = crate::render_state::current_scaling_font_scale_curve() else {
        return prepare(text, style);
    };
    let scaled_text = match scale_annotated_font_sizes(text, curve) {
        Cow::Borrowed(_) => Rc::clone(text),
        Cow::Owned(scaled) => Rc::new(scaled),
    };
    prepare(
        &scaled_text,
        &Arc::new(scale_text_style_font_sizes(style, curve).into_owned()),
    )
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
        return line_ranges.into_vec();
    };
    let mut lines = DisplayLines::with_capacity(line_ranges.len());
    for line_range in line_ranges {
        wrap_line_to_width(
            measurer,
            text,
            line_range,
            style,
            (width_limit, LineLimit::NONE, &mut None),
            (line_break_mode, hyphens_mode),
            &mut lines,
        );
    }
    lines.into_iter().map(|line| line.source_range).collect()
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
    prepare_layout(
        measurer,
        node_id,
        LayoutSource::Borrowed { text, style },
        options,
        max_width,
    )
}

/// The text and style a layout is prepared from: borrowed, which the layout
/// copies, or shared with the node that holds them.
#[derive(Clone, Copy)]
enum LayoutSource<'a> {
    Borrowed {
        text: &'a crate::text::AnnotatedString,
        style: &'a TextStyle,
    },
    Shared {
        text: &'a Rc<crate::text::AnnotatedString>,
        style: &'a Arc<TextStyle>,
    },
}

impl LayoutSource<'_> {
    fn text(&self) -> &crate::text::AnnotatedString {
        match self {
            Self::Borrowed { text, .. } => text,
            Self::Shared { text, .. } => text,
        }
    }

    fn style(&self) -> &TextStyle {
        match self {
            Self::Borrowed { style, .. } => style,
            Self::Shared { style, .. } => style,
        }
    }

    /// The source text as the layout's display text, when the layout left it
    /// as it is: shared, or built from `lines` when it cannot be shared.
    fn display_text(
        &self,
        lines: &[DisplayLine],
        unchanged: bool,
    ) -> Rc<crate::text::AnnotatedString> {
        match self {
            Self::Shared { text, .. } if unchanged => Rc::clone(text),
            _ => Rc::new(build_display_annotated(self.text(), lines)),
        }
    }

    fn visual_style(&self) -> Arc<TextStyle> {
        match self {
            Self::Borrowed { style, .. } => Arc::new((*style).clone()),
            Self::Shared { style, .. } => Arc::clone(style),
        }
    }
}

fn prepare_layout<M: TextMeasurer + ?Sized>(
    measurer: &M,
    node_id: Option<NodeId>,
    source: LayoutSource<'_>,
    options: TextLayoutOptions,
    max_width: Option<f32>,
) -> PreparedTextLayout {
    let (text, style) = (source.text(), source.style());
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
    let mut visible_lines: DisplayLines;
    let mut wrap_hold = None;
    if let Some(width_limit) = wrap_width {
        (visible_lines, wrap_hold) = wrap_lines(
            measurer,
            text,
            line_ranges,
            style,
            (width_limit, LineLimit::of(opts)),
            (line_break_mode, hyphens_mode),
        );
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
        (text, style),
        opts,
        max_width,
        (&mut visible_lines, &mut wrap_hold),
    );
    let overflow_ms = overflow_start.map(|start| start.elapsed().as_secs_f64() * 1000.0);

    let build_start = telemetry.then(Instant::now);
    // No line wrapped, was cut or elided: the display text is the source.
    let unchanged = !did_overflow
        && visible_lines.len() == source_line_count
        && visible_lines
            .iter()
            .all(|line| matches!(line.text, DisplayLineText::Source));
    let display_annotated = source.display_text(&visible_lines, unchanged);
    debug_assert_eq!(
        display_annotated.text,
        join_display_line_text(text, &visible_lines)
    );
    let build_ms = build_start.map(|start| start.elapsed().as_secs_f64() * 1000.0);

    let metrics_start = telemetry.then(Instant::now);
    let display_line_count = visible_lines.len().max(1);
    let layout_line_count = display_line_count.max(opts.min_lines);

    let measured_width = if visible_lines.is_empty() {
        0.0
    } else {
        visible_lines
            .iter_mut()
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
    let wrap_hold = WrapHold::settle(wrap_hold, measured_width, wrap_width);

    let vertical = prepared_line_metrics(
        measurer,
        node_id,
        text,
        &display_annotated,
        style,
        opts.min_lines,
    );
    let prepared = PreparedTextLayout {
        text: display_annotated,
        visual_style: source.visual_style(),
        alignment_lines: vertical.alignment_lines,
        metrics: TextMetrics {
            width,
            height: vertical.height,
            line_height: vertical.line_height,
            line_count: layout_line_count,
        },
        did_overflow,
        render_text: Default::default(),
        wrap_hold,
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

struct PreparedLineMetrics {
    height: f32,
    line_height: f32,
    alignment_lines: cranpose_ui_layout::AlignmentLines,
}

fn prepared_line_metrics<M: TextMeasurer + ?Sized>(
    measurer: &M,
    node_id: Option<NodeId>,
    source: &crate::text::AnnotatedString,
    display: &crate::text::AnnotatedString,
    style: &TextStyle,
    min_lines: usize,
) -> PreparedLineMetrics {
    let base_box = measurer.line_box(style);
    if !display.span_styles.is_empty() {
        let mut top = 0.0;
        let mut trim_bottom = 0.0;
        let mut first = None;
        let mut last = None;
        let mut line_height = 0.0_f32;
        let resolved = measurer.visit_line_boxes(display, style, &mut |line| {
            if first.is_none() {
                top = -line.trim_top;
                first = Some(top + line.baseline);
            }
            last = Some(top + line.baseline);
            top += line.height;
            trim_bottom = line.trim_bottom;
            line_height = line_height.max(line.height);
        });
        if resolved.is_some() {
            let min_height = if min_lines > 1 {
                base_box.map_or(0.0, |line| line.block_height(min_lines))
            } else {
                0.0
            };
            return PreparedLineMetrics {
                height: (top - trim_bottom).max(min_height),
                line_height,
                alignment_lines: cranpose_ui_layout::AlignmentLines::new(first, last),
            };
        }
    }
    let measured_text = if source.span_styles.is_empty() {
        source
    } else {
        display
    };
    let line_height = measurer
        .line_height_for_node(node_id, measured_text, style)
        .max(0.0);
    let first = base_box
        .map(crate::text::LineBox::first_baseline)
        .or_else(|| measurer.first_baseline(style));
    let displayed_lines = display.text.split('\n').count().max(1);
    let layout_line_count = displayed_lines.max(min_lines);
    let edges = base_box.unwrap_or_else(|| crate::text::LineBox::untrimmed(line_height, 0.0));
    PreparedLineMetrics {
        height: (layout_line_count as f32 * line_height - edges.trim_top - edges.trim_bottom)
            .max(0.0),
        line_height,
        alignment_lines: cranpose_ui_layout::AlignmentLines::new(
            first,
            first.map(|first| first + (displayed_lines - 1) as f32 * line_height),
        ),
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
        scale_span_style_font_sizes(&mut span.item, curve, None);
    }
    Cow::Owned(scaled)
}

fn scale_text_style_font_sizes(style: &TextStyle, curve: FontScaleCurve) -> Cow<'_, TextStyle> {
    if curve.is_identity() {
        return Cow::Borrowed(style);
    }

    let mut scaled = style.clone();
    scale_span_style_font_sizes(&mut scaled.span_style, curve, Some(DEFAULT_FONT_SIZE_SP));
    scaled.paragraph_style.line_height =
        scale_text_unit_sp(scaled.paragraph_style.line_height, curve);
    if let Some(mut indent) = scaled.paragraph_style.text_indent {
        indent.first_line = scale_text_unit_sp(indent.first_line, curve);
        indent.rest_line = scale_text_unit_sp(indent.rest_line, curve);
        scaled.paragraph_style.text_indent = Some(indent);
    }
    Cow::Owned(scaled)
}

/// `style` at the running app's font scale.
fn system_scaled_style(style: &TextStyle) -> Cow<'_, TextStyle> {
    match crate::render_state::current_scaling_font_scale_curve() {
        Some(curve) => scale_text_style_font_sizes(style, curve),
        None => Cow::Borrowed(style),
    }
}

fn with_system_font_scale<R>(
    text: &crate::text::AnnotatedString,
    style: &TextStyle,
    block: impl FnOnce(&crate::text::AnnotatedString, &TextStyle) -> R,
) -> R {
    let Some(curve) = crate::render_state::current_scaling_font_scale_curve() else {
        return block(text, style);
    };
    let visual_style = scale_text_style_font_sizes(style, curve);
    let visual_text = scale_annotated_font_sizes(text, curve);
    block(visual_text.as_ref(), &visual_style)
}

/// Scales `scaled`'s sizes in place by `curve`.
fn scale_span_style_font_sizes(
    scaled: &mut crate::text::SpanStyle,
    curve: FontScaleCurve,
    default_font_size_sp: Option<f32>,
) {
    let factor = curve.scale();
    scaled.font_size = match (scaled.font_size, default_font_size_sp) {
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
    Ellipsized(crate::text::AnnotatedString),
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
            DisplayLineText::Ellipsized(annotated) => annotated.text.as_str(),
        }
    }

    /// The line's width, measured once and kept.
    fn measure_width<M: TextMeasurer + ?Sized>(
        &mut self,
        measurer: &M,
        node_id: Option<NodeId>,
        source: &crate::text::AnnotatedString,
        style: &TextStyle,
    ) -> f32 {
        *self.measured_width.get_or_insert_with(|| match &self.text {
            DisplayLineText::Source => {
                measurer
                    .measure_subsequence_for_node(node_id, source, self.source_range.clone(), style)
                    .width
            }
            DisplayLineText::Ellipsized(annotated) => {
                measurer.measure_for_node(node_id, annotated, style).width
            }
        })
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

    /// Elides the line to fit `max_width` and returns the widths that cut
    /// it at the same character.
    fn ellipsize<M: TextMeasurer + ?Sized>(
        &mut self,
        measurer: &M,
        node_id: Option<NodeId>,
        source: &crate::text::AnnotatedString,
        style: &TextStyle,
        max_width: Option<f32>,
        placement: EllipsisPlacement,
    ) -> WrapHold {
        let (line, cut) = fit_ellipsis(
            measurer,
            node_id,
            source,
            self.source_range.clone(),
            style,
            max_width,
            placement,
        );
        *self = line;
        cut
    }
}

/// A text's lines, which for most texts is one, kept without an allocation.
type LineRanges = smallvec::SmallVec<[Range<usize>; 1]>;
/// A layout's display lines, kept without an allocation for one line.
type DisplayLines = smallvec::SmallVec<[DisplayLine; 1]>;

fn split_line_ranges(text: &str) -> LineRanges {
    if text.is_empty() {
        return smallvec::smallvec![0..0];
    }

    let mut ranges = LineRanges::new();
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
            DisplayLineText::Ellipsized(annotated) => builder.append_annotated(annotated),
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

/// The max widths a prepared layout comes out the same for, so a node whose
/// width moves can keep one layout instead of preparing it again.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) enum PreparedWidths {
    /// Only the width it was prepared at; `None` is unconstrained.
    Exact(Option<u32>),
    /// No line wrapped or overflowed: unconstrained, and every width from
    /// its measured width up. A narrower width may wrap, so it is not held.
    AtLeast(f32),
    /// Lines wrapped greedily, and every width that breaks them and cuts the
    /// last kept line's ellipsis the same.
    Wrapped(WrapHold),
}

impl PreparedWidths {
    /// The widths `prepared`, made from `text` at `max_width`, holds for.
    pub(crate) fn of(
        text: &crate::text::AnnotatedString,
        options: TextLayoutOptions,
        max_width: Option<f32>,
        prepared: &PreparedTextLayout,
    ) -> Self {
        let max_width = normalize_max_width(max_width);
        let exact = Self::Exact(max_width.map(f32::to_bits));
        let wrapped = prepared.text.text.matches('\n').count() != text.text.matches('\n').count();
        // A line's trailing spaces count when it is fitted but not in the
        // width it reports, so such a line may wrap at its own width.
        let trailing_space = text
            .text
            .split('\n')
            .any(|line| line.ends_with(char::is_whitespace));
        if options
            .normalized()
            .overflow
            .scale_down_min_font_size_sp()
            .is_some()
            || trailing_space
        {
            return exact;
        }
        if wrapped || prepared.did_overflow {
            return match (prepared.wrap_hold, max_width) {
                (Some(hold), Some(width)) if hold.holds(width) => Self::Wrapped(hold),
                _ => exact,
            };
        }
        match max_width {
            Some(width) if prepared.metrics.width >= width => exact,
            _ => Self::AtLeast(prepared.metrics.width),
        }
    }

    /// Whether preparing at `max_width` gives the same layout.
    pub(crate) fn hold(self, max_width: Option<f32>) -> bool {
        let max_width = normalize_max_width(max_width);
        match self {
            Self::Exact(bits) => max_width.map(f32::to_bits) == bits,
            Self::AtLeast(min) => max_width.is_none_or(|width| width >= min),
            Self::Wrapped(hold) => max_width.is_some_and(|width| hold.holds(width)),
        }
    }
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

/// The display lines `line_ranges` wrap into at `max_width`, and when any
/// wrapped greedily, the widths that wrap them the same.
/// How many lines a wrap keeps, and whether the last kept line runs on to
/// its paragraph's end to be elided when text is cut after it.
#[derive(Clone, Copy, Debug)]
struct LineLimit {
    lines: usize,
    elides_last: bool,
}

impl LineLimit {
    const NONE: Self = Self {
        lines: usize::MAX,
        elides_last: false,
    };

    fn of(options: TextLayoutOptions) -> Self {
        Self {
            lines: options.max_lines,
            elides_last: EllipsisPlacement::for_options(options).is_some(),
        }
    }

    /// Whether a wrap with `lines` lines so far has reached the limit: it
    /// then ends with one line that stands for all the cut text.
    fn reached(self, lines: usize) -> bool {
        lines >= self.lines
    }

    /// Whether the next line after `lines` lines is the last kept line and
    /// is elided, so where it breaks does not change the layout.
    fn elides_next(self, lines: usize) -> bool {
        self.elides_last && lines + 1 == self.lines
    }
}

fn wrap_lines<M: TextMeasurer + ?Sized>(
    measurer: &M,
    text: &crate::text::AnnotatedString,
    line_ranges: LineRanges,
    style: &TextStyle,
    (max_width, limit): (f32, LineLimit),
    modes: (LineBreak, Hyphens),
) -> (DisplayLines, Option<WrapHold>) {
    let source_lines = line_ranges.len();
    let mut lines = DisplayLines::with_capacity(source_lines.min(limit.lines.saturating_add(1)));
    let mut hold = Some(WrapHold::ANY);
    for line_range in line_ranges {
        if limit.reached(lines.len()) {
            if lines.len() == limit.lines {
                lines.push(DisplayLine::from_source_range(line_range));
            }
            break;
        }
        wrap_line_to_width(
            measurer,
            text,
            line_range,
            style,
            (max_width, limit, &mut hold),
            modes,
            &mut lines,
        );
    }
    let wrapped = lines.len() != source_lines;
    (lines, hold.filter(|_| wrapped))
}

/// Appends the display lines `line_range` wraps into at `max_width` to
/// `out`: most lines fit whole, and take no allocation of their own. Narrows
/// `hold` to the widths that wrap it the same, or clears it when the wrap is
/// not greedy.
fn wrap_line_to_width<M: TextMeasurer + ?Sized>(
    measurer: &M,
    text: &crate::text::AnnotatedString,
    line_range: Range<usize>,
    style: &TextStyle,
    (max_width, limit, hold): (f32, LineLimit, &mut Option<WrapHold>),
    (line_break, hyphens): (LineBreak, Hyphens),
    out: &mut DisplayLines,
) {
    let line_text = &text.text[line_range.clone()];
    if line_text.is_empty() {
        out.push(DisplayLine::from_source_range(
            line_range.start..line_range.start,
        ));
        return;
    }

    if let Some(measured_width) = measurer.measure_line_width(text, line_range.clone(), style)
        && measured_width <= max_width + WRAP_EPSILON
    {
        WrapHold::fit_whole(hold, measured_width);
        out.push(DisplayLine::from_measured_source_range(
            line_range,
            measured_width,
        ));
        return;
    }

    if matches!(line_break, LineBreak::Heading | LineBreak::Paragraph)
        && line_text.chars().any(char::is_whitespace)
        && wrap_line_with_word_balance(
            measurer,
            text,
            line_range.clone(),
            style,
            max_width,
            line_break,
            out,
        )
    {
        *hold = None;
        return;
    }

    wrap_line_greedy(
        measurer,
        text,
        line_range,
        style,
        (max_width, limit, hold),
        (line_break, hyphens),
        out,
    );
}

fn wrap_line_greedy<M: TextMeasurer + ?Sized>(
    measurer: &M,
    text: &crate::text::AnnotatedString,
    line_range: Range<usize>,
    style: &TextStyle,
    (max_width, limit, hold): (f32, LineLimit, &mut Option<WrapHold>),
    (line_break, hyphens): (LineBreak, Hyphens),
    out: &mut DisplayLines,
) {
    let line_text = &text.text[line_range.clone()];
    let boundaries = char_boundaries(line_text);
    let measure_context =
        LineMeasureContext::new(measurer, text, &line_range, style, boundaries.len());
    if let Some(measured_width) =
        measure_context.prefix_width_for_char_range(0, boundaries.len() - 1)
        && measured_width <= max_width + WRAP_EPSILON
    {
        WrapHold::fit_whole(hold, measured_width);
        out.push(DisplayLine::from_measured_source_range(
            line_range,
            measured_width,
        ));
        return;
    }
    let first = out.len();
    let mut start_idx = 0usize;

    while start_idx < boundaries.len() - 1 {
        if limit.reached(out.len()) {
            out.push(DisplayLine::from_source_range(
                line_range.start + boundaries[start_idx]..line_range.end,
            ));
            return;
        }
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
        narrow_to_break(
            hold,
            &measure_context,
            (line_text, &boundaries),
            (start_idx, best),
            (can_hyphenate, limit.elides_next(out.len())),
        );
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
        out.push(measure_context.display_line_for_char_range(
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

    if out.len() == first {
        out.push(DisplayLine::from_source_range(
            line_range.start..line_range.start,
        ));
    }
}

/// Appends the word-balanced lines of `line_range` to `out` and returns
/// whether it could balance them; when it cannot, `out` is left as it was.
fn wrap_line_with_word_balance<M: TextMeasurer + ?Sized>(
    measurer: &M,
    text: &crate::text::AnnotatedString,
    line_range: Range<usize>,
    style: &TextStyle,
    max_width: f32,
    line_break: LineBreak,
    out: &mut DisplayLines,
) -> bool {
    let line_text = &text.text[line_range.clone()];
    let boundaries = char_boundaries(line_text);
    let measure_context =
        LineMeasureContext::new(measurer, text, &line_range, style, boundaries.len());
    if let Some(measured_width) =
        measure_context.prefix_width_for_char_range(0, boundaries.len() - 1)
        && measured_width <= max_width + WRAP_EPSILON
    {
        out.push(DisplayLine::from_measured_source_range(
            line_range,
            measured_width,
        ));
        return true;
    }
    let breakpoints = collect_word_breakpoints(line_text, &boundaries);
    if breakpoints.len() <= 2 {
        return false;
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

    let first = out.len();
    let mut current = 0usize;
    while current < node_count - 1 {
        let Some(next) = next_index[current] else {
            out.truncate(first);
            return false;
        };
        let start_byte = boundaries[breakpoints[current]];
        let end_byte = boundaries[breakpoints[next]];
        let trimmed_end = trim_segment_end_whitespace(line_text, start_byte, end_byte);
        if trimmed_end <= start_byte {
            out.truncate(first);
            return false;
        }
        let segment_start_idx = breakpoints[current];
        let segment_end_idx = boundary_index_for_byte(&boundaries, trimmed_end);
        out.push(measure_context.display_line_for_char_range(
            &boundaries,
            segment_start_idx,
            segment_end_idx,
        ));
        current = next;
    }
    out.len() > first
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

/// Narrows `hold` to the widths a line from `start_idx` breaks the same at
/// as it does where `best` characters fit, or clears it when the break was
/// hyphenated, which no width range describes.
/// Narrows `hold` to the widths that break the line from `start_idx` at
/// `best` the same. A line that runs on to be elided holds while the rest of
/// its paragraph does not fit on it, wherever it breaks.
fn narrow_to_break<M: TextMeasurer + ?Sized>(
    hold: &mut Option<WrapHold>,
    measure_context: &LineMeasureContext<'_, M>,
    (line, boundaries): (&str, &[usize]),
    (start_idx, best): (usize, usize),
    (hyphenated, elided): (bool, bool),
) {
    let end = boundaries.len() - 1;
    if elided && best < end {
        if let Some(hold) = hold {
            let rest = measure_context.measure_char_range(boundaries, start_idx, end);
            hold.narrow(f32::NEG_INFINITY, rest);
        }
        return;
    }
    if hyphenated {
        *hold = None;
    }
    let Some(hold) = hold else {
        return;
    };
    let (fits, pulls_up) = wrap_break_widths(line, boundaries, start_idx, best);
    let width = |idx| measure_context.measure_char_range(boundaries, start_idx, idx);
    hold.narrow(
        fits.map_or(f32::NEG_INFINITY, width),
        pulls_up.map_or(f32::INFINITY, width),
    );
}

/// The character counts, from `start_idx`, whose widths bound the widths
/// [`choose_wrap_break`] picks the same break at as it does for `best`: the
/// first a line must fit to keep its break (`None` when any width does),
/// and the first that would take the break past it (`None` when none
/// would).
fn wrap_break_widths(
    line: &str,
    boundaries: &[usize],
    start_idx: usize,
    best: usize,
) -> (Option<usize>, Option<usize>) {
    let end = boundaries.len() - 1;
    // A line takes its first character whether it fits or not.
    if best <= start_idx + 1 {
        return (None, (best < end).then_some(best + 1));
    }
    if best >= end {
        return (Some(end), None);
    }
    let after_space = |idx: usize| {
        line[boundaries[idx - 1]..boundaries[idx]]
            .chars()
            .all(char::is_whitespace)
    };
    match (start_idx + 1..=best).rev().find(|&idx| after_space(idx)) {
        Some(wrap_idx) => {
            let next = (best + 1..end).find(|&idx| after_space(idx)).unwrap_or(end);
            (Some(wrap_idx), Some(next))
        }
        None => (Some(best), Some(best + 1)),
    }
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

/// Cuts `visible_lines` to the options' line limit and elides what does not
/// fit, and returns whether anything did not. Narrows `hold` to the widths
/// that cut the ellipsis at the same character, or clears it when a line
/// overflows its width.
fn apply_overflow<M: TextMeasurer + ?Sized>(
    measurer: &M,
    node_id: Option<NodeId>,
    (text, style): (&crate::text::AnnotatedString, &TextStyle),
    options: TextLayoutOptions,
    max_width: Option<f32>,
    (visible_lines, hold): (&mut DisplayLines, &mut Option<WrapHold>),
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
            let cut = last_line.ellipsize(measurer, node_id, text, style, max_width, placement);
            if let Some(hold) = hold {
                hold.narrow(cut.fits, cut.pulls_up);
            }
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
        *hold = None;
        if line_index + 1 == visible_len
            && let Some(placement) = ellipsis
        {
            line.ellipsize(measurer, node_id, text, style, max_width, placement);
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
    /// How many of `kept_chars` stay before the ellipsis and how many after.
    fn split(self, kept_chars: usize) -> (usize, usize) {
        match self {
            Self::End => (kept_chars, 0),
            Self::Start => (0, kept_chars),
            Self::Middle => (kept_chars.div_ceil(2), kept_chars / 2),
        }
    }

    /// The most characters an elided line can keep within `width_limit`,
    /// estimated from the line's prefix widths and the ellipsis's width
    /// without measuring any elided string. Shaping across the cut can move
    /// the real width a little either way, so callers confirm it.
    fn estimated_kept_chars(
        self,
        prefix_widths: &TextLinePrefixWidths,
        ellipsis_width: f32,
        width_limit: f32,
    ) -> Option<usize> {
        let char_count = prefix_widths.char_count();
        let width = |kept_chars: usize| {
            let (head_chars, tail_chars) = self.split(kept_chars);
            Some(
                prefix_widths.width_for_char_range(0, head_chars)?
                    + ellipsis_width
                    + prefix_widths.width_for_char_range(char_count - tail_chars, char_count)?,
            )
        };
        let (mut fitting, mut overflowing) = (0usize, char_count + 1);
        while fitting + 1 < overflowing {
            let kept_chars = fitting + (overflowing - fitting) / 2;
            if width(kept_chars)? <= width_limit + WRAP_EPSILON {
                fitting = kept_chars;
            } else {
                overflowing = kept_chars;
            }
        }
        Some(fitting)
    }

    fn for_options(options: TextLayoutOptions) -> Option<Self> {
        let single_line = options.max_lines == 1;
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

    fn elide(
        self,
        source: &crate::text::AnnotatedString,
        source_range: Range<usize>,
        boundaries: &[usize],
        kept_chars: usize,
    ) -> crate::text::AnnotatedString {
        let char_count = boundaries.len() - 1;
        let (head_chars, tail_chars) = self.split(kept_chars);
        let head_end = source_range.start + boundaries[head_chars];
        let tail_start = source_range.start + boundaries[char_count - tail_chars];
        crate::text::AnnotatedString::builder()
            .append_annotated_subsequence(source, source_range.start..head_end)
            .append(ELLIPSIS)
            .append_annotated_subsequence(source, tail_start..source_range.end)
            .to_annotated_string()
    }
}

/// The line `source_range` elided at `placement` to fit `max_width`, and the
/// widths the same elision fits: from its own width up to the width of the
/// elision keeping one character more.
fn fit_ellipsis<M: TextMeasurer + ?Sized>(
    measurer: &M,
    node_id: Option<NodeId>,
    source: &crate::text::AnnotatedString,
    source_range: Range<usize>,
    style: &TextStyle,
    max_width: Option<f32>,
    placement: EllipsisPlacement,
) -> (DisplayLine, WrapHold) {
    let width_limit = max_width.unwrap_or(f32::INFINITY);
    // The line when it fits, or the width it overflows at.
    let fitting_line = |text: DisplayLineText| {
        let mut line = DisplayLine {
            source_range: source_range.clone(),
            text,
            measured_width: None,
        };
        let width = line.measure_width(measurer, node_id, source, style);
        if width <= width_limit + WRAP_EPSILON {
            Ok(line)
        } else {
            Err(width)
        }
    };
    // Keeping every character elides nothing, so the whole line bounds the
    // widths an elision holds.
    let whole = if placement == EllipsisPlacement::End {
        f32::INFINITY
    } else {
        match fitting_line(DisplayLineText::Source) {
            Ok(line) => return ElisionSearch::found(line, f32::INFINITY),
            Err(width) => width,
        }
    };

    let boundaries = char_boundaries(&source.text[source_range.clone()]);
    let elided_line = |kept_chars: usize| {
        fitting_line(DisplayLineText::Ellipsized(placement.elide(
            source,
            source_range.clone(),
            &boundaries,
            kept_chars,
        )))
    };
    let best = match elided_line(0) {
        Ok(line) => line,
        Err(ellipsis_width) => {
            let empty = DisplayLine {
                source_range: source_range.clone(),
                text: DisplayLineText::Ellipsized(crate::text::AnnotatedString::default()),
                measured_width: None,
            };
            return ElisionSearch::found(empty, ellipsis_width);
        }
    };

    // The line's prefix widths place the cut without measuring an elided
    // string per guess; measuring the guess and the one past it confirms it,
    // and the search below only runs when shaping across the cut moved it.
    let guess = best.measured_width.and_then(|ellipsis_width| {
        measurer
            .measure_line_prefix_widths(source, source_range.clone(), style)
            .filter(|widths| widths.char_count() + 1 == boundaries.len())
            .and_then(|widths| placement.estimated_kept_chars(&widths, ellipsis_width, width_limit))
    });
    let mut search = ElisionSearch {
        best,
        fitting: 0,
        overflowing: boundaries.len(),
        overflow_width: whole,
    };
    if let Some(guess) = guess.filter(|guess| *guess > 0) {
        for kept_chars in [guess, guess + 1] {
            if kept_chars <= search.fitting || kept_chars >= search.overflowing {
                break;
            }
            search.probe(kept_chars, elided_line(kept_chars));
        }
    }
    while search.fitting + 1 < search.overflowing {
        let kept_chars = search.fitting + (search.overflowing - search.fitting) / 2;
        search.probe(kept_chars, elided_line(kept_chars));
    }
    ElisionSearch::found(search.best, search.overflow_width)
}

/// The most characters an elision was found to keep within the width, and
/// the fewest it was found to overflow at.
struct ElisionSearch {
    best: DisplayLine,
    fitting: usize,
    overflowing: usize,
    overflow_width: f32,
}

impl ElisionSearch {
    fn probe(&mut self, kept_chars: usize, line: Result<DisplayLine, f32>) {
        match line {
            Ok(line) => (self.fitting, self.best) = (kept_chars, line),
            Err(width) => (self.overflowing, self.overflow_width) = (kept_chars, width),
        }
    }

    /// `line`, and the widths it fits while the next wider elision, which is
    /// `overflow_width` wide, does not.
    fn found(line: DisplayLine, overflow_width: f32) -> (DisplayLine, WrapHold) {
        let cut = WrapHold {
            fits: line.measured_width.unwrap_or(f32::NEG_INFINITY),
            pulls_up: overflow_width,
        };
        (line, cut)
    }
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
