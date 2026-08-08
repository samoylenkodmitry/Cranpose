//! Stroke styling and analytic arc geometry.
//!
//! # Angle convention
//!
//! Every angle in this module is expressed in **radians**, with `0` pointing
//! along the **+X axis** and increasing angles sweeping **clockwise on
//! screen**. Cranpose uses y-down device coordinates, so a point on the arc of
//! radius `r` at angle `θ` is
//!
//! ```text
//! (center.x + r * cos(θ), center.y + r * sin(θ))
//! ```
//!
//! which — because `y` grows downwards — visually rotates clockwise as `θ`
//! grows. This is exactly the convention already baked into the sweep-gradient
//! branch of `shape.wgsl`, which derives its parameter from `atan2(dy, dx)`.

use crate::{Point, Rect};

/// Shape of the two ends of an open stroked path (an arc, today).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum StrokeCap {
    /// Flat end exactly at the geometric end of the path.
    #[default]
    Butt,
    /// Semicircular end bulging half the stroke width past the path end.
    Round,
    /// Flat end projected half the stroke width past the path end.
    Square,
}

/// Shape produced where two stroked segments meet at a corner.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum StrokeJoin {
    /// Extend the outer edges until they meet in a sharp point.
    #[default]
    Miter,
    /// Fill the corner with a circular arc of half the stroke width.
    Round,
    /// Cut the corner off with a straight chamfer.
    Bevel,
}

/// Describes how an outline is stroked.
///
/// The stroke is *centered* on the geometry: it extends `width / 2` to either
/// side of the path, matching Skia / Jetpack Compose semantics.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Stroke {
    /// Total stroke width in the caller's coordinate space (dp for
    /// [`crate::DrawScope`] callers).
    pub width: f32,
    pub cap: StrokeCap,
    pub join: StrokeJoin,
}

impl Stroke {
    /// A `width`-wide stroke with butt caps and miter joins.
    pub const fn new(width: f32) -> Self {
        Self {
            width,
            cap: StrokeCap::Butt,
            join: StrokeJoin::Miter,
        }
    }

    pub const fn with_width(mut self, width: f32) -> Self {
        self.width = width;
        self
    }

    pub const fn with_cap(mut self, cap: StrokeCap) -> Self {
        self.cap = cap;
        self
    }

    pub const fn with_join(mut self, join: StrokeJoin) -> Self {
        self.join = join;
        self
    }

    /// Half the stroke width, clamped to a finite non-negative value.
    ///
    /// This is the amount the stroke bleeds outside (and inside) the geometry.
    pub fn half_width(&self) -> f32 {
        if self.width.is_finite() {
            (self.width * 0.5).max(0.0)
        } else {
            0.0
        }
    }

    /// A stroke is renderable only when it has a strictly positive, finite width.
    pub fn is_visible(&self) -> bool {
        self.width.is_finite() && self.width > 0.0
    }

    /// Scales the stroke width (used when a layer transform scales the shape).
    pub fn scaled(&self, scale: f32) -> Self {
        Self {
            width: self.width * scale,
            ..*self
        }
    }
}

impl Default for Stroke {
    fn default() -> Self {
        Self::new(1.0)
    }
}

/// Full turn in radians.
pub const TAU: f32 = std::f32::consts::PI * 2.0;

/// A resolved circular *band* between two radii, limited to an angular sweep.
///
/// Both a stroked arc and a filled annular sector lower to this single form:
///
/// * stroked arc — `inner = radius - width/2`, `outer = radius + width/2`,
///   ends shaped by the stroke's [`StrokeCap`];
/// * filled annular sector — `inner`/`outer` as given, always butt (flat
///   radial) ends.
///
/// Values are normalized on construction: `sweep_angle` is non-negative and at
/// most [`TAU`], `outer_radius >= inner_radius >= 0`, and non-finite inputs
/// collapse to a degenerate geometry (see [`ArcGeometry::is_degenerate`]).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ArcGeometry {
    pub center: Point,
    pub inner_radius: f32,
    pub outer_radius: f32,
    /// Normalized to `[0, TAU)`.
    pub start_angle: f32,
    /// Normalized to `[0, TAU]`.
    pub sweep_angle: f32,
    pub cap: StrokeCap,
}

/// `x mod TAU` into `[0, TAU)` without `rem_euclid`, whose `fmodf` lowers to
/// the software routine in compiler_builtins on aarch64 Android and shows up
/// in profiles at two calls per arc per frame. Multiply-floor keeps it to a
/// couple of instructions; the fixup folds the one-ulp overshoot cases back
/// into range.
#[inline]
fn wrap_angle_tau(x: f32) -> f32 {
    let wrapped = x - (x * (1.0 / TAU)).floor() * TAU;
    if wrapped >= TAU {
        wrapped - TAU
    } else if wrapped < 0.0 {
        0.0
    } else {
        wrapped
    }
}

