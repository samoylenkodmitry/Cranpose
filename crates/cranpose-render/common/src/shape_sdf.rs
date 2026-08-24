//! CPU mirror of the signed-distance functions in `shaders/shape.wgsl`.
//!
//! The software (pixels) renderer rasterizes the *same* shapes as the GPU. Both
//! evaluate these formulas, so a stroked rect or an arc looks the same on either
//! backend instead of the CPU path quietly degrading to a filled box.
//!
//! Angle convention (shared with `cranpose_ui_graphics::stroke`): radians, `0`
//! along +X, increasing clockwise on screen (y-down device space).

use cranpose_ui_graphics::{ArcGeometry, CornerRadii, Point, Rect, StrokeCap, StrokeJoin};

const INV_SQRT2: f32 = std::f32::consts::FRAC_1_SQRT_2;

/// Signed distance to a rounded box centered at the origin.
///
/// `radii` is ordered exactly like the WGSL `vec4`: top-left, top-right,
/// bottom-left, bottom-right.
pub fn sdf_rounded_rect(p: Point, half_size: (f32, f32), radii: [f32; 4]) -> f32 {
    let radius = match (p.x > 0.0, p.y > 0.0) {
        (false, false) => radii[0],
        (true, false) => radii[1],
        (false, true) => radii[2],
        (true, true) => radii[3],
    };
    let qx = p.x.abs() - half_size.0 + radius;
    let qy = p.y.abs() - half_size.1 + radius;
    let inside = qx.max(qy).min(0.0);
    let outside = (qx.max(0.0).powi(2) + qy.max(0.0).powi(2)).sqrt();
    inside + outside - radius
}

/// Signed distance to the outline of a rounded box stroked with a centered
/// stroke of width `2 * half_width`.
///
/// `half_size` is the **inflated** box (geometry plus `half_width` on every
/// side), matching what the renderer hands the shader.
pub fn sdf_stroked_rounded_rect(
    p: Point,
    half_size: (f32, f32),
    radii: [f32; 4],
    half_width: f32,
    join: StrokeJoin,
) -> f32 {
    let hw = half_width.max(0.0);
    let geom = ((half_size.0 - hw).max(0.0), (half_size.1 - hw).max(0.0));

    let mut outer_radii = [radii[0] + hw, radii[1] + hw, radii[2] + hw, radii[3] + hw];
    if join != StrokeJoin::Round {
        // Miter/bevel keep a square corner square; an already-rounded corner
        // has no join and keeps the true parallel offset.
        for (out, r) in outer_radii.iter_mut().zip(radii.iter()) {
            if *r < 0.0001 {
                *out = 0.0;
            }
        }
    }
    let inner_radii = [
        (radii[0] - hw).max(0.0),
        (radii[1] - hw).max(0.0),
        (radii[2] - hw).max(0.0),
        (radii[3] - hw).max(0.0),
    ];

    let outer = sdf_rounded_rect(p, (geom.0 + hw, geom.1 + hw), outer_radii);
    let inner = sdf_rounded_rect(
        p,
        ((geom.0 - hw).max(0.0), (geom.1 - hw).max(0.0)),
        inner_radii,
    );
    let mut dist = outer.max(-inner);

    if join == StrokeJoin::Bevel {
        let chamfer = (p.x.abs() + p.y.abs() - (geom.0 + geom.1 + hw)) * INV_SQRT2;
        dist = dist.max(chamfer);
    }
    dist
}

