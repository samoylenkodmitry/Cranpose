#[test]
fn letter_spacing_resolves_to_something_a_measurer_can_use() {
    let style = DrawTextStyle::default().with_font_size(18.0);
    assert_eq!(style.font_size, 18.0);
    assert_eq!(style.resolved_font_size(), 18.0);
    assert_eq!(style.resolved_letter_spacing(), 0.0);

    assert_eq!(
        style
            .clone()
            .with_letter_spacing(1.5)
            .resolved_letter_spacing(),
        1.5
    );
    assert_eq!(
        style
            .clone()
            .with_letter_spacing(-0.5)
            .resolved_letter_spacing(),
        -0.5,
        "tightening is a real request, not an error"
    );
    for broken in [f32::NAN, f32::INFINITY, f32::NEG_INFINITY] {
        assert_eq!(
            style
                .clone()
                .with_letter_spacing(broken)
                .resolved_letter_spacing(),
            0.0
        );
    }

    for broken in [0.0, -12.0, f32::NAN, f32::INFINITY] {
        assert_eq!(
            DrawTextStyle::default()
                .with_font_size(broken)
                .resolved_font_size(),
            DrawTextStyle::DEFAULT_FONT_SIZE
        );
    }
}
use super::*;

#[test]
fn text_style_resolves_degenerate_sizes_to_the_framework_default() {
    for size in [0.0, -12.0, f32::NAN, f32::INFINITY] {
        assert_eq!(
            DrawTextStyle::new(size).resolved_font_size(),
            DrawTextStyle::DEFAULT_FONT_SIZE,
            "font size {size} must not reach a font backend"
        );
    }
    assert_eq!(DrawTextStyle::new(19.0).resolved_font_size(), 19.0);
}

#[test]
fn text_style_line_height_falls_back_to_the_natural_one() {
    let style = DrawTextStyle::new(20.0);
    assert_eq!(style.resolved_line_height(28.0), 28.0);
    assert_eq!(
        style
            .clone()
            .with_line_height(40.0)
            .resolved_line_height(28.0),
        40.0
    );
    assert_eq!(
        style.with_line_height(f32::NAN).resolved_line_height(28.0),
        28.0
    );
}

#[test]
fn empty_font_family_name_means_the_default_family() {
    assert_eq!(
        DrawTextStyle::new(14.0).with_font_family("").font_family,
        None
    );
    assert_eq!(
        DrawTextStyle::new(14.0)
            .with_font_family("Fira Sans")
            .font_family,
        Some("Fira Sans".to_string())
    );
}

#[test]
fn estimated_measurement_grows_with_the_longest_line() {
    let style = DrawTextStyle::new(10.0);
    let one = estimate_text_measurement("AAAA", &style);
    let two = estimate_text_measurement("AAAA\nAAAAAAAA", &style);
    assert_eq!(one.line_count, 1);
    assert_eq!(two.line_count, 2);
    assert!(two.size.width > one.size.width);
    assert!((two.size.height - one.size.height * 2.0).abs() < 1e-3);
}

#[test]
fn estimated_measurement_of_an_empty_string_keeps_one_line_of_metrics() {
    let measurement = estimate_text_measurement("", &DrawTextStyle::new(16.0));
    assert_eq!(measurement.size, Size::ZERO);
    assert_eq!(measurement.line_count, 1);
    assert!(measurement.line_height > 0.0);
    assert!(measurement.first_baseline > 0.0);
}

#[test]
fn estimated_baseline_sits_inside_the_line_slot() {
    let measurement = estimate_text_measurement("Ag", &DrawTextStyle::new(24.0));
    assert!(measurement.first_baseline > 0.0);
    assert!(measurement.first_baseline < measurement.line_height);
}
