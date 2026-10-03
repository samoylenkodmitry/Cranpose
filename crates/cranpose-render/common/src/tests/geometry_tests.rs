use super::*;

#[test]
fn union_rect_ignores_empty_rhs() {
    let lhs = Some(Rect {
        x: 1.0,
        y: 2.0,
        width: 3.0,
        height: 4.0,
    });
    let rhs = Rect {
        x: 5.0,
        y: 6.0,
        width: 0.0,
        height: 7.0,
    };

    assert_eq!(union_rect(lhs, rhs), lhs);
}

#[test]
fn union_rect_merges_extents() {
    let lhs = Some(Rect {
        x: 8.0,
        y: 4.0,
        width: 3.0,
        height: 5.0,
    });
    let rhs = Rect {
        x: 2.0,
        y: 7.0,
        width: 12.0,
        height: 4.0,
    };

    assert_eq!(
        union_rect(lhs, rhs),
        Some(Rect {
            x: 2.0,
            y: 4.0,
            width: 12.0,
            height: 7.0,
        })
    );
}

#[test]
fn a_blur_reaches_its_kernel_and_its_scratch_blocks_past_the_source() {
    assert_eq!(blur_reach_px(0.0), 1.0);
    assert_eq!(blur_reach_px(-5.0), 1.0);
    assert_eq!(blur_reach_px(2.0), 6.0);
    assert_eq!(blur_reach_px(10.0), 18.0);
    assert_eq!(blur_reach_px(44.0), 60.0);
    assert_eq!(blur_reach_px(200.0), 144.0);
}

#[test]
fn the_logical_reach_follows_the_device_scale() {
    assert_eq!(blur_reach(2.0, 1.0), 6.0);
    assert!((blur_reach(20.0, 2.25) - 60.0 / 2.25).abs() < 1e-5);
    assert_eq!(blur_reach(2.0, 0.0), 6.0);
    assert_eq!(blur_reach(2.0, f32::NAN), 6.0);
}

#[test]
fn a_minimum_scale_bound_covers_blur_reach_across_scratch_transitions() {
    for minimum_scale in [0.125, 1.0, 4.0, 64.0] {
        for scale in [minimum_scale, minimum_scale * 1.125, minimum_scale * 2.0] {
            for device_radius in [
                0.0, 5.999, 6.0, 6.001, 15.999, 16.0, 16.001, 127.999, 128.0, 128.001, 256.0,
            ] {
                let radius = device_radius / scale;
                let bound = blur_reach_for_minimum_scale(radius, minimum_scale);
                assert!(
                    bound >= blur_reach(radius, scale),
                    "bound {bound} must cover radius {radius} at scale {scale} (minimum {minimum_scale})"
                );
            }
        }
    }
}

#[test]
fn a_minimum_scale_bound_covers_invalid_and_saturated_radii() {
    for minimum_scale in [0.5, 2.0, 8.0] {
        for scale in [minimum_scale, minimum_scale * 1.5] {
            for radius in [f32::NAN, f32::NEG_INFINITY, -5.0, f32::INFINITY] {
                let bound = blur_reach_for_minimum_scale(radius, minimum_scale);
                assert!(
                    bound >= blur_reach(radius, scale),
                    "bound {bound} must cover radius {radius} at scale {scale} (minimum {minimum_scale})"
                );
            }
        }
    }
}

#[test]
fn a_minimum_scale_bound_without_a_valid_minimum_is_unbounded() {
    for minimum_scale in [0.0, -1.0, f32::NAN, f32::INFINITY, f32::NEG_INFINITY] {
        assert_eq!(
            blur_reach_for_minimum_scale(2.0, minimum_scale),
            f32::INFINITY
        );
    }
}

#[test]
fn expand_blurred_rect_applies_margin_and_clip() {
    let expanded = expand_blurred_rect(
        Rect {
            x: 10.0,
            y: 20.0,
            width: 30.0,
            height: 40.0,
        },
        2.0,
        1.0,
        Some(Rect {
            x: 8.0,
            y: 18.0,
            width: 20.0,
            height: 20.0,
        }),
    );

    assert_eq!(
        expanded,
        Some(Rect {
            x: 8.0,
            y: 18.0,
            width: 20.0,
            height: 20.0,
        })
    );
}
