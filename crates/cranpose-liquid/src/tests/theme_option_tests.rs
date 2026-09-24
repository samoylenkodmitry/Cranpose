use super::*;

#[test]
fn more_contrast_lifts_the_quiet_colors_toward_the_label() {
    let plain = LiquidColors::light(Color::from_rgb_u8(0, 122, 255));
    let strong = plain.with_more_contrast();
    assert!(strong.secondary_label.a() > plain.secondary_label.a());
    assert!(strong.separator.a() > plain.separator.a());
    assert_eq!(strong.label, plain.label);
    assert_eq!(strong.accent, plain.accent);
}

#[test]
fn bold_text_adds_a_weight_step_and_stops_at_the_top() {
    let ramp = LiquidTypography::default().bolder();
    assert_eq!(
        ramp.body.span_style.font_weight,
        Some(FontWeight::SEMI_BOLD)
    );
    assert_eq!(
        ramp.headline.span_style.font_weight,
        Some(FontWeight::EXTRA_BOLD)
    );
    assert_eq!(
        ramp.large_title.span_style.font_weight,
        Some(FontWeight(900))
    );
    assert_eq!(ramp.body.span_style.font_size, TextUnit::Sp(17.0));
}
