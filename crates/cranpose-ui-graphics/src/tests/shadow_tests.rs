use super::*;

#[test]
fn dp_offset_converts_to_px() {
    let offset = DpOffset::new(Dp(4.0), Dp(-2.5));
    let px = offset.to_px(Density::from_scale(2.0));
    assert_eq!(px, Point::new(8.0, -5.0));
}

#[test]
fn shadow_default_matches_black_src_over() {
    let shadow = Shadow::default();
    assert_eq!(shadow.radius, Dp(0.0));
    assert_eq!(shadow.spread, Dp(0.0));
    assert_eq!(shadow.offset, DpOffset::ZERO);
    assert_eq!(shadow.color, Color::BLACK);
    assert_eq!(shadow.brush, None);
    assert!((shadow.alpha - 1.0).abs() < 1e-6);
    assert_eq!(shadow.blend_mode, BlendMode::SrcOver);
}

#[test]
fn shadow_to_scope_uses_density() {
    let shadow = Shadow {
        radius: Dp(5.0),
        spread: Dp(2.0),
        offset: DpOffset::new(Dp(-1.0), Dp(3.0)),
        color: Color::from_rgba_u8(10, 20, 30, 255),
        brush: None,
        alpha: 0.7,
        blend_mode: BlendMode::Overlay,
        cutout: false,
    };
    let scope = shadow.to_scope(Density::from_scale(2.0));
    assert!((scope.radius - 10.0).abs() < 1e-6);
    assert!((scope.spread - 4.0).abs() < 1e-6);
    assert_eq!(scope.offset, Point::new(-2.0, 6.0));
    assert_eq!(scope.color, Color::from_rgba_u8(10, 20, 30, 255));
    assert!((scope.alpha - 0.7).abs() < 1e-6);
    assert_eq!(scope.blend_mode, BlendMode::Overlay);
}
