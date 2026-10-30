use super::*;

#[test]
fn app_badge_geometry_honors_the_tab_optical_scale() {
    let full = app_badge_geometry(1.0);
    let corrected = app_badge_geometry(0.85);
    assert_eq!(full.size, Size::new(20.0, 32.0));
    assert_eq!(corrected.size, Size::new(17.0, 27.2));
    assert!((corrected.glyph.width - 11.9).abs() < 1.0e-5);
    assert!((corrected.corner_radius / full.corner_radius - 0.85).abs() < f32::EPSILON);
    assert!((corrected.stripe.x / full.stripe.x - 0.85).abs() < f32::EPSILON);
    assert!((corrected.glyph.width / full.glyph.width - 0.85).abs() < f32::EPSILON);
}
