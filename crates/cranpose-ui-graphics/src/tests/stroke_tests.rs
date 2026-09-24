use std::f32::consts::{FRAC_PI_2, PI};

use super::*;

fn approx(a: f32, b: f32) -> bool {
    (a - b).abs() < 0.15
}

#[test]
fn scaling_an_arc_moves_its_centre_and_its_radii_and_nothing_else() {
    let arc = ArcGeometry::new(
        Point { x: 10.0, y: 20.0 },
        4.0,
        10.0,
        FRAC_PI_2,
        PI,
        StrokeCap::Round,
    );
    let moved = arc.scaled_about(Point { x: 100.0, y: 200.0 }, 2.5);

    assert_eq!(moved.center, Point { x: 100.0, y: 200.0 });
    assert_eq!(moved.inner_radius, 10.0);
    assert_eq!(moved.outer_radius, 25.0);
    assert_eq!(moved.start_angle, arc.start_angle);
    assert_eq!(moved.sweep_angle, arc.sweep_angle);
    assert_eq!(moved.cap, arc.cap);

    let same = arc.scaled_about(arc.center, 1.0);
    assert_eq!(same, arc);
}

#[test]
fn exact_floor_is_bit_equal_to_floorf() {
    let mut probes: Vec<f32> = vec![
        0.0,
        -0.0,
        0.5,
        -0.5,
        1.0,
        -1.0,
        8_388_607.5,
        -8_388_607.5,
        8_388_608.0,
        -8_388_608.0,
        1.0e30,
        -1.0e30,
        f32::INFINITY,
        f32::NEG_INFINITY,
        f32::MIN_POSITIVE,
        -f32::MIN_POSITIVE,
    ];
    for i in -4000..4000 {
        probes.push(i as f32 * 0.01737);
        probes.push(i as f32 * PI);
    }
    for x in probes {
        assert_eq!(
            exact_floor(x).to_bits(),
            x.floor().to_bits(),
            "exact_floor({x}) diverged from floorf"
        );
    }
    assert!(exact_floor(f32::NAN).is_nan());
}

#[test]
fn stroke_builders_compose() {
    let stroke = Stroke::new(4.0)
        .with_cap(StrokeCap::Round)
        .with_join(StrokeJoin::Bevel);
    assert_eq!(stroke.width, 4.0);
    assert_eq!(stroke.cap, StrokeCap::Round);
    assert_eq!(stroke.join, StrokeJoin::Bevel);
    assert_eq!(stroke.half_width(), 2.0);
    assert!(stroke.is_visible());
    assert_eq!(Stroke::default(), Stroke::new(1.0));
    assert_eq!(Stroke::new(4.0).with_width(6.0).width, 6.0);
}

#[test]
fn stroke_rejects_non_positive_and_non_finite_widths() {
    assert!(!Stroke::new(0.0).is_visible());
    assert!(!Stroke::new(-3.0).is_visible());
    assert!(!Stroke::new(f32::NAN).is_visible());
    assert!(!Stroke::new(f32::INFINITY).is_visible());
    assert_eq!(Stroke::new(f32::NAN).half_width(), 0.0);
    assert_eq!(Stroke::new(-3.0).half_width(), 0.0);
}

#[test]
fn arc_geometry_normalizes_negative_sweeps() {
    let arc = ArcGeometry::new(Point::ZERO, 1.0, 2.0, PI, -FRAC_PI_2, StrokeCap::Butt);
    assert!(approx(arc.start_angle, PI - FRAC_PI_2));
    assert!(approx(arc.sweep_angle, FRAC_PI_2));
}

#[test]
fn arc_geometry_clamps_full_turns_and_forces_round_caps() {
    let arc = ArcGeometry::new(Point::ZERO, 1.0, 2.0, 0.3, TAU * 3.0, StrokeCap::Butt);
    assert_eq!(arc.sweep_angle, TAU);
    assert_eq!(
        arc.cap,
        StrokeCap::Round,
        "a closed ring must not clip its (invisible) caps"
    );
    assert!(arc.contains_angle(0.0));
    assert!(arc.contains_angle(PI));
}

