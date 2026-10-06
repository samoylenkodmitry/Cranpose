use std::cell::Cell;

use super::*;
use crate::{
    text::{Hyphens, LineBreak, ParagraphStyle, TextUnit},
    text_layout_result::TextLayoutResult,
};

#[test]
fn text_layout_telemetry_env_flag_is_not_process_cached() {
    let source = include_str!("../measure.rs");
    let once_lock = ["Once", "Lock"].concat();
    let cached_init_call = ["get", "_or", "_init"].concat();

    assert!(
        !source.contains(&once_lock) && !source.contains(&cached_init_call),
        "text layout telemetry env flag must be read at the diagnostic boundary"
    );
}

#[test]
fn prepared_layout_cache_distinguishes_visual_styles() {
    let service = TextService::new();
    let text = crate::text::AnnotatedString::from("tinted".to_string());
    let options = TextLayoutOptions::default();

    let mut style = TextStyle::default();
    style.span_style.color = Some(crate::Color(1.0, 0.0, 0.0, 1.0));
    let red = service.prepare_with_options(None, &text, &style, options, None);

    style.span_style.color = Some(crate::Color(0.0, 0.0, 1.0, 1.0));
    let blue = service.prepare_with_options(None, &text, &style, options, None);

    assert_eq!(
        red.visual_style.span_style.color,
        Some(crate::Color(1.0, 0.0, 0.0, 1.0)),
    );
    assert_eq!(
        blue.visual_style.span_style.color,
        Some(crate::Color(0.0, 0.0, 1.0, 1.0)),
        "a color-only style change must not be served a stale prepared layout \
         (measurement hashes ignore visual attributes by design)"
    );
}

#[test]
fn system_font_scale_changes_sp_measurement_and_prepared_text() {
    let _app_context = crate::render_state::app_context_test_scope();
    let text = crate::text::AnnotatedString::from("scale me");
    let style = TextStyle {
        span_style: crate::text::SpanStyle {
            font_size: TextUnit::Sp(10.0),
            ..Default::default()
        },
        ..Default::default()
    };

    let unscaled = measure_text(&text, &style);
    crate::set_font_scale(2.0);
    let scaled = measure_text(&text, &style);
    let prepared = prepare_text_layout(&text, &style, TextLayoutOptions::default(), None);

    assert!((scaled.width - unscaled.width * 2.0).abs() <= f32::EPSILON);
    assert!((scaled.height - unscaled.height * 2.0).abs() <= f32::EPSILON);
    assert_eq!(
        prepared.visual_style.span_style.font_size,
        TextUnit::Sp(20.0)
    );
}

#[test]
fn a_platform_curve_resolves_an_sp_where_the_platform_does_and_not_where_a_multiplier_would() {
    let _app_context = crate::render_state::app_context_test_scope();
    let text = crate::text::AnnotatedString::from("SOLID  next at 3 gold");
    let style = TextStyle {
        span_style: crate::text::SpanStyle {
            font_size: TextUnit::Sp(13.0),
            letter_spacing: TextUnit::Sp(0.4),
            ..Default::default()
        },
        ..Default::default()
    };

    crate::set_font_scale_curve(FontScaleCurve::from_samples(
        1.24,
        &[
            (8.0, 9.92),
            (10.0, 12.4),
            (12.0, 14.88),
            (14.0, 17.84),
            (16.0, 19.36),
            (18.0, 20.88),
            (20.0, 22.88),
            (24.0, 25.92),
            (30.0, 30.0),
            (100.0, 100.0),
        ],
    ));
    let prepared = prepare_text_layout(&text, &style, TextLayoutOptions::default(), None);
    assert_eq!(
        prepared.visual_style.span_style.font_size,
        TextUnit::Sp(16.36)
    );
    assert_eq!(
        prepared.visual_style.span_style.letter_spacing,
        TextUnit::Sp(0.4 * 1.24)
    );
    assert_eq!(crate::current_font_scale(), 1.24);

    crate::set_font_scale(1.24);
    let multiplied = prepare_text_layout(&text, &style, TextLayoutOptions::default(), None);
    assert_eq!(
        multiplied.visual_style.span_style.font_size,
        TextUnit::Sp(13.0 * 1.24)
    );
}

