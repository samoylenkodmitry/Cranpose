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
    let reach = RoundedReach::at(p, half_size, radii);
    reach.inside + reach.outside_squared.sqrt() - reach.radius
}

// The parts of `sdf_rounded_rect`'s distance at a point: the distance inside
// the box, the square of the distance outside it and the radius of the
// corner whose quarter holds the point.
struct RoundedReach {
    inside: f32,
    outside_squared: f32,
    radius: f32,
}

impl RoundedReach {
    fn at(p: Point, half_size: (f32, f32), radii: [f32; 4]) -> Self {
        let radius = match (p.x > 0.0, p.y > 0.0) {
            (false, false) => radii[0],
            (true, false) => radii[1],
            (false, true) => radii[2],
            (true, true) => radii[3],
        };
        let qx = p.x.abs() - half_size.0 + radius;
        let qy = p.y.abs() - half_size.1 + radius;
        Self {
            inside: qx.max(qy).min(0.0),
            outside_squared: qx.max(0.0).powi(2) + qy.max(0.0).powi(2),
            radius,
        }
    }
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
    StrokedBox::new(Point::ZERO, half_size, radii, half_width, join).distance(p)
}

/// The band a stroke covers around a rounded box, with the sizes and radii
/// of its two edges worked out once, for the distances of many points.
pub struct StrokedBox {
    center: Point,
    outer: ((f32, f32), [f32; 4]),
    inner: ((f32, f32), [f32; 4]),
    // Where a bevel join cuts each corner, along |x| + |y|.
    bevel: Option<f32>,
}

impl StrokedBox {
    /// The band [`sdf_stroked_rounded_rect`] measures, around a box centred
    /// on `center`.
    pub fn new(
        center: Point,
        half_size: (f32, f32),
        radii: [f32; 4],
        half_width: f32,
        join: StrokeJoin,
    ) -> Self {
        let hw = half_width.max(0.0);
        let geom = ((half_size.0 - hw).max(0.0), (half_size.1 - hw).max(0.0));

        let mut outer_radii = [radii[0] + hw, radii[1] + hw, radii[2] + hw, radii[3] + hw];
        if join != StrokeJoin::Round {
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
        Self {
            center,
            outer: ((geom.0 + hw, geom.1 + hw), outer_radii),
            inner: (
                ((geom.0 - hw).max(0.0), (geom.1 - hw).max(0.0)),
                inner_radii,
            ),
            bevel: (join == StrokeJoin::Bevel).then_some(geom.0 + geom.1 + hw),
        }
    }

    /// The band of the stroke [`stroked_rect_coverage`] covers.
    pub fn of_rect(
        rect: Rect,
        radii: Option<CornerRadii>,
        half_width: f32,
        join: StrokeJoin,
    ) -> Self {
        let half_size = (rect.width * 0.5, rect.height * 0.5);
        let radii = radii.unwrap_or_default();
        Self::new(
            Point::new(rect.x + half_size.0, rect.y + half_size.1),
            half_size,
            [
                radii.top_left,
                radii.top_right,
                radii.bottom_left,
                radii.bottom_right,
            ],
            half_width,
            join,
        )
    }

    /// The signed distance from `point` to the edge of the band.
    pub fn distance(&self, point: Point) -> f32 {
        let p = Point::new(point.x - self.center.x, point.y - self.center.y);
        let outer = RoundedReach::at(p, self.outer.0, self.outer.1);
        let inner = RoundedReach::at(p, self.inner.0, self.inner.1);
        let outer_outside = outer.outside_squared.sqrt();
        // Unless a corner's radius is under the band's half width, its two
        // edges' circles share a centre, and the point lies as far outside
        // both boxes: one root serves both. Equal bits give an equal root.
        let inner_outside = if inner.outside_squared.to_bits() == outer.outside_squared.to_bits() {
            outer_outside
        } else {
            inner.outside_squared.sqrt()
        };
        let mut dist = (outer.inside + outer_outside - outer.radius)
            .max(-(inner.inside + inner_outside - inner.radius));
        if let Some(cut) = self.bevel {
            let chamfer = (p.x.abs() + p.y.abs() - cut) * INV_SQRT2;
            dist = dist.max(chamfer);
        }
        dist
    }
}

/// Signed distance to a circular band limited to an angular sweep — the shape
/// behind both stroked arcs and filled annular sectors.
pub fn sdf_arc_band(p: Point, arc: &ArcGeometry) -> f32 {
    ArcBand::new(arc).distance(p)
}

/// An arc's band with the sines and cosines of its sweep worked out once,
/// for the distances of many points.
pub struct ArcBand {
    center: Point,
    mid_radius: f32,
    half_thickness: f32,
    // The sine and cosine of the angle halfway along the sweep.
    mid: (f32, f32),
    // The sine, not below zero, and the cosine of half the sweep.
    half_sweep: (f32, f32),
    cap: StrokeCap,
}

impl ArcBand {
    /// The band of `arc`.
    pub fn new(arc: &ArcGeometry) -> Self {
        let sweep = arc.sweep_angle.clamp(0.0, cranpose_ui_graphics::TAU);
        let half_sweep = sweep * 0.5;
        let mid = arc.start_angle + half_sweep;
        Self {
            center: arc.center,
            mid_radius: arc.mid_radius(),
            half_thickness: arc.half_thickness().max(0.0),
            mid: mid.sin_cos(),
            half_sweep: (half_sweep.sin().max(0.0), half_sweep.cos()),
            cap: arc.cap,
        }
    }