#[test]
fn arc_geometry_sanitizes_non_finite_input() {
    for arc in [
        ArcGeometry::new(
            Point::new(f32::NAN, 0.0),
            1.0,
            2.0,
            0.0,
            1.0,
            StrokeCap::Butt,
        ),
        ArcGeometry::new(Point::ZERO, f32::NAN, 2.0, 0.0, 1.0, StrokeCap::Butt),
        ArcGeometry::new(Point::ZERO, 1.0, f32::INFINITY, 0.0, 1.0, StrokeCap::Butt),
        ArcGeometry::new(Point::ZERO, 1.0, 2.0, f32::NAN, 1.0, StrokeCap::Butt),
        ArcGeometry::new(Point::ZERO, 1.0, 2.0, 0.0, f32::NAN, StrokeCap::Butt),
    ] {
        assert!(arc.is_degenerate());
        let bounds = arc.bounds();
        for value in [bounds.x, bounds.y, bounds.width, bounds.height] {
            assert!(value.is_finite(), "degenerate arc bounds must stay finite");
        }
    }
}

#[test]
fn approximate_bounds_contain_the_exact_box_within_documented_slack() {
    for radius in [2.0f32, 10.0, 57.0, 204.0] {
        for cap in [StrokeCap::Butt, StrokeCap::Round, StrokeCap::Square] {
            for step in 0..48 {
                let start = step as f32 * (TAU / 48.0) * 1.031;
                for sweep in [0.05f32, 0.9, FRAC_PI_2, 3.6] {
                    let arc = ArcGeometry::new(
                        Point::new(11.0, -7.0),
                        radius * 0.55,
                        radius,
                        start,
                        sweep,
                        cap,
                    );
                    if arc.is_degenerate() {
                        continue;
                    }
                    let bounds = arc.bounds();
                    let exact = exact_bounds(&arc);
                    let slack =
                        (arc.outer_radius + arc.half_thickness()) * FAST_TRIG_ERR * 2.0 + 0.05;
                    assert!(
                        bounds.x <= exact.x + 1e-3
                            && bounds.y <= exact.y + 1e-3
                            && bounds.x + bounds.width >= exact.x + exact.width - 1e-3
                            && bounds.y + bounds.height >= exact.y + exact.height - 1e-3,
                        "approximate box lost containment: {bounds:?} vs exact {exact:?} \
                         (radius {radius}, start {start}, sweep {sweep}, cap {cap:?})"
                    );
                    assert!(
                        (bounds.x - exact.x).abs() <= slack
                            && (bounds.y - exact.y).abs() <= slack
                            && (bounds.width - exact.width).abs() <= 2.0 * slack
                            && (bounds.height - exact.height).abs() <= 2.0 * slack,
                        "approximate box drifted past its slack: {bounds:?} vs exact \
                         {exact:?} slack {slack} (radius {radius}, start {start}, sweep \
                         {sweep}, cap {cap:?})"
                    );
                }
            }
        }
    }
}

fn exact_bounds(arc: &ArcGeometry) -> Rect {
    let mut min_x = f32::INFINITY;
    let mut min_y = f32::INFINITY;
    let mut max_x = f32::NEG_INFINITY;
    let mut max_y = f32::NEG_INFINITY;
    let mut include = |x: f32, y: f32| {
        min_x = min_x.min(x);
        min_y = min_y.min(y);
        max_x = max_x.max(x);
        max_y = max_y.max(y);
    };
    let rb = arc.half_thickness();
    let ra = arc.mid_radius();
    let end_angle = arc.start_angle + arc.sweep_angle;
    for (angle, outward) in [(arc.start_angle, -1.0f32), (end_angle, 1.0f32)] {
        let (sin, cos) = angle.sin_cos();
        match arc.cap {
            StrokeCap::Butt => {
                include(
                    arc.center.x + cos * arc.inner_radius,
                    arc.center.y + sin * arc.inner_radius,
                );
                include(
                    arc.center.x + cos * arc.outer_radius,
                    arc.center.y + sin * arc.outer_radius,
                );
            }
            StrokeCap::Square => {
                let tx = -sin * rb * outward;
                let ty = cos * rb * outward;
                include(
                    arc.center.x + cos * arc.inner_radius + tx,
                    arc.center.y + sin * arc.inner_radius + ty,
                );
                include(
                    arc.center.x + cos * arc.outer_radius + tx,
                    arc.center.y + sin * arc.outer_radius + ty,
                );
            }
            StrokeCap::Round => {
                let cx = arc.center.x + cos * ra;
                let cy = arc.center.y + sin * ra;
                include(cx - rb, cy - rb);
                include(cx + rb, cy + rb);
            }
        }
    }
    const AXIS_DIRECTIONS: [(f32, f32); 4] = [(0.0, 1.0), (1.0, 0.0), (0.0, -1.0), (-1.0, 0.0)];
    for (quadrant, (sin, cos)) in AXIS_DIRECTIONS.into_iter().enumerate() {
        let angle = quadrant as f32 * FRAC_PI_2;
        if arc.contains_angle(angle) {
            include(
                arc.center.x + cos * arc.outer_radius,
                arc.center.y + sin * arc.outer_radius,
            );
        }
    }
    Rect {
        x: min_x,
        y: min_y,
        width: (max_x - min_x).max(0.0),
        height: (max_y - min_y).max(0.0),
    }
}

