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
    coverage_for_distance(stroked_rect_distance(point, rect, radii, half_width, join))
}

/// The signed distance from `point` to the edge of the stroke
/// [`stroked_rect_coverage`] covers.
pub fn stroked_rect_distance(
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
    sdf_stroked_rounded_rect(
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
    )
}

/// Coverage of `point` by an arc band.
pub fn arc_coverage(point: Point, arc: &ArcGeometry) -> f32 {
    ArcBand::new(arc).coverage(point)
}

#[cfg(test)]
#[path = "tests/shape_sdf_tests.rs"]
mod tests;
