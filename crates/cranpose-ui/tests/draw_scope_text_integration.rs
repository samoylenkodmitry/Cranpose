//! `DrawScope` text goes through the app's text measurer.
//!
//! These exercise the whole UI-side path — modifier closure to primitive — with
//! a measurer installed on a real `AppContext`, which is what proves a draw
//! scope built by `draw_behind` picks the app's fonts up rather than the
//! font-free estimate.

use std::cell::RefCell;
use std::rc::Rc;

use cranpose_ui::text::{AnnotatedString, TextLayoutResult};
use cranpose_ui::{
    collect_slices_from_modifier, execute_draw_commands, set_font_scale, set_text_measurer,
    AppContext, DrawCommand, Modifier, TextMeasurer, TextMetrics, TextStyle,
};
use cranpose_ui_graphics::{
    estimate_text_measurement, Brush, Color, DrawPrimitive, DrawScope, Point, Rect, Size,
    TextAlign, TextStyle as DrawTextStyle, TextVerticalAlign,
};

const CHAR_WIDTH: f32 = 6.0;
const LINE_HEIGHT: f32 = 10.0;
const FIRST_BASELINE: f32 = 8.0;

/// Measures a fixed advance per character and records every string it was asked
/// about, so a test can tell whether the draw scope reached it at all.
struct RecordingMeasurer {
    measured: Rc<RefCell<Vec<String>>>,
}

impl TextMeasurer for RecordingMeasurer {
    fn measure(&self, text: &AnnotatedString, _style: &TextStyle) -> TextMetrics {
        self.measured.borrow_mut().push(text.text.clone());
        let line_count = text.text.split('\n').count().max(1);
        let width = text
            .text
            .split('\n')
            .map(|line| line.chars().count() as f32 * CHAR_WIDTH)
            .fold(0.0_f32, f32::max);
        TextMetrics {
            width,
            height: line_count as f32 * LINE_HEIGHT,
            line_height: LINE_HEIGHT,
            line_count,
        }
    }

    fn first_baseline(&self, _style: &TextStyle) -> Option<f32> {
        Some(FIRST_BASELINE)
    }

    fn get_offset_for_position(
        &self,
        _text: &AnnotatedString,
        _style: &TextStyle,
        _x: f32,
        _y: f32,
    ) -> usize {
        0
    }

    fn get_cursor_x_for_offset(
        &self,
        _text: &AnnotatedString,
        _style: &TextStyle,
        _offset: usize,
    ) -> f32 {
        0.0
    }

    fn layout(&self, text: &AnnotatedString, _style: &TextStyle) -> TextLayoutResult {
        TextLayoutResult::monospaced(&text.text, CHAR_WIDTH, LINE_HEIGHT)
    }
}

/// Runs `draw` inside an app context that measures text with
/// [`RecordingMeasurer`], and returns the primitives plus the strings measured.
fn primitives_from_draw_behind(
    size: Size,
    draw: impl Fn(&mut dyn DrawScope) + 'static,
) -> (Vec<DrawPrimitive>, Vec<String>) {
    let app_context = AppContext::new();
    let measured = Rc::new(RefCell::new(Vec::new()));
    let commands: Vec<DrawCommand> =
        collect_slices_from_modifier(&Modifier::empty().draw_behind(draw))
            .draw_commands()
            .to_vec();

    let primitives = app_context.enter(|| {
        set_text_measurer(RecordingMeasurer {
            measured: Rc::clone(&measured),
        });
        execute_draw_commands(&commands, size)
    });
    let measured = measured.borrow().clone();
    (primitives, measured)
}

fn text_primitive(primitives: &[DrawPrimitive]) -> &cranpose_ui_graphics::TextPrimitive {
    match primitives.first() {
        Some(DrawPrimitive::Text(text)) => text,
        other => panic!("expected a text primitive, got {other:?}"),
    }
}

#[test]
fn draw_behind_text_is_measured_by_the_app_context_measurer() {
    let (primitives, measured) = primitives_from_draw_behind(Size::new(200.0, 100.0), |scope| {
        scope.draw_text_from(
            Point::new(4.0, 5.0),
            Brush::solid(Color::WHITE),
            "SCORE",
            &DrawTextStyle::new(12.0),
        );
    });

    assert_eq!(measured, vec!["SCORE".to_string()]);
    let text = text_primitive(&primitives);
    // 5 chars x 6.0 advance, one 10.0 line — the measurer's numbers, not the
    // font-free estimate's.
    assert_eq!(
        text.rect,
        Rect {
            x: 4.0,
            y: 5.0,
            width: 5.0 * CHAR_WIDTH,
            height: LINE_HEIGHT,
        }
    );
    assert_ne!(
        text.rect.width,
        estimate_text_measurement("SCORE", &DrawTextStyle::new(12.0))
            .size
            .width
    );
}

#[test]
fn measure_text_reports_the_box_a_later_draw_fills() {
    let reported: Rc<RefCell<Option<cranpose_ui_graphics::TextMeasurement>>> =
        Rc::new(RefCell::new(None));
    let sink = Rc::clone(&reported);
    let (primitives, _) = primitives_from_draw_behind(Size::new(300.0, 60.0), move |scope| {
        let style = DrawTextStyle::new(12.0).with_align(TextAlign::Center);
        *sink.borrow_mut() = Some(scope.measure_text("GAME OVER", &style));
        scope.draw_text(Brush::solid(Color::WHITE), "GAME OVER", &style);
    });

    let measurement = reported.borrow().expect("measurement");
    let text = text_primitive(&primitives);
    assert_eq!(text.rect.width, measurement.size.width);
    assert_eq!(text.rect.height, measurement.size.height);
    // A caller centering by hand must land on the same x the scope chose.
    assert_eq!(text.rect.x, (300.0 - measurement.size.width) / 2.0);
}