#[test]
fn arc_geometry_flags_degenerate_bands() {
    assert!(ArcGeometry::new(Point::ZERO, 5.0, 5.0, 0.0, 1.0, StrokeCap::Butt).is_degenerate());
    assert!(ArcGeometry::new(Point::ZERO, 9.0, 5.0, 0.0, 1.0, StrokeCap::Butt).is_degenerate());
    assert!(ArcGeometry::new(Point::ZERO, 1.0, 5.0, 0.0, 0.0, StrokeCap::Butt).is_degenerate());
    assert!(ArcGeometry::new(Point::ZERO, 0.0, 0.0, 0.0, 1.0, StrokeCap::Butt).is_degenerate());
}

#[test]
fn arc_bounds_quarter_sweep_hugs_the_quadrant() {
    let arc = ArcGeometry::new(
        Point::new(100.0, 100.0),
        0.0,
        10.0,
        0.0,
        FRAC_PI_2,
        StrokeCap::Butt,
    );
    let bounds = arc.bounds();
    assert!(approx(bounds.x, 100.0), "{bounds:?}");
    assert!(approx(bounds.y, 100.0), "{bounds:?}");
    assert!(approx(bounds.width, 10.0), "{bounds:?}");
    assert!(approx(bounds.height, 10.0), "{bounds:?}");
}

#[test]
fn arc_bounds_three_quarter_sweep_spans_every_axis_it_crosses() {
    let arc = ArcGeometry::new(
        Point::new(0.0, 0.0),
        0.0,
        10.0,
        0.0,
        3.0 * FRAC_PI_2,
        StrokeCap::Butt,
    );
    let bounds = arc.bounds();
    assert!(approx(bounds.x, -10.0), "{bounds:?}");
    assert!(approx(bounds.y, -10.0), "{bounds:?}");
    assert!(approx(bounds.width, 20.0), "{bounds:?}");
    assert!(approx(bounds.height, 20.0), "{bounds:?}");
}

#[test]
fn arc_bounds_include_inner_endpoints_when_no_axis_is_crossed() {
    let arc = ArcGeometry::new(
        Point::ZERO,
        8.0,
        10.0,
        std::f32::consts::FRAC_PI_4,
        FRAC_PI_2,
        StrokeCap::Butt,
    );
    let bounds = arc.bounds();
    let sqrt2_2 = std::f32::consts::FRAC_1_SQRT_2;
    assert!(approx(bounds.y, 8.0 * sqrt2_2), "{bounds:?}");
    assert!(approx(bounds.y + bounds.height, 10.0), "{bounds:?}");
    assert!(approx(bounds.x, -10.0 * sqrt2_2), "{bounds:?}");
    assert!(approx(bounds.width, 20.0 * sqrt2_2), "{bounds:?}");
}

