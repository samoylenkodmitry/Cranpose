use cranpose_ui_graphics::{Rect, TransformOrigin};

use super::*;

#[test]
fn layer_transform_to_parent_maps_local_bounds_to_positioned_bounds() {
    let local_bounds = Rect {
        x: 0.0,
        y: 0.0,
        width: 30.0,
        height: 18.0,
    };
    let placement = Point { x: 12.0, y: 9.0 };
    let transform = layer_transform_to_parent(local_bounds, placement, &GraphicsLayer::default());

    assert_eq!(
        transform.bounds_for_rect(local_bounds),
        Rect {
            x: 12.0,
            y: 9.0,
            width: 30.0,
            height: 18.0,
        }
    );
}

#[test]
fn layer_transform_to_parent_applies_rotation_about_transform_origin() {
    let local_bounds = Rect {
        x: 0.0,
        y: 0.0,
        width: 100.0,
        height: 40.0,
    };
    let layer = GraphicsLayer {
        rotation_z: 90.0,
        transform_origin: TransformOrigin::CENTER,
        ..Default::default()
    };

    let transform = layer_transform_to_parent(local_bounds, Point::default(), &layer);
    let mapped = transform.bounds_for_rect(local_bounds);

    assert!((mapped.width - 40.0).abs() < 0.01);
    assert!((mapped.height - 100.0).abs() < 0.01);
}

#[test]
fn layer_transform_to_parent_scales_about_transform_origin() {
    let local_bounds = Rect {
        x: 0.0,
        y: 0.0,
        width: 36.0,
        height: 36.0,
    };
    let placement = Point { x: 54.0, y: 416.0 };
    let small = layer_transform_to_parent(
        local_bounds,
        placement,
        &GraphicsLayer {
            scale: 0.85,
            transform_origin: TransformOrigin::CENTER,
            ..Default::default()
        },
    )
    .bounds_for_rect(local_bounds);
    let large = layer_transform_to_parent(
        local_bounds,
        placement,
        &GraphicsLayer {
            scale: 1.15,
            transform_origin: TransformOrigin::CENTER,
            ..Default::default()
        },
    )
    .bounds_for_rect(local_bounds);

    let small_center_y = small.y + small.height * 0.5;
    let large_center_y = large.y + large.height * 0.5;
    assert!(
        (small_center_y - large_center_y).abs() < 0.01,
        "scale must not move the layer center when transform origin is centered"
    );
}