impl ArcGeometry {
    /// Normalizing constructor. Never panics and never stores a NaN.
    pub fn new(
        center: Point,
        inner_radius: f32,
        outer_radius: f32,
        start_angle: f32,
        sweep_angle: f32,
        cap: StrokeCap,
    ) -> Self {
        let finite = center.x.is_finite()
            && center.y.is_finite()
            && inner_radius.is_finite()
            && outer_radius.is_finite()
            && start_angle.is_finite()
            && sweep_angle.is_finite();
        if !finite {
            return Self::DEGENERATE;
        }

        let outer = outer_radius.max(0.0);
        let inner = inner_radius.clamp(0.0, outer);

        // Fold negative sweeps into a positive sweep starting at the other end
        // so downstream math (and the shader) only ever sees `0 ..= TAU`.
        let (mut start, mut sweep) = if sweep_angle < 0.0 {
            (start_angle + sweep_angle, -sweep_angle)
        } else {
            (start_angle, sweep_angle)
        };
        if sweep >= TAU {
            // A closed ring: caps can never be seen, and forcing `Round` keeps
            // the shader from clipping a hairline seam at the wrap point.
            sweep = TAU;
            start = 0.0;
        }
        start = wrap_angle_tau(start);
        if !start.is_finite() {
            start = 0.0;
        }
        let cap = if sweep >= TAU { StrokeCap::Round } else { cap };

        Self {
            center,
            inner_radius: inner,
            outer_radius: outer,
            start_angle: start,
            sweep_angle: sweep,
            cap,
        }
    }

    const DEGENERATE: Self = Self {
        center: Point::ZERO,
        inner_radius: 0.0,
        outer_radius: 0.0,
        start_angle: 0.0,
        sweep_angle: 0.0,
        cap: StrokeCap::Butt,
    };

    /// Radius of the band's centerline (`ra` in the analytic arc SDF).
    pub fn mid_radius(&self) -> f32 {
        (self.inner_radius + self.outer_radius) * 0.5
    }

    /// Half the band thickness (`rb` in the analytic arc SDF). Also the radius
    /// of a round cap and the projection distance of a square cap.
    pub fn half_thickness(&self) -> f32 {
        (self.outer_radius - self.inner_radius) * 0.5
    }

    /// True when the band encloses no area and therefore must not be emitted.
    pub fn is_degenerate(&self) -> bool {
        !(self.outer_radius > 0.0
            && self.outer_radius > self.inner_radius
            && self.sweep_angle > 0.0)
    }

    /// True when `angle` lies inside `[start, start + sweep]` (mod `TAU`).
    pub fn contains_angle(&self, angle: f32) -> bool {
        if self.sweep_angle >= TAU {
            return true;
        }
        let delta = wrap_angle_tau(angle - self.start_angle);
        delta <= self.sweep_angle + 1e-6
    }

    /// Scales radii and translates the center. Angles are unchanged, so this is
    /// only valid for a uniform (non-mirroring) scale.
    pub fn scaled_about(&self, center: Point, scale: f32) -> Self {
        Self {
            center,
            inner_radius: self.inner_radius * scale,
            outer_radius: self.outer_radius * scale,
            ..*self
        }
    }