#[test]
fn arc_bounds_negative_sweep_matches_equivalent_positive_sweep() {
    let forward = ArcGeometry::new(Point::ZERO, 4.0, 6.0, 0.0, FRAC_PI_2, StrokeCap::Butt);
    let backward = ArcGeometry::new(
        Point::ZERO,
        4.0,
        6.0,
        FRAC_PI_2,
        -FRAC_PI_2,
        StrokeCap::Butt,
    );
    assert_eq!(forward.bounds(), backward.bounds());
}

#[test]
fn arc_bounds_full_turn_is_the_outer_circle() {
    let arc = ArcGeometry::new(Point::new(5.0, 7.0), 3.0, 9.0, 1.1, TAU, StrokeCap::Butt);
    let bounds = arc.bounds();
    assert!(approx(bounds.x, -4.0), "{bounds:?}");
    assert!(approx(bounds.y, -2.0), "{bounds:?}");
    assert!(approx(bounds.width, 18.0), "{bounds:?}");
    assert!(approx(bounds.height, 18.0), "{bounds:?}");
}

#[test]
fn arc_bounds_round_caps_bulge_past_the_radial_ends() {
    let butt = ArcGeometry::new(Point::ZERO, 8.0, 12.0, 0.0, FRAC_PI_2, StrokeCap::Butt);
    let round = ArcGeometry::new(Point::ZERO, 8.0, 12.0, 0.0, FRAC_PI_2, StrokeCap::Round);
    let butt_bounds = butt.bounds();
    let round_bounds = round.bounds();
    assert!(approx(butt_bounds.y, 0.0), "{butt_bounds:?}");
    assert!(approx(round_bounds.y, -2.0), "{round_bounds:?}");
    assert!(round_bounds.width >= butt_bounds.width);
    assert!(round_bounds.height >= butt_bounds.height);
}

#[test]
fn arc_bounds_square_caps_project_along_the_tangent() {
    let square = ArcGeometry::new(Point::ZERO, 8.0, 12.0, 0.0, FRAC_PI_2, StrokeCap::Square);
    let bounds = square.bounds();
    assert!(approx(bounds.y, -2.0), "{bounds:?}");
    assert!(approx(bounds.x + bounds.width, 12.0), "{bounds:?}");
}

#[test]
fn arc_band_resolves_stroked_and_filled_forms() {
    let (inner, outer, cap) =
        arc_band(10.0, 0.0, Some(Stroke::new(4.0).with_cap(StrokeCap::Round)));
    assert_eq!((inner, outer), (8.0, 12.0));
    assert_eq!(cap, StrokeCap::Round);

    let (inner, outer, cap) = arc_band(10.0, 6.0, None);
    assert_eq!((inner, outer), (6.0, 10.0));
    assert_eq!(cap, StrokeCap::Butt);

    let (inner, outer, _) = arc_band(10.0, 40.0, None);
    assert_eq!((inner, outer), (10.0, 10.0));

    let (inner, outer, _) = arc_band(1.0, 0.0, Some(Stroke::new(10.0)));
    assert_eq!((inner, outer), (0.0, 6.0));
}

#[test]
fn full_ring_bounds_shortcut_matches_the_endpoint_walk() {
    let ring = ArcGeometry::new(Point::new(10.0, -4.0), 6.0, 9.0, 1.3, TAU, StrokeCap::Butt);
    assert_eq!(
        ring.bounds(),
        Rect {
            x: 1.0,
            y: -13.0,
            width: 18.0,
            height: 18.0
        }
    );

    let square = ArcGeometry {
        cap: StrokeCap::Square,
        start_angle: TAU - (1.5f32 / 9.0).atan(),
        ..ring
    };
    let bounds = square.bounds();
    assert!(bounds.x + bounds.width > square.center.x + square.outer_radius);
}

#[test]
fn inflate_rect_ignores_non_positive_amounts() {
    let rect = Rect {
        x: 1.0,
        y: 2.0,
        width: 3.0,
        height: 4.0,
    };
    assert_eq!(inflate_rect(rect, 0.0), rect);
    assert_eq!(inflate_rect(rect, -1.0), rect);
    assert_eq!(inflate_rect(rect, f32::NAN), rect);
    assert_eq!(
        inflate_rect(rect, 1.0),
        Rect {
            x: 0.0,
            y: 1.0,
            width: 5.0,
            height: 6.0
        }
    );
}