/// Signed distance to a circular band limited to an angular sweep — the shape
/// behind both stroked arcs and filled annular sectors.
pub fn sdf_arc_band(p: Point, arc: &ArcGeometry) -> f32 {
    let ra = arc.mid_radius();
    let rb = arc.half_thickness().max(0.0);
    let sweep = arc.sweep_angle.clamp(0.0, cranpose_ui_graphics::TAU);
    let half_sweep = sweep * 0.5;
    let mid = arc.start_angle + half_sweep;

    let (sm, cm) = mid.sin_cos();
    let dx = p.x - arc.center.x;
    let dy = p.y - arc.center.y;
    // Rotate into the frame the analytic arc SDF expects: band straddling +Y.
    let qx = (-sm * dx + cm * dy).abs();
    let qy = cm * dx + sm * dy;

    // sin() of a half sweep in [0, PI] is non-negative in exact math; the max()
    // pins a full turn to exactly (0, -1) so a closed ring has no seam.
    let sc = (half_sweep.sin().max(0.0), half_sweep.cos());

    let mut dist = if sc.1 * qx > sc.0 * qy {
        ((qx - sc.0 * ra).powi(2) + (qy - sc.1 * ra).powi(2)).sqrt() - rb
    } else {
        ((qx * qx + qy * qy).sqrt() - ra).abs() - rb
    };

    // Distance to the radial boundary plane, positive outside the wedge.
    let plane = sc.1 * qx - sc.0 * qy;
    match arc.cap {
        StrokeCap::Butt => dist = dist.max(plane),
        StrokeCap::Square => dist = dist.max(plane - rb),
        StrokeCap::Round => {}
    }
    dist
}

/// Antialiased coverage for a signed distance, matching the shader's
/// `1.0 - smoothstep(-0.5, 0.5, d)`.
pub fn coverage_for_distance(distance: f32) -> f32 {
    if !distance.is_finite() {
        return 0.0;
    }
    let t = ((distance + 0.5).clamp(0.0, 1.0)) as f64;
    let smooth = t * t * (3.0 - 2.0 * t);
    (1.0 - smooth) as f32
}

/// Coverage of `point` by a stroked rect/round-rect whose (already inflated)
/// bounds are `rect`.
pub fn stroked_rect_coverage(
    point: Point,
    rect: Rect,
    radii: Option<CornerRadii>,
    half_width: f32,
    join: StrokeJoin,
) -> f32 {
    let half_size = (rect.width * 0.5, rect.height * 0.5);
    let local = Point::new(
        point.x - (rect.x + half_size.0),
        point.y - (rect.y + half_size.1),
    );
    let radii = radii.unwrap_or_default();
    let distance = sdf_stroked_rounded_rect(
        local,
        half_size,
        [
            radii.top_left,
            radii.top_right,
            radii.bottom_left,
            radii.bottom_right,
        ],
        half_width,
        join,
    );
    coverage_for_distance(distance)
}

/// Coverage of `point` by an arc band.
pub fn arc_coverage(point: Point, arc: &ArcGeometry) -> f32 {
    coverage_for_distance(sdf_arc_band(point, arc))
}

#[cfg(test)]
mod tests {
    use std::f32::consts::{FRAC_PI_2, PI};

    use cranpose_ui_graphics::TAU;

    use super::*;

    fn arc(inner: f32, outer: f32, start: f32, sweep: f32, cap: StrokeCap) -> ArcGeometry {
        ArcGeometry::new(Point::ZERO, inner, outer, start, sweep, cap)
    }

    #[test]
    fn rounded_rect_sdf_matches_known_distances() {
        // A 20x20 box, no radii: the center is 10 inside, a point 5 to the
        // right of the right edge is 5 outside.
        let d_center = sdf_rounded_rect(Point::ZERO, (10.0, 10.0), [0.0; 4]);
        assert!((d_center + 10.0).abs() < 1e-4, "{d_center}");
        let d_outside = sdf_rounded_rect(Point::new(15.0, 0.0), (10.0, 10.0), [0.0; 4]);
        assert!((d_outside - 5.0).abs() < 1e-4, "{d_outside}");
    }

