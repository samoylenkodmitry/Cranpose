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
    let mut cache = BoundedTextCache::new(TEXT_SERVICE_CACHE_CAPACITY);
    let metrics = TextMetrics {
        width: 1.0,
        height: 1.0,
        line_height: 1.0,
        line_count: 1,
    };

    for index in 0..4096u64 {
        cache.insert(
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
fn prepared_text_sharing_preserves_width_variants_and_owned_edits() {
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
    assert!(Rc::ptr_eq(&narrow.text, &retained.text));
    assert!(Rc::ptr_eq(&narrow.text, &cached.text));
    assert!(!Rc::ptr_eq(&wide.text, &narrow.text));

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
        visual_style: TextStyle::default(),
        metrics: TextMetrics {
            width,
            height: 10.0,
            line_height: 10.0,
            line_count: display.split('\n').count(),
        },
        did_overflow,
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
