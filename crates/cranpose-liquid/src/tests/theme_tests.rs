use super::*;

#[test]
fn palettes_differ_and_share_accent() {
    let accent = Color::from_rgb_u8(0, 122, 255);
    let light = LiquidColors::light(accent);
    let dark = LiquidColors::dark(accent);
    assert!(!light.is_dark);
    assert!(dark.is_dark);
    assert_eq!(light.accent, dark.accent);
    assert_ne!(light.background, dark.background);
    assert_ne!(light.glass_tint, dark.glass_tint);
    assert_eq!(light.toggle_on, accent);
    assert_eq!(dark.toggle_on, accent);
    assert_ne!(light.toggle_off, dark.toggle_off);
}

#[test]
fn type_ramp_is_descending() {
    let t = LiquidTypography::default();
    let sizes = [
        &t.large_title,
        &t.title1,
        &t.title2,
        &t.title3,
        &t.body,
        &t.footnote,
        &t.caption2,
    ];
    let values: Vec<f32> = sizes
        .iter()
        .map(|style| style.span_style.font_size.value())
        .collect();
    assert!(values.windows(2).all(|pair| pair[0] >= pair[1]));
}