#[test]
fn text_service_cache_retains_large_lazy_text_working_set() {
    let mut cache = PassAgedCache::with_capacity_at_least_one(TEXT_SERVICE_CACHE_CAPACITY);
    let metrics = TextMetrics {
        width: 1.0,
        height: 1.0,
        line_height: 1.0,
        line_count: 1,
    };

    for index in 0..4096u64 {
        cache.push(
            TextBaseCacheKey {
                text_hash: index,
                style_hash: 7,
            },
            metrics,
        );
    }

    for index in 0..4096u64 {
        assert!(
            cache
                .get(&TextBaseCacheKey {
                    text_hash: index,
                    style_hash: 7,
                })
                .is_some(),
            "large lazy text working-set entry {index} was evicted too early"
        );
    }
}

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

    fn layout(&self, text: &crate::text::AnnotatedString, style: &TextStyle) -> TextLayoutResult {
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

fn monospaced_measure(text: &crate::text::AnnotatedString, style: &TextStyle) -> TextMetrics {
    MonospacedTextMeasurer.measure(text, style)
}

fn monospaced_offset_for_position(
    text: &crate::text::AnnotatedString,
    style: &TextStyle,
    x: f32,
    y: f32,
) -> usize {
    MonospacedTextMeasurer.get_offset_for_position(text, style, x, y)
}

fn monospaced_cursor_x_for_offset(
    text: &crate::text::AnnotatedString,
    style: &TextStyle,
    offset: usize,
) -> f32 {
    MonospacedTextMeasurer.get_cursor_x_for_offset(text, style, offset)
}

fn monospaced_layout(text: &crate::text::AnnotatedString, style: &TextStyle) -> TextLayoutResult {
    MonospacedTextMeasurer.layout(text, style)
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
        monospaced_measure(text, style)
    }
    fn get_offset_for_position(
        &self,
        text: &crate::text::AnnotatedString,
        style: &TextStyle,
        x: f32,
        y: f32,
    ) -> usize {
        monospaced_offset_for_position(text, style, x, y)
    }

    fn get_cursor_x_for_offset(
        &self,
        text: &crate::text::AnnotatedString,
        style: &TextStyle,
        offset: usize,
    ) -> f32 {
        monospaced_cursor_x_for_offset(text, style, offset)
    }

    fn layout(&self, text: &crate::text::AnnotatedString, style: &TextStyle) -> TextLayoutResult {
        self.layout_calls.set(self.layout_calls.get() + 1);
        monospaced_layout(text, style)
    }
}

struct CountingPreparedTextMeasurer {
    prepare_calls: Rc<Cell<usize>>,
}

impl CountingPreparedTextMeasurer {
    fn new(prepare_calls: Rc<Cell<usize>>) -> Self {
        Self { prepare_calls }
    }
}

struct PrefixWidthCountingMeasurer {
    prefix_calls: Rc<Cell<usize>>,
    subsequence_calls: Rc<Cell<usize>>,
}

impl PrefixWidthCountingMeasurer {
    fn new(prefix_calls: Rc<Cell<usize>>, subsequence_calls: Rc<Cell<usize>>) -> Self {
        Self {
            prefix_calls,
            subsequence_calls,
        }
    }
}
impl TextMeasurer for PrefixWidthCountingMeasurer {
    fn measure(&self, text: &crate::text::AnnotatedString, style: &TextStyle) -> TextMetrics {
        monospaced_measure(text, style)
    }

    fn measure_subsequence(
        &self,
        text: &crate::text::AnnotatedString,
        range: Range<usize>,
        style: &TextStyle,
    ) -> TextMetrics {
        self.subsequence_calls.set(self.subsequence_calls.get() + 1);
        MonospacedTextMeasurer.measure_subsequence(text, range, style)
    }

    fn measure_line_prefix_widths(
        &self,
        text: &crate::text::AnnotatedString,
        line_range: Range<usize>,
        style: &TextStyle,
    ) -> Option<TextLinePrefixWidths> {
        self.prefix_calls.set(self.prefix_calls.get() + 1);
        MonospacedTextMeasurer.measure_line_prefix_widths(text, line_range, style)
    }
    fn get_offset_for_position(
        &self,
        text: &crate::text::AnnotatedString,
        style: &TextStyle,
        x: f32,
        y: f32,
    ) -> usize {
        monospaced_offset_for_position(text, style, x, y)
    }

    fn get_cursor_x_for_offset(
        &self,
        text: &crate::text::AnnotatedString,
        style: &TextStyle,
        offset: usize,
    ) -> f32 {
        monospaced_cursor_x_for_offset(text, style, offset)
    }

    fn layout(&self, text: &crate::text::AnnotatedString, style: &TextStyle) -> TextLayoutResult {
        monospaced_layout(text, style)
    }
}

struct LineHeightCountingMeasurer {
    measure_calls: Rc<Cell<usize>>,
    line_height_calls: Rc<Cell<usize>>,
}

struct FitProbeCountingMeasurer {
    line_width_calls: Rc<Cell<usize>>,
    prefix_calls: Rc<Cell<usize>>,
}

impl FitProbeCountingMeasurer {
    fn new(line_width_calls: Rc<Cell<usize>>, prefix_calls: Rc<Cell<usize>>) -> Self {
        Self {
            line_width_calls,
            prefix_calls,
        }
    }
}

