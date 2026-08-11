use cranpose_ui::round_scaling_list::{place_row, scale_and_alpha};

#[test]
fn public_scaling_geometry_preserves_full_height_row_offsets() {
    let viewport = 227.0;
    let first = place_row(viewport, 0.0, 50.0, 2.0).unwrap();
    let second = place_row(viewport, 50.0, 50.0, 2.0).unwrap();

    assert!(first.scale < 1.0);
    assert!(second.top >= 49.0);
    assert_eq!(scale_and_alpha(f32::NAN, 0.0, 50.0), None);
}
