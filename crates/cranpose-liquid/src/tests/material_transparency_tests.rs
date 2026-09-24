use super::*;

#[test]
fn less_transparency_makes_glass_a_flat_surface() {
    let colors = crate::theme::LiquidColors::light(Color::from_rgb_u8(0, 122, 255));
    let flat = Glass::regular()
        .resolve(&colors)
        .without_transparency(&colors);
    assert_eq!(flat.tint, colors.surface);
    assert_eq!(flat.blur_radius_dp, 0.0);
    assert_eq!(flat.refraction_depth, 0.0);
    assert_eq!(flat.saturation, 1.0);
    assert!(flat.edge_spectrum.is_none());
    let clear = Glass::regular().resolve(&colors);
    assert!(clear.blur_radius_dp > 0.0, "plain glass keeps its blur");
}