impl LineHeightCountingMeasurer {
    fn new(measure_calls: Rc<Cell<usize>>, line_height_calls: Rc<Cell<usize>>) -> Self {
        Self {
            measure_calls,
            line_height_calls,
        }
    }
}
impl TextMeasurer for LineHeightCountingMeasurer {
    fn measure(&self, text: &crate::text::AnnotatedString, style: &TextStyle) -> TextMetrics {
        self.measure_calls.set(self.measure_calls.get() + 1);
        monospaced_measure(text, style)
    }

    fn measure_line_prefix_widths(
        &self,
        text: &crate::text::AnnotatedString,
        line_range: Range<usize>,
        style: &TextStyle,
    ) -> Option<TextLinePrefixWidths> {
        MonospacedTextMeasurer.measure_line_prefix_widths(text, line_range, style)
    }

    fn line_height(&self, text: &crate::text::AnnotatedString, style: &TextStyle) -> f32 {
        self.line_height_calls.set(self.line_height_calls.get() + 1);
        MonospacedTextMeasurer.line_height(text, style)
    }
    fn get_offset_for_position(
        &self,
        text: &crate::text::AnnotatedString,
        style: &TextStyle,
        x: f32,
        y: f32,
    ) -> usize {
        monospaced_offset_for_position(text, style, x, y)
    }

    fn get_cursor_x_for_offset(
        &self,
        text: &crate::text::AnnotatedString,
        style: &TextStyle,
        offset: usize,
    ) -> f32 {
        monospaced_cursor_x_for_offset(text, style, offset)
    }

    fn layout(&self, text: &crate::text::AnnotatedString, style: &TextStyle) -> TextLayoutResult {
        monospaced_layout(text, style)
    }
}
impl TextMeasurer for FitProbeCountingMeasurer {
    fn measure(&self, text: &crate::text::AnnotatedString, style: &TextStyle) -> TextMetrics {
        monospaced_measure(text, style)
    }

    fn measure_line_width(
        &self,
        text: &crate::text::AnnotatedString,
        line_range: Range<usize>,
        style: &TextStyle,
    ) -> Option<f32> {
        self.line_width_calls.set(self.line_width_calls.get() + 1);
        MonospacedTextMeasurer.measure_line_width(text, line_range, style)
    }

    fn measure_line_prefix_widths(
        &self,
        text: &crate::text::AnnotatedString,
        line_range: Range<usize>,
        style: &TextStyle,
    ) -> Option<TextLinePrefixWidths> {
        self.prefix_calls.set(self.prefix_calls.get() + 1);
        MonospacedTextMeasurer.measure_line_prefix_widths(text, line_range, style)
    }
    fn get_offset_for_position(
        &self,
        text: &crate::text::AnnotatedString,
        style: &TextStyle,
        x: f32,
        y: f32,
    ) -> usize {
        monospaced_offset_for_position(text, style, x, y)
    }

    fn get_cursor_x_for_offset(
        &self,
        text: &crate::text::AnnotatedString,
        style: &TextStyle,
        offset: usize,
    ) -> f32 {
        monospaced_cursor_x_for_offset(text, style, offset)
    }

    fn layout(&self, text: &crate::text::AnnotatedString, style: &TextStyle) -> TextLayoutResult {
        monospaced_layout(text, style)
    }
}
impl TextMeasurer for CountingPreparedTextMeasurer {
    fn measure(&self, text: &crate::text::AnnotatedString, style: &TextStyle) -> TextMetrics {
        monospaced_measure(text, style)
    }

    fn prepare_with_options_for_node(
        &self,
        _node_id: Option<NodeId>,
        text: &crate::text::AnnotatedString,
        style: &TextStyle,
        options: TextLayoutOptions,
        max_width: Option<f32>,
    ) -> PreparedTextLayout {
        self.prepare_calls.set(self.prepare_calls.get() + 1);
        MonospacedTextMeasurer.prepare_with_options(text, style, options, max_width)
    }
    fn get_offset_for_position(
        &self,
        text: &crate::text::AnnotatedString,
        style: &TextStyle,
        x: f32,
        y: f32,
    ) -> usize {
        monospaced_offset_for_position(text, style, x, y)
    }

    fn get_cursor_x_for_offset(
        &self,
        text: &crate::text::AnnotatedString,
        style: &TextStyle,
        offset: usize,
    ) -> f32 {
        monospaced_cursor_x_for_offset(text, style, offset)
    }

