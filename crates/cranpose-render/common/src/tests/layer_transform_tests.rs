use cranpose_ui_graphics::{GraphicsLayer, Point, Rect, TransformOrigin};

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

#[test]
fn a_layer_bounded_away_from_the_node_origin_leaves_the_content_where_the_node_is() {
    // A clip declared before an offset: its layer starts where the offset
    // does, 7 and 5 before the node's origin.
    let layer_bounds = Rect {
        x: -7.0,
        y: -5.0,
        width: 20.0,
        height: 14.0,
    };
    let placement = Point { x: 30.0, y: 40.0 };
    let transform = layer_transform_to_parent(layer_bounds, placement, &GraphicsLayer::default());

    assert_eq!(transform.map_point(Point { x: 0.0, y: 0.0 }), placement);
    assert_eq!(
        transform.bounds_for_rect(layer_bounds),
        Rect {
            x: 23.0,
            y: 35.0,
            width: 20.0,
            height: 14.0,
        }
    );
}

#[test]
fn a_layer_bounded_away_from_the_node_origin_pivots_on_its_own_centre() {
    let layer_bounds = Rect {
        x: -10.0,
        y: 0.0,
        width: 20.0,
        height: 10.0,
    };
    let layer = GraphicsLayer {
        rotation_z: 180.0,
        transform_origin: TransformOrigin::CENTER,
        ..Default::default()
    };
    let transform = layer_transform_to_parent(layer_bounds, Point::default(), &layer);

    let turned = transform.map_point(Point { x: 10.0, y: 5.0 });
    assert!(
        (turned.x + 10.0).abs() < 1e-3 && (turned.y - 5.0).abs() < 1e-3,
        "{turned:?}"
    );
}

/// A grid cell near the bottom of a phone screen, turned a few degrees: its
/// transform only turns and moves it, so the renderer draws it in place.
#[test]
fn a_turned_layer_far_from_the_origin_keeps_an_exact_rotation() {
    let local_bounds = Rect {
        x: 1.142_857_2,
        y: 1.142_857_2,
        width: 75.428_57,
        height: 71.238_1,
    };
    let placement = Point {
        x: 696.18,
        y: 2129.6,
    };
    let layer = GraphicsLayer {
        rotation_z: -6.0,
        ..Default::default()
    };
    let [[a, b, _], [c, d, _], perspective] =
        layer_transform_to_parent(local_bounds, placement, &layer).matrix();
    let (sin, cos) = (-6.0f32).to_radians().sin_cos();
    let expected = [cos, -sin, sin, cos, 0.0, 0.0, 1.0];
    let actual = [a, b, c, d, perspective[0], perspective[1], perspective[2]];
    for (value, want) in actual.into_iter().zip(expected) {
        assert!(
            (value - want).abs() <= 4.0 * f32::EPSILON,
            "the linear part is the rotation, whatever the placement: {actual:?} vs {expected:?}"
        );
    }
}

/// The transform takes every corner of the layer where the layer's own
/// scale, turn, tilt and translation put it.
#[test]
fn a_layer_transform_puts_each_corner_where_the_layer_puts_it() {
    let local_bounds = Rect {
        x: -4.0,
        y: 3.0,
        width: 120.0,
        height: 80.0,
    };
    let placement = Point {
        x: 310.0,
        y: 1460.0,
    };
    let layer = GraphicsLayer {
        scale_x: 1.2,
        scale_y: 0.9,
        rotation_x: 25.0,
        rotation_y: -15.0,
        rotation_z: 10.0,
        translation_x: 6.0,
        translation_y: -3.0,
        transform_origin: TransformOrigin {
            pivot_fraction_x: 0.25,
            pivot_fraction_y: 0.75,
        },
        ..Default::default()
    };
    let transform = layer_transform_to_parent(local_bounds, placement, &layer);
    let placed = Rect {
        x: placement.x + local_bounds.x,
        y: placement.y + local_bounds.y,
        ..local_bounds
    };
    let expected = apply_layer_to_quad(placed, placed, &layer);
    for (corner, want) in transform.map_rect(local_bounds).into_iter().zip(expected) {
        assert!(
            (corner[0] - want[0]).abs() < 1e-3 && (corner[1] - want[1]).abs() < 1e-3,
            "{corner:?} vs {want:?}"
        );
    }
}
