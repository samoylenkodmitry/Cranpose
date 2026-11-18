use super::*;

#[test]
fn icon_backplate_colors_only_the_compact_foreground_core() {
    let blue = Color::from_rgb_u8(0, 122, 255);
    let spec = GlassButtonSpec::glass()
        .with_icon_backplate(blue)
        .with_content_color(Color::WHITE);
    assert_eq!(spec.style, GlassButtonStyle::Glass);
    assert_eq!(spec.icon_backplate, Some(blue));
    assert_eq!(spec.content_color, Some(Color::WHITE));
    assert!(spec.glass.is_none());
    assert!((0.49..=0.51).contains(&ICON_BACKPLATE_DIAMETER_RATIO));
    assert!((0.27..=0.29).contains(&ICON_BACKPLATE_GLYPH_RATIO));

    let colors = crate::theme::LiquidColors::light(blue);
    let material = GlassButtonSpec::glass()
        .resolve_material(&colors, colors.label, 44.0)
        .expect("glass button material");
    assert_eq!(material.resolve(&colors).tint, Color::WHITE.with_alpha(0.2));
}

#[test]
fn neutral_button_surface_has_no_foreground_color_wash() {
    let accent = Color::from_rgb_u8(0, 122, 255);
    for (colors, tint_alpha) in [
        (crate::theme::LiquidColors::light(accent), 0.2),
        (crate::theme::LiquidColors::dark(accent), 0.0),
    ] {
        let material = GlassButtonSpec::glass()
            .resolve_material(&colors, accent, 34.0)
            .expect("glass button material");
        assert_eq!(
            material.resolve(&colors).tint,
            Color::WHITE.with_alpha(tint_alpha)
        );
        assert_eq!(material.foreground, Some(colors.label));
    }
}