    fn layout(&self, text: &crate::text::AnnotatedString, style: &TextStyle) -> TextLayoutResult {
        monospaced_layout(text, style)
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
fn text_service_reuses_metrics_cache_across_node_ids() {
    let _app_context = crate::render_state::app_context_test_scope();
    let measure_calls = Rc::new(Cell::new(0));
    let layout_calls = Rc::new(Cell::new(0));
    let service = TextService::from_measurer(Rc::new(CountingTextMeasurer::new(
        Rc::clone(&measure_calls),
        Rc::clone(&layout_calls),
    )));
    let text = crate::text::AnnotatedString::from("same lazy item text");
    let style = TextStyle::default();

    let first_metrics = service.measure(Some(7), &text, &style);
    let second_metrics = service.measure(Some(8), &text, &style);

    assert_eq!(first_metrics, second_metrics);
    assert_eq!(measure_calls.get(), 1);
}

#[test]
fn text_service_reuses_prepared_layout_cache_across_node_ids() {
    let _app_context = crate::render_state::app_context_test_scope();
    let prepare_calls = Rc::new(Cell::new(0));
    let service = TextService::from_measurer(Rc::new(CountingPreparedTextMeasurer::new(
        Rc::clone(&prepare_calls),
    )));
    let text = crate::text::AnnotatedString::from("same prepared lazy item text");
    let style = TextStyle::default();
    let options = TextLayoutOptions::default();

    let first = service.prepare_with_options(Some(9), &text, &style, options, Some(120.0));
    let second = service.prepare_with_options(Some(10), &text, &style, options, Some(120.0));

    assert_eq!(first.metrics, second.metrics);
    assert_eq!(prepare_calls.get(), 1);
}

#[test]
fn prepared_text_preserves_width_variants_and_owned_edits() {
    let _app_context = crate::render_state::app_context_test_scope();
    let service = TextService::from_measurer(Rc::new(MonospacedTextMeasurer));
    let text = crate::text::AnnotatedString::builder()
        .push_string_annotation("kind", "label")
        .append("shared prepared text")
        .pop()
        .to_annotated_string();
    let style = TextStyle::default();
    let options = TextLayoutOptions {
        overflow: TextOverflow::Ellipsis,
        soft_wrap: false,
        max_lines: 1,
        ..Default::default()
    };
    let wide = service.prepare_with_options(None, &text, &style, options, Some(1000.0));
    let mut narrow =
        Rc::unwrap_or_clone(service.prepare_with_options(None, &text, &style, options, Some(50.0)));
    let retained = narrow.clone();
    let cached = service.prepare_with_options(None, &text, &style, options, Some(50.0));

    assert_eq!(wide.text.as_ref(), &text);
    assert_ne!(wide.text.text, narrow.text.text);
    assert!(narrow.text.text.ends_with(ELLIPSIS));
    assert!(!narrow.text.string_annotations.is_empty());
    assert_eq!(narrow.text, retained.text);
    assert_eq!(narrow.text, cached.text);

    let edited = Rc::make_mut(&mut narrow.text);
    edited.text = "edited".to_owned();
    edited.string_annotations.clear();
    let reloaded = service.prepare_with_options(None, &text, &style, options, Some(50.0));

    assert_eq!(*reloaded, retained);
    assert_eq!(*cached, retained);
    assert_ne!(narrow.text, retained.text);
    assert_eq!(wide.text.as_ref(), &text);
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

#[test]
fn text_wrapping_uses_prefix_widths_without_subsequence_measurement() {
    let _app_context = crate::render_state::app_context_test_scope();
    let prefix_calls = Rc::new(Cell::new(0));
    let subsequence_calls = Rc::new(Cell::new(0));
    set_text_measurer(PrefixWidthCountingMeasurer::new(
        Rc::clone(&prefix_calls),
        Rc::clone(&subsequence_calls),
    ));
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
    let text = crate::text::AnnotatedString::from("word ".repeat(80).as_str());

    let prepared = prepare_text_layout(&text, &style, options, Some(80.0));

    assert!(prepared.metrics.line_count > 1);
    assert!(
        prefix_calls.get() > 0,
        "wrapping should request a line prefix width plan"
    );
    assert_eq!(
        subsequence_calls.get(),
        0,
        "prefix-capable wrapping should not probe candidate substrings"
    );
}

#[test]
fn text_wrapping_skips_prefix_widths_when_fit_probe_says_line_fits() {
    let _app_context = crate::render_state::app_context_test_scope();
    let line_width_calls = Rc::new(Cell::new(0));
    let prefix_calls = Rc::new(Cell::new(0));
    set_text_measurer(FitProbeCountingMeasurer::new(
        Rc::clone(&line_width_calls),
        Rc::clone(&prefix_calls),
    ));
    let style = TextStyle {
        span_style: crate::text::SpanStyle {
            font_size: TextUnit::Sp(10.0),
            ..Default::default()
        },
        ..Default::default()
    };
    let text = crate::text::AnnotatedString::from("fits without per-glyph prefix widths");

    let prepared = prepare_text_layout(&text, &style, TextLayoutOptions::default(), Some(800.0));

    assert_eq!(prepared.metrics.line_count, 1);
    assert_eq!(line_width_calls.get(), 1);
    assert_eq!(
        prefix_calls.get(),
        0,
        "fitting lines should not allocate prefix-width plans"
    );
}

#[test]
fn prepare_text_layout_uses_line_height_without_full_text_measurement() {
    let _app_context = crate::render_state::app_context_test_scope();
    let measure_calls = Rc::new(Cell::new(0));
    let line_height_calls = Rc::new(Cell::new(0));
    let measurer =
        LineHeightCountingMeasurer::new(Rc::clone(&measure_calls), Rc::clone(&line_height_calls));
    let text = crate::text::AnnotatedString::from(
        "one two three four five six seven eight nine ten eleven twelve",
    );

    let prepared = prepare_text_layout_with_measurer_for_node(
        &measurer,
        Some(7),
        &text,
        &TextStyle::default(),
        TextLayoutOptions::default(),
        Some(96.0),
    );

    assert!(prepared.metrics.height > 0.0);
    assert_eq!(line_height_calls.get(), 1);
    assert_eq!(
        measure_calls.get(),
        0,
        "line-height lookup must not re-measure the whole paragraph"
    );
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
        Some(24.0),
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
        scale_annotated_font_sizes(&plain, FontScaleCurve::linear(0.5)),
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
        scale_annotated_font_sizes(&colored, FontScaleCurve::linear(0.5)),
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

    let scaled = scale_annotated_font_sizes(&text, FontScaleCurve::linear(0.5));
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
fn a_word_that_fits_stays_on_the_line_when_the_space_after_it_fits_too() {
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
    let width_of = |text: &str| {
        measure_text_with_options(
            &crate::text::AnnotatedString::from(text.to_string()),
            &style,
            options,
            None,
        )
        .width
    };
    let fits = width_of("aa bb ");
    let overflows = width_of("aa bb c");
    assert!(overflows > fits, "the fixture needs a real gap here");
    let max_width = (fits + overflows) * 0.5;

    let prepared = prepare_text_layout(
        &crate::text::AnnotatedString::from("aa bb cc".to_string()),
        &style,
        options,
        Some(max_width),
    );
    assert_eq!(prepared.text.text, "aa bb\ncc");
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

fn prepared_as(display: &str, width: f32, did_overflow: bool) -> PreparedTextLayout {
    PreparedTextLayout {
        text: Rc::new(crate::text::AnnotatedString::from(display)),
        visual_style: std::sync::Arc::new(TextStyle::default()),
        metrics: TextMetrics {
            width,
            height: 10.0,
            line_height: 10.0,
            line_count: display.split('\n').count(),
        },
        did_overflow,
        render_text: Default::default(),
        alignment_lines: Default::default(),
        wrap_hold: None,
    }
}

fn widths_of(
    source: &str,
    max_width: Option<f32>,
    prepared: &PreparedTextLayout,
) -> PreparedWidths {
    PreparedWidths::of(
        &crate::text::AnnotatedString::from(source),
        TextLayoutOptions::default(),
        max_width,
        prepared,
    )
}

#[test]
fn a_layout_that_wrapped_nothing_holds_from_its_width_up() {
    let widths = widths_of(
        "two\nlines",
        Some(120.0),
        &prepared_as("two\nlines", 40.0, false),
    );
    assert_eq!(widths, PreparedWidths::AtLeast(40.0));
    for held in [
        None,
        Some(40.0),
        Some(41.5),
        Some(10_000.0),
        Some(f32::INFINITY),
    ] {
        assert!(widths.hold(held), "{held:?} lays out the same");
    }
    assert!(!widths.hold(Some(39.5)), "a narrower width may wrap");
    let unconstrained = widths_of("label", None, &prepared_as("label", 40.0, false));
    assert_eq!(unconstrained, PreparedWidths::AtLeast(40.0));
}

#[test]
fn a_layout_that_wrapped_overflowed_or_filled_its_width_holds_only_that_width() {
    let exact = PreparedWidths::Exact(Some(120.0f32.to_bits()));
    assert_eq!(
        widths_of("a b", Some(120.0), &prepared_as("a\nb", 30.0, false)),
        exact,
        "wrapped"
    );
    assert_eq!(
        widths_of("label", Some(120.0), &prepared_as("lab…", 30.0, true)),
        exact,
        "overflowed"
    );
    assert_eq!(
        widths_of("label", Some(120.0), &prepared_as("label", 120.0, false)),
        exact,
        "clamped to the width it was given"
    );
    assert_eq!(
        widths_of("label ", Some(120.0), &prepared_as("label ", 30.0, false)),
        exact,
        "a trailing space counts when fitting"
    );
    let scaled = PreparedWidths::of(
        &crate::text::AnnotatedString::from("label"),
        TextLayoutOptions {
            overflow: TextOverflow::ScaleDown {
                min_font_size_sp: 8.0,
            },
            ..TextLayoutOptions::default()
        },
        Some(120.0),
        &prepared_as("label", 30.0, false),
    );
    assert_eq!(scaled, exact, "scale-down sizes the font to the width");
    assert!(exact.hold(Some(120.0)));
    assert!(!exact.hold(Some(121.0)));
    assert!(!exact.hold(None));
    assert!(
        PreparedWidths::Exact(None).hold(Some(-1.0)),
        "no usable width is unconstrained"
    );
}

#[test]
fn a_held_width_prepares_the_layout_the_held_one_is() {
    let measurer = MonospacedTextMeasurer;
    let prepare = |text: &crate::text::AnnotatedString, max_width| {
        prepare_text_layout_with_measurer_for_node(
            &measurer,
            None,
            text,
            &TextStyle::default(),
            TextLayoutOptions::default(),
            max_width,
        )
    };
    for source in ["label", "two words", "first line\nsecond one"] {
        let text = crate::text::AnnotatedString::from(source);
        let held = prepare(&text, Some(1000.0));
        let widths = PreparedWidths::of(&text, TextLayoutOptions::default(), Some(1000.0), &held);
        let PreparedWidths::AtLeast(min) = widths else {
            panic!("{source:?} wraps nothing at 1000");
        };
        for width in [Some(min), Some(min + 0.25), Some(min * 3.0), None] {
            assert!(widths.hold(width));
            assert_eq!(prepare(&text, width), held, "{source:?} at {width:?}");
        }
        assert_ne!(
            prepare(&text, Some(min - 20.0)).text.text,
            held.text.text,
            "{source:?} wraps below its width"
        );
    }
}

#[test]
fn a_measurer_without_font_metrics_has_no_line_box() {
    let _app_context = crate::render_state::app_context_test_scope();
    assert_eq!(text_line_box(&TextStyle::default()), None);
}

/// Monospaced widths that count the elided strings measured. An elided
/// string measures `cut_penalty` wider than the line's prefix widths say, as
/// shaping across the cut can make it.
struct EllipsisProbeMeasurer {
    elided_measures: Rc<Cell<usize>>,
    cut_penalty: f32,
}

impl TextMeasurer for EllipsisProbeMeasurer {
    fn measure(&self, text: &crate::text::AnnotatedString, style: &TextStyle) -> TextMetrics {
        let mut metrics = monospaced_measure(text, style);
        if text.text.contains(ELLIPSIS) {
            self.elided_measures.set(self.elided_measures.get() + 1);
            metrics.width += self.cut_penalty;
        }
        metrics
    }

    fn measure_line_prefix_widths(
        &self,
        text: &crate::text::AnnotatedString,
        line_range: Range<usize>,
        style: &TextStyle,
    ) -> Option<TextLinePrefixWidths> {
        MonospacedTextMeasurer.measure_line_prefix_widths(text, line_range, style)
    }

    fn get_offset_for_position(
        &self,
        text: &crate::text::AnnotatedString,
        style: &TextStyle,
        x: f32,
        y: f32,
    ) -> usize {
        monospaced_offset_for_position(text, style, x, y)
    }

    fn get_cursor_x_for_offset(
        &self,
        text: &crate::text::AnnotatedString,
        style: &TextStyle,
        offset: usize,
    ) -> f32 {
        monospaced_cursor_x_for_offset(text, style, offset)
    }

    fn layout(&self, text: &crate::text::AnnotatedString, style: &TextStyle) -> TextLayoutResult {
        monospaced_layout(text, style)
    }
}

/// The most characters `placement` can keep within `max_width`, found by
/// measuring every elided candidate.
fn widest_fitting_elision(
    measurer: &EllipsisProbeMeasurer,
    source: &crate::text::AnnotatedString,
    style: &TextStyle,
    max_width: f32,
    placement: EllipsisPlacement,
) -> String {
    let boundaries = char_boundaries(&source.text);
    (0..boundaries.len())
        .rev()
        .map(|kept| placement.elide(source, 0..source.text.len(), &boundaries, kept))
        .find(|elided| measurer.measure(elided, style).width <= max_width + WRAP_EPSILON)
        .map(|elided| elided.text)
        .unwrap_or_default()
}

fn elided_text(line: DisplayLine) -> String {
    match line.text {
        DisplayLineText::Ellipsized(text) => text.text,
        DisplayLineText::Source => String::from("<source>"),
    }
}

#[test]
fn fit_ellipsis_places_the_cut_from_prefix_widths() {
    let _app_context = crate::render_state::app_context_test_scope();
    let style = TextStyle::default();
    let source = crate::text::AnnotatedString::from("abcdefghij".repeat(30).as_str());
    let max_width = 173.0;
    for placement in [
        EllipsisPlacement::End,
        EllipsisPlacement::Start,
        EllipsisPlacement::Middle,
    ] {
        for cut_penalty in [0.0, 25.0] {
            let elided_measures = Rc::new(Cell::new(0));
            let measurer = EllipsisProbeMeasurer {
                elided_measures: Rc::clone(&elided_measures),
                cut_penalty,
            };
            let (line, _) = fit_ellipsis(
                &measurer,
                None,
                &source,
                0..source.text.len(),
                &style,
                Some(max_width),
                placement,
            );
            let probes = elided_measures.get();
            let expected = widest_fitting_elision(&measurer, &source, &style, max_width, placement);
            assert_eq!(
                elided_text(line),
                expected,
                "{placement:?} with a {cut_penalty} px cut keeps the most that fits"
            );
            if cut_penalty == 0.0 {
                assert!(
                    probes <= 3,
                    "{placement:?}: exact prefix widths confirm the cut in {probes} elided measures, not a search"
                );
            }
        }
    }
}

#[test]
fn a_prepared_layout_converts_its_render_text_once() {
    let layout = PreparedTextLayout {
        text: Rc::new(crate::text::AnnotatedString::from("shown")),
        visual_style: std::sync::Arc::new(TextStyle::default()),
        alignment_lines: Default::default(),
        metrics: TextMetrics {
            width: 10.0,
            height: 10.0,
            line_height: 10.0,
            line_count: 1,
        },
        did_overflow: false,
        render_text: Default::default(),
        wrap_hold: None,
    };
    let untouched = layout.clone();
    let first = layout.render_text();
    assert_eq!(first.text(), "shown");
    assert!(std::sync::Arc::ptr_eq(&first, &layout.render_text()));
    assert_eq!(
        layout, untouched,
        "the converted copy does not change what the layout is"
    );
}

#[test]
fn wrapped_lines_append_after_what_the_caller_holds() {
    let text = crate::text::AnnotatedString::from("alpha beta gamma delta epsilon");
    let style = TextStyle::default();
    let whole = 0..text.text.len();
    let held = DisplayLine::from_source_range(0..0);
    let mut lines = vec![held.clone()];
    wrap_line_to_width(
        &MonospacedTextMeasurer,
        &text,
        whole.clone(),
        &style,
        (f32::MAX, LineLimit::NONE, &mut None),
        (LineBreak::Simple, Hyphens::None),
        &mut lines,
    );
    assert_eq!(lines.len(), 2, "a line that fits takes one display line");
    assert_eq!(lines[0].source_range, held.source_range);
    assert_eq!(lines[1].source_range, whole);
    wrap_line_to_width(
        &MonospacedTextMeasurer,
        &text,
        whole,
        &style,
        (60.0, LineLimit::NONE, &mut None),
        (LineBreak::Simple, Hyphens::None),
        &mut lines,
    );
    assert!(
        lines.len() > 3,
        "a narrow width wraps into several lines: {}",
        lines.len()
    );
    assert_eq!(lines[0].source_range, held.source_range);
}

#[test]
fn a_line_that_cannot_balance_leaves_the_lines_as_they_were() {
    let text = crate::text::AnnotatedString::from("unbreakable");
    let mut lines = vec![DisplayLine::from_source_range(0..0)];
    let balanced = wrap_line_with_word_balance(
        &MonospacedTextMeasurer,
        &text,
        0..text.text.len(),
        &TextStyle::default(),
        10.0,
        LineBreak::Paragraph,
        &mut lines,
    );
    assert!(!balanced, "one word has no breakpoints to balance");
    assert_eq!(lines.len(), 1);
}

/// Counts the measurements it is asked for.
struct CountingMeasurer(Rc<Cell<usize>>);

impl TextMeasurer for CountingMeasurer {
    fn measure(&self, text: &crate::text::AnnotatedString, style: &TextStyle) -> TextMetrics {
        self.0.set(self.0.get() + 1);
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

    fn layout(&self, text: &crate::text::AnnotatedString, style: &TextStyle) -> TextLayoutResult {
        MonospacedTextMeasurer.layout(text, style)
    }
}

#[test]
fn the_text_service_drops_what_layout_passes_stopped_measuring() {
    let measured = Rc::new(Cell::new(0));
    let service = TextService::from_measurer(Rc::new(CountingMeasurer(Rc::clone(&measured))));
    let style = TextStyle::default();
    let shown = crate::text::AnnotatedString::from("shown");
    let gone = crate::text::AnnotatedString::from("scrolled away");
    service.measure(None, &shown, &style);
    service.measure(None, &gone, &style);
    assert_eq!(measured.get(), 2);

    for _ in 0..=cranpose_core::collections::pass_aged::IDLE_PASSES {
        service.begin_layout_pass();
        service.measure(None, &shown, &style);
    }
    assert_eq!(measured.get(), 2, "a text measured every pass stays cached");

    service.measure(None, &gone, &style);
    assert_eq!(
        measured.get(),
        3,
        "a text no pass measured is measured anew"
    );
}

/// Lays `text` out greedily with the monospaced measurer at `max_width`.
fn wrapped_at(text: &str, max_width: f32) -> PreparedTextLayout {
    laid_out_at(text, TextLayoutOptions::default(), max_width)
}

fn laid_out_at(text: &str, options: TextLayoutOptions, max_width: f32) -> PreparedTextLayout {
    prepare_text_layout_with_measurer_for_node(
        &MonospacedTextMeasurer,
        None,
        &crate::text::AnnotatedString::from(text),
        &TextStyle::default(),
        options,
        Some(max_width),
    )
}

/// Sweeps `texts` over widths and checks that every width a layout holds
/// lays it out the same; returns how many layouts held a width range.
fn sweep_held_widths(texts: &[&str], options: TextLayoutOptions) -> usize {
    let mut held = 0;
    for text in texts {
        for step in 0..120 {
            let width = 6.25 + step as f32 * 2.5;
            let prepared = laid_out_at(text, options, width);
            let Some(hold) = prepared.wrap_hold else {
                continue;
            };
            assert!(hold.holds(width), "{text:?} at {width} holds its own width");
            held += 1;
            let first_probe = (hold.fits - WRAP_EPSILON - 4.0).max(0.5);
            for probe_step in 0..200 {
                let probe = first_probe + probe_step as f32 * 0.37;
                if !hold.holds(probe) {
                    continue;
                }
                let probed = laid_out_at(text, options, probe);
                assert_eq!(
                    probed.text.text, prepared.text.text,
                    "{text:?} lays out at {probe} as at {width}: {hold:?}"
                );
                assert_eq!(probed.metrics, prepared.metrics, "{text:?} at {probe}");
                assert_eq!(probed.did_overflow, prepared.did_overflow);
            }
        }
    }
    held
}

#[test]
fn a_greedily_wrapped_layout_comes_out_the_same_at_every_width_it_holds() {
    let texts = [
        "cell 123",
        "alpha beta gamma delta epsilon",
        "a bb ccc dddd eeeee ffffff",
        "supercalifragilistic word",
        "x  y   z",
        "one\ntwo three four five",
    ];
    let held = sweep_held_widths(&texts, TextLayoutOptions::default());
    assert!(held > 100, "only {held} layouts wrapped");
}

#[test]
fn an_elided_layout_comes_out_the_same_at_every_width_it_holds() {
    let texts = [
        "alpha beta gamma delta epsilon zeta eta theta",
        "a bb ccc dddd eeeee ffffff ggggggg hhhhhhhh",
        "supercalifragilistic word and more words after it",
        "x  y   z    w",
        "one\ntwo three four five six seven",
    ];
    for (overflow, max_lines) in [
        (TextOverflow::Ellipsis, 1),
        (TextOverflow::Ellipsis, 2),
        (TextOverflow::Ellipsis, 4),
        (TextOverflow::StartEllipsis, 1),
        (TextOverflow::MiddleEllipsis, 1),
        (TextOverflow::Clip, 2),
    ] {
        let options = TextLayoutOptions {
            overflow,
            max_lines,
            ..TextLayoutOptions::default()
        };
        let held = sweep_held_widths(&texts, options);
        assert!(
            held > 100,
            "only {held} layouts held a width range with {overflow:?} in {max_lines} lines"
        );
    }
}

#[test]
fn a_greedily_wrapped_layout_holds_the_widths_that_break_it_the_same() {
    // "cell 123" breaks into "cell" and "123" at any width from "cell " up to
    // the whole label, less one character.
    let char_width = 14.0 * 0.6;
    let prepared = wrapped_at("cell 123", 5.5 * char_width);
    assert_eq!(prepared.text.text, "cell\n123");
    let widths = PreparedWidths::of(
        &crate::text::AnnotatedString::from("cell 123"),
        TextLayoutOptions::default(),
        Some(5.5 * char_width),
        &prepared,
    );
    for width in [5.0, 6.5, 7.4] {
        assert!(
            widths.hold(Some(width * char_width)),
            "{width} characters break it the same"
        );
    }
    for width in [4.0, 8.0, 20.0] {
        assert!(
            !widths.hold(Some(width * char_width)),
            "{width} characters may break it elsewhere"
        );
    }
    assert!(!widths.hold(None), "unconstrained, it does not wrap");
}

#[test]
fn metrics_at_options_a_text_was_laid_out_at_come_from_its_prepared_layout() {
    let _app_context = crate::render_state::app_context_test_scope();
    let measure_calls = Rc::new(Cell::new(0));
    let layout_calls = Rc::new(Cell::new(0));
    let service = TextService::from_measurer(Rc::new(CountingTextMeasurer::new(
        Rc::clone(&measure_calls),
        Rc::clone(&layout_calls),
    )));
    let text = crate::text::AnnotatedString::from("laid out once");
    let style = TextStyle::default();
    let options = TextLayoutOptions::default();

    let prepared = service.prepare_with_options(Some(3), &text, &style, options, Some(90.0));
    assert!(
        service.options_metrics_cache.borrow().is_empty(),
        "laying a text out keeps its metrics in the prepared layout only"
    );
    let calls = (measure_calls.get(), layout_calls.get());
    let metrics = service.measure_with_options(Some(3), &text, &style, options, Some(90.0));

    assert_eq!(metrics, prepared.metrics);
    assert_eq!(
        (measure_calls.get(), layout_calls.get()),
        calls,
        "the metrics were read from the prepared layout, not measured again"
    );
}
