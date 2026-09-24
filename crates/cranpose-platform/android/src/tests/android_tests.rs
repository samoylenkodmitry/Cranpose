use super::AndroidPlatform;

#[test]
fn default_platform_uses_mdpi_and_zero_input_offset() {
    let platform = AndroidPlatform::new();

    assert_eq!(platform.scale_factor(), 1.0);
    assert_eq!(platform.input_surface_offset_px(), (0.0, 0.0));
    assert_eq!(platform.pointer_position(12.0, 34.0).x, 12.0);
    assert_eq!(platform.pointer_position(12.0, 34.0).y, 34.0);
}

#[test]
fn pointer_position_applies_density() {
    let mut platform = AndroidPlatform::new();
    platform.set_scale_factor(2.0);

    let logical = platform.pointer_position(80.0, 120.0);

    assert_eq!(platform.scale_factor(), 2.0);
    assert_eq!(logical.x, 40.0);
    assert_eq!(logical.y, 60.0);
}

#[test]
fn pointer_position_applies_surface_offset_before_density() {
    let mut platform = AndroidPlatform::new();
    platform.set_scale_factor(1.5);
    platform.set_input_surface_offset_px(39.0, 21.0);

    let logical = platform.pointer_position(111.0, 129.0);

    assert_eq!(platform.input_surface_offset_px(), (39.0, 21.0));
    assert_eq!(logical.x, 100.0);
    assert_eq!(logical.y, 100.0);
}
