use std::f32::consts::{FRAC_PI_2, PI};

use cranpose_ui_graphics::TAU;

use super::*;

fn arc(inner: f32, outer: f32, start: f32, sweep: f32, cap: StrokeCap) -> ArcGeometry {
    ArcGeometry::new(Point::ZERO, inner, outer, start, sweep, cap)
}

#[test]
fn rounded_rect_sdf_matches_known_distances() {
    let d_center = sdf_rounded_rect(Point::ZERO, (10.0, 10.0), [0.0; 4]);
    assert!((d_center + 10.0).abs() < 1e-4, "{d_center}");
    let d_outside = sdf_rounded_rect(Point::new(15.0, 0.0), (10.0, 10.0), [0.0; 4]);
    assert!((d_outside - 5.0).abs() < 1e-4, "{d_outside}");
}

#[test]
fn stroked_rect_covers_only_the_band_around_the_edge() {
    let half = (12.0, 12.0);
    let on_edge = sdf_stroked_rounded_rect(
        Point::new(10.0, 0.0),
        half,
        [0.0; 4],
        2.0,
        StrokeJoin::Miter,
    );
    assert!(on_edge < 0.0, "the edge itself must be inside the stroke");
    let inside =
        sdf_stroked_rounded_rect(Point::new(4.0, 0.0), half, [0.0; 4], 2.0, StrokeJoin::Miter);
    assert!(inside > 0.0, "the interior must be empty for a stroke");
    let outside = sdf_stroked_rounded_rect(
        Point::new(16.0, 0.0),
        half,
        [0.0; 4],
        2.0,
        StrokeJoin::Miter,
    );
    assert!(outside > 0.0, "well outside must be empty");
}

#[test]
fn miter_join_keeps_a_square_corner_round_join_does_not() {
    let corner = Point::new(11.9, 11.9);
    let miter = sdf_stroked_rounded_rect(corner, (12.0, 12.0), [0.0; 4], 2.0, StrokeJoin::Miter);
    let round = sdf_stroked_rounded_rect(corner, (12.0, 12.0), [0.0; 4], 2.0, StrokeJoin::Round);
    let bevel = sdf_stroked_rounded_rect(corner, (12.0, 12.0), [0.0; 4], 2.0, StrokeJoin::Bevel);
    assert!(miter < 0.0, "miter fills the corner point: {miter}");
    assert!(round > 0.0, "round cuts the corner off: {round}");
    assert!(bevel > 0.0, "bevel cuts the corner off: {bevel}");
    assert!(
        bevel > round,
        "the bevel chord must cut deeper than the round arc: \
         bevel={bevel} round={round}"
    );
}

#[test]
fn full_ring_has_no_seam_at_the_wrap_point() {
    let ring = arc(8.0, 12.0, 0.0, TAU, StrokeCap::Butt);
    for step in 0..64 {
        let angle = step as f32 / 64.0 * TAU;
        let (sin, cos) = angle.sin_cos();
        let p = Point::new(cos * 10.0, sin * 10.0);
        let d = sdf_arc_band(p, &ring);
        assert!(
            d < 0.0,
            "the ring centerline must be covered at angle {angle}: d={d}"
        );
    }
}

#[test]
fn butt_caps_cut_the_band_at_the_radial_ends() {
    let band = arc(8.0, 12.0, 0.0, FRAC_PI_2, StrokeCap::Butt);
    let inside = Point::new(10.0 * INV_SQRT2, 10.0 * INV_SQRT2);
    assert!(sdf_arc_band(inside, &band) < 0.0);
    let past_end = Point::new(-1.0, 10.0);
    assert!(
        sdf_arc_band(past_end, &band) > 0.0,
        "butt cap must not bulge past the radial end"
    );
    let before_start = Point::new(10.0, -1.0);
    assert!(sdf_arc_band(before_start, &band) > 0.0);
}

#[test]
fn round_caps_bulge_past_the_radial_ends_and_square_caps_project() {
    let round = arc(8.0, 12.0, 0.0, FRAC_PI_2, StrokeCap::Round);
    let square = arc(8.0, 12.0, 0.0, FRAC_PI_2, StrokeCap::Square);
    let before_start = Point::new(10.0, -1.0);
    assert!(
        sdf_arc_band(before_start, &round) < 0.0,
        "round cap must cover the semicircle past the end"
    );
    assert!(
        sdf_arc_band(before_start, &square) < 0.0,
        "square cap must cover the projection past the end"
    );
    let far = Point::new(10.0, -3.0);
    assert!(sdf_arc_band(far, &round) > 0.0);
    assert!(sdf_arc_band(far, &square) > 0.0);
}

#[test]
fn annular_sector_has_flat_radial_edges() {
    let sector = arc(6.0, 12.0, 0.0, PI, StrokeCap::Butt);
    for radius in [6.5, 8.0, 10.0, 11.5] {
        let p = Point::new(radius * (0.01f32).cos(), radius * (0.01f32).sin());
        assert!(
            sdf_arc_band(p, &sector) < 0.0,
            "radius {radius} just inside the sweep must be covered"
        );
        let q = Point::new(radius * (-0.2f32).cos(), radius * (-0.2f32).sin());
        assert!(
            sdf_arc_band(q, &sector) > 0.0,
            "radius {radius} just outside the sweep must be empty"
        );
    }
}

#[test]
fn wedge_with_zero_inner_radius_reaches_the_center() {
    let wedge = arc(0.0, 10.0, 0.0, FRAC_PI_2, StrokeCap::Butt);
    assert!(sdf_arc_band(Point::new(0.5, 0.5), &wedge) < 0.0);
    assert!(sdf_arc_band(Point::new(-0.5, -0.5), &wedge) > 0.0);
}

#[test]
fn degenerate_arcs_never_produce_nan_coverage() {
    for geometry in [
        arc(0.0, 0.0, 0.0, 0.0, StrokeCap::Butt),
        arc(5.0, 5.0, 0.0, 1.0, StrokeCap::Round),
        arc(0.0, 10.0, 0.0, 0.0, StrokeCap::Square),
        ArcGeometry::new(Point::ZERO, f32::NAN, 1.0, 0.0, 1.0, StrokeCap::Butt),
    ] {
        for p in [Point::ZERO, Point::new(3.0, -4.0), Point::new(-9.0, 9.0)] {
            let value = arc_coverage(p, &geometry);
            assert!(value.is_finite(), "coverage must stay finite: {value}");
            assert!((0.0..=1.0).contains(&value), "{value}");
        }
    }
}

#[test]
fn coverage_saturates_and_antialiases() {
    assert_eq!(coverage_for_distance(-5.0), 1.0);
    assert_eq!(coverage_for_distance(5.0), 0.0);
    assert!((coverage_for_distance(0.0) - 0.5).abs() < 1e-5);
    assert_eq!(coverage_for_distance(f32::NAN), 0.0);
}

#[test]
fn stroked_rect_coverage_uses_the_inflated_bounds() {
    let bounds = Rect {
        x: 8.0,
        y: 8.0,
        width: 24.0,
        height: 24.0,
    };
    let on_edge =
        stroked_rect_coverage(Point::new(10.0, 20.0), bounds, None, 2.0, StrokeJoin::Miter);
    assert!(on_edge > 0.9, "the stroked edge must be opaque: {on_edge}");
    let interior =
        stroked_rect_coverage(Point::new(20.0, 20.0), bounds, None, 2.0, StrokeJoin::Miter);
    assert_eq!(interior, 0.0, "a stroke must not fill its interior");
}