    /// Tight axis-aligned bounding box of the rendered band, caps included.
    ///
    /// The box is the union of
    /// * the two radial ends (inner and outer radius, extended for
    ///   round/square caps), and
    /// * the outer-radius point at every axis direction (0, 90, 180, 270
    ///   degrees) that the sweep actually crosses.
    ///
    /// Sampling only the endpoints would be wrong for any sweep that crosses an
    /// axis: a 0..270 degree sweep reaches `center.x + outer` *and*
    /// `center.x - outer` even though neither endpoint does.
    pub fn bounds(&self) -> Rect {
        if self.is_degenerate() {
            return Rect {
                x: self.center.x,
                y: self.center.y,
                width: 0.0,
                height: 0.0,
            };
        }

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

        let rb = self.half_thickness();
        let ra = self.mid_radius();
        let end_angle = self.start_angle + self.sweep_angle;

        for (angle, outward) in [(self.start_angle, -1.0f32), (end_angle, 1.0f32)] {
            let (sin, cos) = angle.sin_cos();
            match self.cap {
                StrokeCap::Butt => {
                    include(
                        self.center.x + cos * self.inner_radius,
                        self.center.y + sin * self.inner_radius,
                    );
                    include(
                        self.center.x + cos * self.outer_radius,
                        self.center.y + sin * self.outer_radius,
                    );
                }
                StrokeCap::Square => {
                    // Projected along the tangent, away from the sweep.
                    let tx = -sin * rb * outward;
                    let ty = cos * rb * outward;
                    include(
                        self.center.x + cos * self.inner_radius + tx,
                        self.center.y + sin * self.inner_radius + ty,
                    );
                    include(
                        self.center.x + cos * self.outer_radius + tx,
                        self.center.y + sin * self.outer_radius + ty,
                    );
                }
                StrokeCap::Round => {
                    // Semicircle of radius `rb` centered on the band centerline.
                    let cx = self.center.x + cos * ra;
                    let cy = self.center.y + sin * ra;
                    include(cx - rb, cy - rb);
                    include(cx + rb, cy + rb);
                }
            }
        }

        // The axis directions have constant sines and cosines; going through
        // `sin_cos` here doubled the trig cost of every arc in a shape-heavy
        // scene.
        const AXIS_DIRECTIONS: [(f32, f32); 4] = [(0.0, 1.0), (1.0, 0.0), (0.0, -1.0), (-1.0, 0.0)];
        for (quadrant, (sin, cos)) in AXIS_DIRECTIONS.into_iter().enumerate() {
            let angle = quadrant as f32 * std::f32::consts::FRAC_PI_2;
            if self.contains_angle(angle) {
                include(
                    self.center.x + cos * self.outer_radius,
                    self.center.y + sin * self.outer_radius,
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
}

/// Resolves the `(inner, outer, cap)` band described by a
/// [`crate::DrawPrimitive::Arc`].
///
/// * `stroke = Some(_)` — a stroked arc centered on `radius`.
/// * `stroke = None` — a filled annular sector from `inner_radius` to `radius`
///   with flat (butt) radial ends. `inner_radius <= 0` yields a filled pie
///   wedge.
///
/// Non-finite input collapses to an empty band so the caller drops the draw
/// instead of pushing NaN down the pipeline.
pub fn arc_band(radius: f32, inner_radius: f32, stroke: Option<Stroke>) -> (f32, f32, StrokeCap) {
    match stroke {
        Some(stroke) => {
            if !radius.is_finite() || !stroke.is_visible() {
                return (0.0, 0.0, stroke.cap);
            }
            let half = stroke.half_width();
            let radius = radius.max(0.0);
            ((radius - half).max(0.0), radius + half, stroke.cap)
        }
        None => {
            if !radius.is_finite() || !inner_radius.is_finite() {
                return (0.0, 0.0, StrokeCap::Butt);
            }
            let outer = radius.max(0.0);
            let inner = inner_radius.clamp(0.0, outer);
            (inner, outer, StrokeCap::Butt)
        }
    }
}

/// Grows `rect` by `amount` on every side, clamping to a non-negative size.
pub fn inflate_rect(rect: Rect, amount: f32) -> Rect {
    if !amount.is_finite() || amount <= 0.0 {
        return rect;
    }
    Rect {
        x: rect.x - amount,
        y: rect.y - amount,
        width: (rect.width + amount * 2.0).max(0.0),
        height: (rect.height + amount * 2.0).max(0.0),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::f32::consts::{FRAC_PI_2, PI};

    fn approx(a: f32, b: f32) -> bool {
        (a - b).abs() < 1e-3
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
    fn arc_geometry_flags_degenerate_bands() {
        // inner >= outer
        assert!(ArcGeometry::new(Point::ZERO, 5.0, 5.0, 0.0, 1.0, StrokeCap::Butt).is_degenerate());
        assert!(ArcGeometry::new(Point::ZERO, 9.0, 5.0, 0.0, 1.0, StrokeCap::Butt).is_degenerate());
        // zero sweep
        assert!(ArcGeometry::new(Point::ZERO, 1.0, 5.0, 0.0, 0.0, StrokeCap::Butt).is_degenerate());
        // zero radius
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
        // 0 -> 270 degrees crosses +X, +Y, -X and ends on -Y.
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
        // 45 -> 135 degrees only crosses +Y; the minimum y comes from the two
        // *inner* radial endpoints, not from the outer arc.
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
        // The start cap at angle 0 bulges to -rb in y; butt stops at y = 0.
        assert!(approx(butt_bounds.y, 0.0), "{butt_bounds:?}");
        assert!(approx(round_bounds.y, -2.0), "{round_bounds:?}");
        assert!(round_bounds.width >= butt_bounds.width);
        assert!(round_bounds.height >= butt_bounds.height);
    }

    #[test]
    fn arc_bounds_square_caps_project_along_the_tangent() {
        let square = ArcGeometry::new(Point::ZERO, 8.0, 12.0, 0.0, FRAC_PI_2, StrokeCap::Square);
        let bounds = square.bounds();
        // Start cap at angle 0: tangent is +Y, projected backwards by rb = 2.
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

        // inner >= outer clamps rather than producing a negative band.
        let (inner, outer, _) = arc_band(10.0, 40.0, None);
        assert_eq!((inner, outer), (10.0, 10.0));

        // A stroke wider than the radius clamps the inner radius at 0.
        let (inner, outer, _) = arc_band(1.0, 0.0, Some(Stroke::new(10.0)));
        assert_eq!((inner, outer), (0.0, 6.0));
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
}