    /// [`sdf_arc_band`] at `p`.
    pub fn distance(&self, p: Point) -> f32 {
        let ra = self.mid_radius;
        let rb = self.half_thickness;
        let (sm, cm) = self.mid;
        let dx = p.x - self.center.x;
        let dy = p.y - self.center.y;
        let qx = (-sm * dx + cm * dy).abs();
        let qy = cm * dx + sm * dy;

        let sc = self.half_sweep;

        let mut dist = if sc.1 * qx > sc.0 * qy {
            ((qx - sc.0 * ra).powi(2) + (qy - sc.1 * ra).powi(2)).sqrt() - rb
        } else {
            ((qx * qx + qy * qy).sqrt() - ra).abs() - rb
        };

        let plane = sc.1 * qx - sc.0 * qy;
        match self.cap {
            StrokeCap::Butt => dist = dist.max(plane),
            StrokeCap::Square => dist = dist.max(plane - rb),
            StrokeCap::Round => {}
        }
        dist
    }

    /// The antialiased coverage of `p`, as [`arc_coverage`] gives it.
    pub fn coverage(&self, p: Point) -> f32 {
        coverage_for_distance(self.distance(p))
    }
}

/// Antialiased coverage for a signed distance, matching the shader's
/// `1.0 - smoothstep(-0.5, 0.5, d)`.
pub fn coverage_for_distance(distance: f32) -> f32 {
    if !distance.is_finite() {
        return 0.0;
    }
    let t = (distance + 0.5).clamp(0.0, 1.0);
    // Inside and outside the edge's pixel the smoothstep is exactly 0 or 1:
    // only the edge needs the f64 steps, which a target without a double
    // precision unit runs in software.
    if t <= 0.0 {
        return 1.0;
    }
    if t >= 1.0 {
        return 0.0;
    }
    let t = f64::from(t);
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
    coverage_for_distance(StrokedBox::of_rect(rect, radii, half_width, join).distance(point))
}

/// Coverage of `point` by an arc band.
pub fn arc_coverage(point: Point, arc: &ArcGeometry) -> f32 {
    ArcBand::new(arc).coverage(point)
}

#[cfg(test)]
#[path = "tests/shape_sdf_tests.rs"]
mod tests;
