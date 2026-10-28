use super::*;
use crate::{
    font_scale::FontScaleCurve,
    text::{SpanStyle, TextOverflow, TextUnit},
};

// Cranpose Test Mono at `size` sp: 0.6 of it a character, all of it a line.
fn sized(size: f32) -> TextStyle {
    TextStyle {
        span_style: SpanStyle {
            font_size: TextUnit::Sp(size),
            ..Default::default()
        },
        ..Default::default()
    }
}

fn scaled_down_to(min_font_size_sp: f32) -> TextLayoutOptions {
    TextLayoutOptions {
        overflow: TextOverflow::ScaleDown { min_font_size_sp },
        soft_wrap: false,
        max_lines: 1,
        min_lines: 1,
    }
}

fn prepare(
    text: &str,
    style: &TextStyle,
    options: TextLayoutOptions,
    max_width: Option<f32>,
) -> Rc<PreparedTextLayout> {
    prepare_text_layout(&AnnotatedString::from(text), style, options, max_width)
}

#[test]
fn a_color_change_lays_out_with_the_new_visual_style() {
    let _app_context = crate::render_state::app_context_test_scope();
    let mut style = TextStyle::default();
    style.span_style.color = Some(crate::Color(1.0, 0.0, 0.0, 1.0));
    let red = prepare("tinted", &style, TextLayoutOptions::default(), None);
    style.span_style.color = Some(crate::Color(0.0, 0.0, 1.0, 1.0));
    let blue = prepare("tinted", &style, TextLayoutOptions::default(), None);

    assert_eq!(
        red.visual_style.span_style.color,
        Some(crate::Color(1.0, 0.0, 0.0, 1.0))
    );
    assert_eq!(
        blue.visual_style.span_style.color,
        Some(crate::Color(0.0, 0.0, 1.0, 1.0)),
        "a color-only style change must not be served a stale prepared layout"
    );
}

#[test]
fn a_platform_curve_resolves_an_sp_where_the_platform_does_and_not_where_a_multiplier_would() {
    let _app_context = crate::render_state::app_context_test_scope();
    let style = TextStyle {
        span_style: SpanStyle {
            font_size: TextUnit::Sp(13.0),
            letter_spacing: TextUnit::Sp(0.4),
            ..Default::default()
        },
        ..Default::default()
    };
    let text = "SOLID  next at 3 gold";

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
    let curved = prepare(text, &style, TextLayoutOptions::default(), None);
    crate::set_font_scale(1.24);
    let multiplied = prepare(text, &style, TextLayoutOptions::default(), None);

    let glyph_size = |prepared: &PreparedTextLayout| {
        prepared
            .layout
            .glyph_runs(0)
            .next()
            .expect("a run of glyphs")
            .size
    };
    assert!((glyph_size(&curved) - 16.36).abs() < 0.01);
    assert!((glyph_size(&multiplied) - 13.0 * 1.24).abs() < 0.01);
}

#[test]
fn visible_overflow_without_soft_wrap_keeps_the_text_on_one_line() {
    let _app_context = crate::render_state::app_context_test_scope();
    let text = "This should remain unchanged";
    let options = TextLayoutOptions {
        overflow: TextOverflow::Visible,
        soft_wrap: false,
        max_lines: 1,
        min_lines: 1,
    };

    let prepared = prepare(text, &sized(10.0), options, Some(10.0));

    assert_eq!(prepared.layout.line_count(), 1);
    assert_eq!(prepared.layout.line_end(0), text.len());
}

#[test]
fn min_lines_make_room_for_lines_the_text_lacks() {
    let _app_context = crate::render_state::app_context_test_scope();
    let options = TextLayoutOptions {
        overflow: TextOverflow::Clip,
        soft_wrap: true,
        max_lines: 4,
        min_lines: 3,
    };

    let prepared = prepare("short", &sized(10.0), options, Some(100.0));

    assert_eq!(prepared.metrics.line_count, 3);
    assert_eq!(prepared.metrics.height, 30.0);
}

#[test]
fn scale_down_shrinks_the_font_until_the_text_fits() {
    let _app_context = crate::render_state::app_context_test_scope();

    let prepared = prepare("ABCDE", &sized(20.0), scaled_down_to(10.0), Some(36.0));

    assert!(prepared.metrics.width <= 36.0, "{:?}", prepared.metrics);
    assert!(!prepared.did_overflow);
    let font_size = prepared.visual_style.resolve_font_size(14.0);
    assert!((10.0..20.0).contains(&font_size), "{font_size}");
}

#[test]
fn scale_down_scales_the_shadow_with_the_font() {
    let _app_context = crate::render_state::app_context_test_scope();
    let mut style = sized(20.0);
    style.span_style.shadow = Some(crate::text::Shadow {
        color: crate::modifier::Color(0.0, 0.0, 0.0, 1.0),
        offset: crate::modifier::Point::new(8.0, 4.0),
        blur_radius: 6.0,
    });

    let prepared = prepare("ABCDE", &style, scaled_down_to(10.0), Some(36.0));

    let scale = prepared.visual_style.resolve_font_size(14.0) / 20.0;
    let shadow = prepared
        .visual_style
        .span_style
        .shadow
        .expect("the scaled style keeps its shadow");
    assert!((shadow.offset.x - 8.0 * scale).abs() < 1e-4);
    assert!((shadow.offset.y - 4.0 * scale).abs() < 1e-4);
    assert!((shadow.blur_radius - 6.0 * scale).abs() < 1e-4);
}

#[test]
fn scale_down_stops_at_its_minimum_and_overflows() {
    let _app_context = crate::render_state::app_context_test_scope();

    let prepared = prepare("ABCDEFGHIJ", &sized(20.0), scaled_down_to(10.0), Some(12.0));

    assert!(prepared.did_overflow);
    assert_eq!(prepared.visual_style.resolve_font_size(14.0), 10.0);
}

#[test]
fn a_text_laid_out_at_the_width_it_measured_keeps_its_lines() {
    let _app_context = crate::render_state::app_context_test_scope();
    let style = sized(14.0);
    let text = "if counter % 2 == 0";
    let width = measure_text(&AnnotatedString::from(text), &style).width;

    let prepared = prepare(text, &style, TextLayoutOptions::default(), Some(width));

    assert_eq!(prepared.layout.line_count(), 1);
}

#[test]
fn text_of_mixed_font_sizes_wraps_without_losing_text() {
    let _app_context = crate::render_state::app_context_test_scope();
    let text = AnnotatedString::builder()
        .append("You can also ")
        .push_style(SpanStyle {
            font_size: TextUnit::Sp(22.0),
            ..Default::default()
        })
        .append("change font size")
        .pop()
        .append(" dynamically mid-sentence!")
        .to_annotated_string();

    let prepared = prepare_text_layout(
        &text,
        &sized(14.0),
        TextLayoutOptions::default(),
        Some(260.0),
    );

    let layout = &prepared.layout;
    assert!(layout.line_count() > 1);
    assert_eq!(layout.line_end(layout.line_count() - 1), text.text.len());
    assert!(!prepared.did_overflow);
}