    #[test]
    fn stroked_rect_covers_only_the_band_around_the_edge() {
        // Geometry is 20x20 (half 10), stroke width 4 => inflated half 12.
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
        // Geometry 20x20 (half 10), width 4 (hw 2) => inflated half 12.
        // The outer miter corner is exactly (12, 12).
        let corner = Point::new(11.9, 11.9);
        let miter =
            sdf_stroked_rounded_rect(corner, (12.0, 12.0), [0.0; 4], 2.0, StrokeJoin::Miter);
        let round =
            sdf_stroked_rounded_rect(corner, (12.0, 12.0), [0.0; 4], 2.0, StrokeJoin::Round);
        let bevel =
            sdf_stroked_rounded_rect(corner, (12.0, 12.0), [0.0; 4], 2.0, StrokeJoin::Bevel);
        assert!(miter < 0.0, "miter fills the corner point: {miter}");
        assert!(round > 0.0, "round cuts the corner off: {round}");
        assert!(bevel > 0.0, "bevel cuts the corner off: {bevel}");
        // The bevel is the chord between the two arc endpoints, so along the
        // diagonal it sits *inside* the round join's arc and cuts more.
        assert!(
            bevel > round,
            "the bevel chord must cut deeper than the round arc: \
             bevel={bevel} round={round}"
        );
    }

    #[test]
    fn full_ring_has_no_seam_at_the_wrap_point() {
        let ring = arc(8.0, 12.0, 0.0, TAU, StrokeCap::Butt);
        // Sample all the way round, including exactly at the wrap angle.
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
        // 0 -> 90 degrees, band 8..12.
        let band = arc(8.0, 12.0, 0.0, FRAC_PI_2, StrokeCap::Butt);
        // Just inside the sweep at 45 degrees, on the centerline.
        let inside = Point::new(10.0 * INV_SQRT2, 10.0 * INV_SQRT2);
        assert!(sdf_arc_band(inside, &band) < 0.0);
        // Just past the end cap (angle slightly > 90 degrees) must be empty.
        let past_end = Point::new(-1.0, 10.0);
        assert!(
            sdf_arc_band(past_end, &band) > 0.0,
            "butt cap must not bulge past the radial end"
        );
        // Just before the start cap likewise.
        let before_start = Point::new(10.0, -1.0);
        assert!(sdf_arc_band(before_start, &band) > 0.0);
    }

    #[test]
    fn round_caps_bulge_past_the_radial_ends_and_square_caps_project() {
        let round = arc(8.0, 12.0, 0.0, FRAC_PI_2, StrokeCap::Round);
        let square = arc(8.0, 12.0, 0.0, FRAC_PI_2, StrokeCap::Square);
        // 1 unit before the start angle, on the centerline (radius 10).
        let before_start = Point::new(10.0, -1.0);
        assert!(
            sdf_arc_band(before_start, &round) < 0.0,
            "round cap must cover the semicircle past the end"
        );
        assert!(
            sdf_arc_band(before_start, &square) < 0.0,
            "square cap must cover the projection past the end"
        );
        // 3 units before the start is past both caps (rb = 2).
        let far = Point::new(10.0, -3.0);
        assert!(sdf_arc_band(far, &round) > 0.0);
        assert!(sdf_arc_band(far, &square) > 0.0);
    }

    #[test]
    fn annular_sector_has_flat_radial_edges() {
        // The defining property: at the start angle the boundary is a straight
        // radial line, so points at the same angle but different radii are all
        // exactly on the edge.
        let sector = arc(6.0, 12.0, 0.0, PI, StrokeCap::Butt);
        for radius in [6.5, 8.0, 10.0, 11.5] {
            // Just inside the sweep.
            let p = Point::new(radius * (0.01f32).cos(), radius * (0.01f32).sin());
            assert!(
                sdf_arc_band(p, &sector) < 0.0,
                "radius {radius} just inside the sweep must be covered"
            );
            // Just outside the sweep (negative angle).
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
        // Geometry (10,10)-(30,30) stroked at width 4 => bounds (8,8)-(32,32).
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
}
