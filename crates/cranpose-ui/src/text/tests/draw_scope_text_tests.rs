use cranpose_ui_graphics::{FontWeight as DrawFontWeight, TextAlign, TextVerticalAlign};

use super::*;

#[test]
fn draw_style_maps_onto_span_attributes() {
    let style = DrawTextStyle::new(23.0)
        .with_font_family("Fira Sans")
        .with_weight(DrawFontWeight::BOLD)
        .with_style(DrawFontStyle::Italic)
        .with_letter_spacing(1.5)
        .with_line_height(30.0);
    let mapped = text_style_for_draw_style(&style);

    assert_eq!(mapped.span_style.font_size, TextUnit::Sp(23.0));
    assert_eq!(mapped.span_style.font_weight, Some(FontWeight::BOLD));
    assert_eq!(mapped.span_style.font_style, Some(FontStyle::Italic));
    assert_eq!(
        mapped.span_style.font_family,
        Some(FontFamily::Named("Fira Sans".to_string()))
    );
    assert_eq!(mapped.span_style.letter_spacing, TextUnit::Sp(1.5));
    assert_eq!(mapped.paragraph_style.line_height, TextUnit::Sp(30.0));
}

#[test]
fn draw_style_leaves_unset_attributes_unspecified() {
    let mapped = text_style_for_draw_style(&DrawTextStyle::new(14.0));
    assert_eq!(mapped.span_style.font_family, None);
    assert!(mapped.span_style.letter_spacing.is_unspecified());
    assert!(mapped.paragraph_style.line_height.is_unspecified());
    assert_eq!(mapped.paragraph_style.line_height_style, None);
    assert_eq!(mapped.span_style.color, None);
}

#[test]
fn a_drawn_run_and_a_composed_text_resolve_the_same_line_box() {
    use crate::{
        text::line_box::{FontExtent, line_box},
        widgets::wear::wear_line_height_style,
    };

    let extent = FontExtent::new(32.0 * 1900.0 / 2048.0, 32.0 * 500.0 / 2048.0, 0.0);
    let drawn = DrawTextStyle::new(32.0)
        .with_line_height(36.0)
        .with_line_height_style(wear_line_height_style());
    let resolved = line_box(&text_style_for_draw_style(&drawn), extent, 36.0, 1.0);
    let composed = line_box(
        &crate::widgets::wear::WearTextStyle::TITLE_MEDIUM
            .resolve(cranpose_ui_graphics::Color::WHITE),
        extent,
        36.0,
        1.0,
    );
    assert_eq!(resolved, composed);
    assert_eq!(resolved.height, 38.0);
    assert_eq!(resolved.baseline, 30.0);

    let unstyled = line_box(
        &text_style_for_draw_style(&DrawTextStyle::new(32.0).with_line_height(36.0)),
        extent,
        36.0,
        1.0,
    );
    assert_ne!(unstyled, resolved);
}

#[test]
fn alignment_never_reaches_the_paragraph_style() {
    let style = DrawTextStyle::new(14.0)
        .with_align(TextAlign::Center)
        .with_vertical_align(TextVerticalAlign::Bottom);
    let mapped = text_style_for_draw_style(&style);
    assert_eq!(
        mapped.paragraph_style.text_align,
        super::super::paragraph::TextAlign::Unspecified
    );
}

#[test]
fn oblique_and_italic_request_the_same_face() {
    let italic =
        text_style_for_draw_style(&DrawTextStyle::new(14.0).with_style(DrawFontStyle::Italic));
    let oblique =
        text_style_for_draw_style(&DrawTextStyle::new(14.0).with_style(DrawFontStyle::Oblique));
    assert_eq!(italic.span_style.font_style, oblique.span_style.font_style);
}

#[test]
fn degenerate_font_sizes_are_resolved_before_they_reach_the_measurer() {
    for size in [0.0, -3.0, f32::NAN] {
        let mapped = text_style_for_draw_style(&DrawTextStyle::new(size));
        assert_eq!(
            mapped.span_style.font_size,
            TextUnit::Sp(DrawTextStyle::DEFAULT_FONT_SIZE)
        );
    }
}

#[test]
fn measuring_without_an_app_context_falls_back_to_the_estimate() {
    let style = DrawTextStyle::new(16.0);
    assert_eq!(
        AppContextTextMeasurer.measure_text("HELLO", &style),
        estimate_text_measurement("HELLO", &style)
    );
}
