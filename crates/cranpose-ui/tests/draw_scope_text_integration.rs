use std::{cell::RefCell, rc::Rc};

use cranpose_ui::{
    AppContext, DrawCommand, Modifier, collect_slices_from_modifier, execute_draw_commands,
    set_font_scale,
};
use cranpose_ui_graphics::{
    Brush, Color, DrawPrimitive, DrawScope, DrawTextStyle, Point, Rect, Size, TextAlign,
    TextVerticalAlign,
};

// Cranpose Test Mono at `FONT_SIZE` pixels: 0.6 em a character, 1 em a
// line, the baseline 0.8 em down.
const FONT_SIZE: f32 = 10.0;
const CHAR_WIDTH: f32 = 6.0;
const LINE_HEIGHT: f32 = 10.0;
const FIRST_BASELINE: f32 = 8.0;

fn draw_commands(draw: impl Fn(&mut dyn DrawScope) + 'static) -> Vec<DrawCommand> {
    collect_slices_from_modifier(&Modifier::empty().draw_behind(draw))
        .draw_commands()
        .to_vec()
}

fn primitives_from_draw_behind(
    size: Size,
    draw: impl Fn(&mut dyn DrawScope) + 'static,
) -> Vec<DrawPrimitive> {
    let commands = draw_commands(draw);
    AppContext::new().enter(|| execute_draw_commands(&commands, size))
}

fn text_primitive(primitives: &[DrawPrimitive]) -> &cranpose_ui_graphics::TextPrimitive {
    match primitives.first() {
        Some(DrawPrimitive::Text(text)) => text,
        other => panic!("expected a text primitive, got {other:?}"),
    }
}

#[test]
fn draw_behind_text_is_laid_out_with_the_app_s_fonts() {
    let primitives = primitives_from_draw_behind(Size::new(200.0, 100.0), |scope| {
        scope.draw_text_from(
            Point::new(4.0, 5.0),
            Brush::solid(Color::WHITE),
            "SCORE",
            &DrawTextStyle::new(FONT_SIZE),
        );
    });

    let text = text_primitive(&primitives);
    assert_eq!(
        text.rect,
        Rect {
            x: 4.0,
            y: 5.0,
            width: 5.0 * CHAR_WIDTH,
            height: LINE_HEIGHT,
        }
    );
}

#[test]
fn measure_text_reports_the_box_a_later_draw_fills() {
    let reported: Rc<RefCell<Option<cranpose_ui_graphics::TextMeasurement>>> =
        Rc::new(RefCell::new(None));
    let sink = Rc::clone(&reported);
    let primitives = primitives_from_draw_behind(Size::new(300.0, 60.0), move |scope| {
        let style = DrawTextStyle::new(12.0).with_align(TextAlign::Center);
        *sink.borrow_mut() = Some(scope.measure_text("GAME OVER", &style));
        scope.draw_text(Brush::solid(Color::WHITE), "GAME OVER", &style);
    });

    let measurement = reported.borrow().expect("measurement");
    let text = text_primitive(&primitives);
    assert_eq!(text.rect.width, measurement.size.width);
    assert_eq!(text.rect.height, measurement.size.height);
    assert_eq!(text.rect.x, (300.0 - measurement.size.width) / 2.0);
}

#[test]
fn baseline_alignment_uses_the_font_s_baseline() {
    let primitives = primitives_from_draw_behind(Size::new(200.0, 100.0), |scope| {
        scope.draw_text_at(
            Rect {
                x: 0.0,
                y: 50.0,
                width: 200.0,
                height: 0.0,
            },
            Brush::solid(Color::WHITE),
            "Ag",
            &DrawTextStyle::new(FONT_SIZE).with_vertical_align(TextVerticalAlign::Baseline),
        );
    });

    assert_eq!(text_primitive(&primitives).rect.y, 50.0 - FIRST_BASELINE);
}

#[test]
fn an_unchanged_string_draws_one_layout_across_repeated_frames() {
    let commands = draw_commands(|scope: &mut dyn DrawScope| {
        scope.draw_text_from(
            Point::ZERO,
            Brush::solid(Color::WHITE),
            "1234",
            &DrawTextStyle::new(FONT_SIZE),
        );
    });

    let layouts: Vec<_> = AppContext::new().enter(|| {
        (0..60)
            .map(|_| {
                let primitives = execute_draw_commands(&commands, Size::new(100.0, 50.0));
                text_primitive(&primitives).layout.clone()
            })
            .collect()
    });

    assert!(layouts[0].is_some(), "a drawn text carries its layout");
    assert!(
        layouts.iter().all(|layout| *layout == layouts[0]),
        "a stable string must be laid out once and drawn from that layout after that"
    );
}

#[test]
fn draw_scope_text_is_measured_at_the_size_the_style_names() {
    const TEXT: &str = "ABCD";

    let commands = draw_commands(|scope: &mut dyn DrawScope| {
        scope.draw_text_from(
            Point::ZERO,
            Brush::solid(Color::WHITE),
            TEXT,
            &DrawTextStyle::new(FONT_SIZE),
        );
    });

    let primitives = AppContext::new().enter(|| {
        set_font_scale(2.0);
        execute_draw_commands(&commands, Size::new(500.0, 100.0))
    });

    let text = text_primitive(&primitives);
    assert_eq!(text.style.font_size, FONT_SIZE);
    assert_eq!(
        text.rect.width,
        TEXT.chars().count() as f32 * CHAR_WIDTH,
        "the block the caller was told about must be the block the glyphs fill"
    );
}

#[test]
fn text_that_cannot_be_measured_never_reaches_the_primitive_stream() {
    let primitives = primitives_from_draw_behind(Size::new(100.0, 100.0), |scope| {
        let style = DrawTextStyle::new(12.0);
        scope.draw_text(Brush::solid(Color::WHITE), "", &style);
        scope.draw_text(Brush::solid(Color(1.0, 1.0, 1.0, 0.0)), "hidden", &style);
    });

    assert!(primitives.is_empty());
}
