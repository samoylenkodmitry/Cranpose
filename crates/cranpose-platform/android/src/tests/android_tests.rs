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

#[test]
fn physical_rect_inverts_pointer_position_and_covers_whole_pixels() {
    let mut platform = AndroidPlatform::new();
    platform.set_scale_factor(2.75);
    platform.set_input_surface_offset_px(10.0, 4.0);
    let rect = cranpose_ui_graphics::Rect {
        x: 20.1,
        y: 30.0,
        width: 100.0,
        height: 40.3,
    };

    let [left, top, width, height] = platform.physical_rect(rect);

    let top_left = platform.pointer_position(f64::from(left), f64::from(top));
    let bottom_right = platform.pointer_position(f64::from(left + width), f64::from(top + height));
    assert!(top_left.x <= rect.x && top_left.y <= rect.y);
    assert!(bottom_right.x >= rect.x + rect.width && bottom_right.y >= rect.y + rect.height);
    assert!(rect.x - top_left.x < 1.0 / 2.75);
    assert!(bottom_right.y - (rect.y + rect.height) < 1.0 / 2.75);
}