#[test]
fn baseline_alignment_uses_the_measurers_baseline() {
    let (primitives, _) = primitives_from_draw_behind(Size::new(200.0, 100.0), |scope| {
        scope.draw_text_at(
            Rect {
                x: 0.0,
                y: 50.0,
                width: 200.0,
                height: 0.0,
            },
            Brush::solid(Color::WHITE),
            "Ag",
            &DrawTextStyle::new(12.0).with_vertical_align(TextVerticalAlign::Baseline),
        );
    });

    assert_eq!(text_primitive(&primitives).rect.y, 50.0 - FIRST_BASELINE);
}

#[test]
fn an_unchanged_string_measures_once_across_repeated_frames() {
    let app_context = AppContext::new();
    let measured = Rc::new(RefCell::new(Vec::new()));
    let commands: Vec<DrawCommand> = collect_slices_from_modifier(&Modifier::empty().draw_behind(
        |scope: &mut dyn DrawScope| {
            scope.draw_text_from(
                Point::ZERO,
                Brush::solid(Color::WHITE),
                "1234",
                &DrawTextStyle::new(12.0),
            );
        },
    ))
    .draw_commands()
    .to_vec();

    app_context.enter(|| {
        set_text_measurer(RecordingMeasurer {
            measured: Rc::clone(&measured),
        });
        for _ in 0..60 {
            execute_draw_commands(&commands, Size::new(100.0, 50.0));
        }
    });

    assert_eq!(
        measured.borrow().len(),
        1,
        "a stable string must be shaped once and served from the metrics cache after that"
    );
}

/// Reports one em of advance per character, so the font size a measurement was
/// taken at is readable straight off the width.
struct EmWidthMeasurer;

impl TextMeasurer for EmWidthMeasurer {
    fn measure(&self, text: &AnnotatedString, style: &TextStyle) -> TextMetrics {
        let font_size = style.resolve_font_size(14.0);
        TextMetrics {
            width: text.text.chars().count() as f32 * font_size,
            height: font_size,
            line_height: font_size,
            line_count: 1,
        }
    }

    fn first_baseline(&self, style: &TextStyle) -> Option<f32> {
        Some(style.resolve_font_size(14.0) * 0.8)
    }

    fn get_offset_for_position(
        &self,
        _text: &AnnotatedString,
        _style: &TextStyle,
        _x: f32,
        _y: f32,
    ) -> usize {
        0
    }

    fn get_cursor_x_for_offset(
        &self,
        _text: &AnnotatedString,
        _style: &TextStyle,
        _offset: usize,
    ) -> f32 {
        0.0
    }

    fn layout(&self, text: &AnnotatedString, style: &TextStyle) -> TextLayoutResult {
        let font_size = style.resolve_font_size(14.0);
        TextLayoutResult::monospaced(&text.text, font_size, font_size)
    }
}

/// A draw scope's text is measured at the size the style names, whatever the
/// system font scale is.
///
/// The rasterizer takes its font size straight off the primitive's
/// `DrawTextStyle` — that style is documented as fully resolved, and a scene is
/// lowered with `text.style.resolved_font_size()` and nothing else. So a
/// measurement that quietly multiplied by the system font scale would hand the
/// caller a box the glyphs never fill: every wrap decision, every centred label
/// and every hit rect computed from `measure_text` would be out by exactly that
/// factor, on the devices where the setting is not 1.0 and nowhere else.
///
/// A `Text` composable is the other case and stays scaled: its style carries
/// real `Sp` sizes, and its prepared layout hands the SCALED style on to the
/// renderer, so both sides move together there.
#[test]
fn draw_scope_text_is_measured_at_the_size_the_style_names() {
    const FONT_SIZE: f32 = 10.0;
    const TEXT: &str = "ABCD";

    let app_context = AppContext::new();
    let commands: Vec<DrawCommand> = collect_slices_from_modifier(&Modifier::empty().draw_behind(
        |scope: &mut dyn DrawScope| {
            scope.draw_text_from(
                Point::ZERO,
                Brush::solid(Color::WHITE),
                TEXT,
                &DrawTextStyle::new(FONT_SIZE),
            );
        },
    ))
    .draw_commands()
    .to_vec();

    let primitives = app_context.enter(|| {
        set_text_measurer(EmWidthMeasurer);
        set_font_scale(2.0);
        execute_draw_commands(&commands, Size::new(500.0, 100.0))
    });

    let text = text_primitive(&primitives);
    // The size the renderer will rasterize at, untouched by the setting.
    assert_eq!(text.style.font_size, FONT_SIZE);
    assert_eq!(
        text.rect.width,
        TEXT.chars().count() as f32 * FONT_SIZE,
        "the block the caller was told about must be the block the glyphs fill"
    );
}

#[test]
fn text_that_cannot_be_measured_never_reaches_the_primitive_stream() {
    let (primitives, measured) = primitives_from_draw_behind(Size::new(100.0, 100.0), |scope| {
        let style = DrawTextStyle::new(12.0);
        scope.draw_text(Brush::solid(Color::WHITE), "", &style);
        scope.draw_text(Brush::solid(Color(1.0, 1.0, 1.0, 0.0)), "hidden", &style);
    });

    assert!(primitives.is_empty());
    assert!(
        measured.is_empty(),
        "neither empty nor invisible text should cost a measurement"
    );
}
