use cranpose_ui_graphics::Rect;

use super::{
    SegmentTransform, axis_aligned_quad_rect, canonicalize_device_coordinate,
    canonicalized_scaled_quad, canonicalized_scaled_rect,
    translation_stable_anchored_device_pixel_bounds, truncated_round,
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
fn device_coordinate_canonicalization_matches_the_f64_snap_everywhere() {
    let snapped_in_f64 = |value: f32| ((f64::from(value) * 16.0).round() / 16.0) as f32;
    let mut values = vec![
        0.0,
        -0.0,
        0.031_25,
        -0.031_25,
        0.093_75,
        f32::MIN_POSITIVE,
        f32::MAX,
        f32::MAX / 16.0,
        -f32::MAX / 15.0,
        (1u32 << 19) as f32 + 0.031_25,
        (1u32 << 20) as f32 + 0.5,
        (1u32 << 24) as f32 + 2.0,
    ];
    let mut bits = 0x3f80_0000_u32;
    for _ in 0..200_000 {
        bits = bits.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
        let value = f32::from_bits(bits);
        if value.is_finite() {
            values.push(value);
        }
    }
    for value in values {
        assert_eq!(
            canonicalize_device_coordinate(value).to_bits(),
            snapped_in_f64(value).to_bits(),
            "{value:e}"
        );
    }
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

fn quarter_turn_then_move() -> SegmentTransform {
    SegmentTransform::affine([0.0, -1.0, 1.0, 0.0], [10.0, 20.0]).expect("a turn is invertible")
}

fn rect(x: f32, y: f32, width: f32, height: f32) -> Rect {
    Rect {
        x,
        y,
        width,
        height,
    }
}

#[test]
fn the_identity_segment_transform_leaves_rects_and_the_uniform_as_they_are() {
    let identity = SegmentTransform::IDENTITY;
    let area = rect(-3.5, 2.25, 10.0, 4.0);
    assert!(identity.is_identity());
    assert_eq!(identity.target_bounds(area), area);
    assert_eq!(identity.segment_bounds(area), area);
    assert_eq!(
        identity.uniform_parts(),
        ([1.0, 0.0, 0.0, 1.0], [0.0, 0.0], [1.0, 0.0, 0.0, 1.0])
    );
    assert!(
        SegmentTransform::affine([1.0, 0.0, 0.0, 1.0], [0.0, 0.0])
            .expect("the identity is invertible")
            .is_identity()
    );
}

#[test]
fn a_turned_segment_maps_rects_to_their_turned_bounds_and_back() {
    let turn = quarter_turn_then_move();
    assert!(!turn.is_identity());
    let turned = turn.target_bounds(rect(0.0, 0.0, 4.0, 2.0));
    assert_eq!(turned, rect(8.0, 20.0, 2.0, 4.0));
    assert_eq!(turn.segment_bounds(turned), rect(0.0, 0.0, 4.0, 2.0));
    assert_eq!(
        turn.uniform_parts(),
        ([0.0, -1.0, 1.0, 0.0], [10.0, 20.0], [0.0, 1.0, -1.0, 0.0])
    );
}

#[test]
fn a_singular_linear_part_is_no_segment_transform() {
    assert!(SegmentTransform::affine([1.0, 2.0, 2.0, 4.0], [0.0, 0.0]).is_none());
    assert!(SegmentTransform::affine([f32::NAN, 0.0, 0.0, 1.0], [0.0, 0.0]).is_none());
}

#[test]
fn composed_segment_transforms_apply_the_inner_one_first() {
    let shift = SegmentTransform::affine([1.0, 0.0, 0.0, 1.0], [5.0, 0.0]).expect("a move");
    let composed = shift.then(quarter_turn_then_move());
    let unit = rect(0.0, 0.0, 1.0, 1.0);
    assert_eq!(composed.target_bounds(unit), rect(9.0, 25.0, 1.0, 1.0));
    assert_eq!(composed.segment_bounds(rect(9.0, 25.0, 1.0, 1.0)), unit);
}

#[test]
fn truncated_round_matches_round_on_ties_signs_and_extremes() {
    let values = [
        0.0,
        -0.0,
        0.5,
        -0.5,
        1.5,
        -1.5,
        2.5,
        -2.5,
        0.49999999999999994,
        -0.49999999999999994,
        0.5000000000000001,
        0.3,
        -0.3,
        0.7,
        -0.7,
        4503599627370495.5,
        -4503599627370495.5,
        4503599627370497.0,
        9007199254740993.0,
        f64::MAX,
        f64::MIN,
        f64::MIN_POSITIVE,
        -f64::MIN_POSITIVE,
        f64::INFINITY,
        f64::NEG_INFINITY,
        12345.0625 * 16.0,
        -98765.4375 * 16.0,
    ];
    for value in values {
        let expected = value.round();
        let actual = truncated_round(value);
        assert_eq!(
            actual.to_bits(),
            expected.to_bits(),
            "{value:e}: {actual:e} against {expected:e}"
        );
    }
    assert!(truncated_round(f64::NAN).is_nan());
    for step in -4000..4000 {
        let value = f64::from(step) / 16.0 + f64::from(step) * 1e-9;
        assert_eq!(
            truncated_round(value).to_bits(),
            value.round().to_bits(),
            "{value:e}"
        );
    }
}
