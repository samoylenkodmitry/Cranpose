use cranpose_ui_graphics::Rect;

use super::{
    axis_aligned_quad_rect, canonicalize_device_coordinate, canonicalized_scaled_quad,
    canonicalized_scaled_rect, translation_stable_anchored_device_pixel_bounds,
};
use crate::rect_to_quad;

#[test]
fn translation_stable_device_bounds_preserve_offscreen_source_origin() {
    let bounds = translation_stable_anchored_device_pixel_bounds(
        Rect {
            x: -12.25,
            y: 8.25,
            width: 34.5,
            height: 10.25,
        },
        None,
        2.0,
        4096,
    )
    .expect("bounds");

    assert_eq!(bounds.x, -25.0);
    assert_eq!(bounds.y, 16.0);
    assert_eq!(bounds.width, 70);
    assert_eq!(bounds.height, 22);
}

#[test]
fn translation_stable_device_bounds_keep_size_across_subpixel_phases() {
    let rect_at = |x: f32| Rect {
        x,
        y: 8.25,
        width: 34.5,
        height: 10.25,
    };
    let scale = 130.0 / 96.0;
    let base = translation_stable_anchored_device_pixel_bounds(rect_at(-12.25), None, scale, 4096)
        .expect("base bounds");
    for step in 1..=12 {
        let moved = translation_stable_anchored_device_pixel_bounds(
            rect_at(-12.25 + step as f32),
            None,
            scale,
            4096,
        )
        .expect("moved bounds");
        assert_eq!((base.width, base.height), (moved.width, moved.height));
    }
}

#[test]
fn anchored_translation_stable_bounds_move_one_pixel_at_fractional_densities() {
    for scale in [1.25, 130.0 / 96.0] {
        let mut origin_y = 127.600_006_f32;
        let mut previous_y = None;

        for step in 0..10 {
            let rect = Rect {
                x: 40.0,
                y: origin_y - 18.0,
                width: 60.0,
                height: 60.0,
            };
            let anchor =
                crate::scene::SnapAnchor::rigid(cranpose_ui_graphics::Point::new(0.0, origin_y));
            let bounds =
                translation_stable_anchored_device_pixel_bounds(rect, Some(anchor), scale, 4096)
                    .expect("anchored shadow bounds");
            if let Some(previous_y) = previous_y {
                assert_eq!(
                    bounds.y,
                    previous_y - 1.0,
                    "anchored bounds jumped at step {step} with scale {scale}"
                );
            }
            previous_y = Some(bounds.y);
            origin_y -= 1.0 / scale;
        }
    }
}

#[test]
fn rigid_snap_keeps_half_pixel_phase_across_one_device_pixel_steps() {
    let scale = 1.25;
    let logical_device_pixel = 1.0 / scale;
    let mut origin = 127.600_006;
    let mut previous_device_origin = None;

    for step in 0..10 {
        let anchor = crate::scene::SnapAnchor::rigid(cranpose_ui_graphics::Point::new(0.0, origin));
        let delta = super::snap_delta_for_anchor(anchor, scale);
        let snapped_device_origin = (origin + delta.y) * scale;
        assert_eq!(
            snapped_device_origin.fract(),
            0.0,
            "step {step} did not snap to a device pixel: origin={origin:?} delta={:?}",
            delta.y
        );
        if let Some(previous) = previous_device_origin {
            assert_eq!(
                previous - snapped_device_origin,
                1.0,
                "step {step} changed the half-pixel rounding direction"
            );
        }
        previous_device_origin = Some(snapped_device_origin);
        origin -= logical_device_pixel;
    }
}

#[test]
fn device_coordinate_canonicalization_absorbs_half_pixel_float_noise() {
    assert_eq!(canonicalize_device_coordinate(338.499_94), 338.5);
    assert_eq!(canonicalize_device_coordinate(338.500_06), 338.5);
    assert_eq!(canonicalize_device_coordinate(f32::INFINITY), f32::INFINITY);
}

#[test]
fn scaled_geometry_canonicalization_preserves_edges_and_quad_topology() {
    let rect = Rect {
        x: 10.000_02,
        y: 20.399_96,
        width: 30.0,
        height: 40.000_03,
    };
    let scaled = canonicalized_scaled_rect(rect, 1.25);
    assert_eq!(scaled.x, 12.5);
    assert_eq!(scaled.y, 25.5);
    assert_eq!(scaled.width, 37.5);
    assert_eq!(scaled.height, 50.0);

    assert_eq!(
        canonicalized_scaled_quad(crate::rect_to_quad(rect), 1.25),
        crate::rect_to_quad(scaled)
    );
}

#[test]
fn axis_aligned_quad_rect_returns_rect_for_cardinal_quad() {
    let quad = rect_to_quad(Rect {
        x: 12.0,
        y: 9.0,
        width: 8.0,
        height: 10.0,
    });

    assert_eq!(
        axis_aligned_quad_rect(quad),
        Some(Rect {
            x: 12.0,
            y: 9.0,
            width: 8.0,
            height: 10.0,
        })
    );
}

#[test]
fn axis_aligned_quad_rect_rejects_skewed_quad() {
    let quad = [[12.0, 9.0], [20.0, 9.5], [12.0, 19.0], [20.0, 19.0]];

    assert_eq!(axis_aligned_quad_rect(quad), None);
}
