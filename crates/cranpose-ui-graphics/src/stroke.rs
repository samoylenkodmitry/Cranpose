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

use crate::{
    Point, Rect,
    float::{all_finite, at_least, within},
};

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
            at_least(self.width * 0.5, 0.0)
        } else {
            0.0
        }
    }

    /// A stroke is renderable only when it has a strictly positive, finite width.
    pub fn is_visible(&self) -> bool {
        // The positive finite floats are the bit patterns 1 through the
        // largest finite one; zero, the negatives, infinity and NaN fall
        // outside. One integer compare instead of two float compares, each
        // an FPSCR transfer on armv7, for every stroked primitive recorded.
        self.width.to_bits().wrapping_sub(1) < f32::MAX.to_bits()
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

/// Exact `x.floor()` without the libm call `f32::floor` lowers to on armv7
/// (no `vrintm` there): truncate via int cast, fix up negatives. Bit-equal
/// to `floorf` for every input — casts only run below 2^23, where i32 cannot
/// saturate, and at 2^23 and above every finite f32 is already an integer.
/// NaN fails the range test and passes through unchanged, like `floorf`.
#[inline]
fn exact_floor(x: f32) -> f32 {
    if x == 0.0 {
        return x;
    }
    if x.abs() < 8_388_608.0 {
        let truncated = x as i32 as f32;
        truncated - ((x < truncated) as i32 as f32)
    } else {
        x
    }
}

/// `x mod TAU` into `[0, TAU)` without `rem_euclid`, whose `fmodf` lowers to
/// the software routine in compiler_builtins on aarch64 Android and shows up
/// in profiles at two calls per arc per frame. Multiply-floor keeps it to a
/// couple of instructions; the fixup folds the one-ulp overshoot cases back
/// into range.
#[inline]
fn wrap_angle_tau(x: f32) -> f32 {
    if (0.0..TAU).contains(&x) {
        return x;
    }
    let wrapped = x - exact_floor(x * (1.0 / TAU)) * TAU;
    if wrapped >= TAU {
        wrapped - TAU
    } else if wrapped < 0.0 {
        0.0
    } else {
        wrapped
    }
}

/// `(sin, cos)` by refined parabola, absolute error under [`FAST_TRIG_ERR`].
/// Bounding boxes only need trig that is close — the box gets padded by the
/// worst-case position error afterwards — and libm's `sincosf`, called twice
/// per partial arc, was one of the larger single costs of recording a
/// shape-heavy frame on a watch-class core.
#[inline]
fn fast_sin_cos(angle: f32) -> (f32, f32) {
    use std::f32::consts::{FRAC_PI_2, PI};
    #[inline]
    fn fold_sin(x: f32) -> f32 {
        const B: f32 = 4.0 / PI;
        const C: f32 = -4.0 / (PI * PI);
        let y = B * x + C * x * x.abs();
        0.225 * (y * y.abs() - y) + y
    }
    let x = wrap_angle_tau(angle);
    let x = if x > PI { x - TAU } else { x };
    let mut c = x + FRAC_PI_2;
    if c > PI {
        c -= TAU;
    }
    (fold_sin(x), fold_sin(c))
}

/// Worst-case absolute error of [`fast_sin_cos`]; bounds derived from it are
/// padded by radius x this so the approximate box always contains the exact
/// shape.
const FAST_TRIG_ERR: f32 = 1.3e-3;

impl ArcGeometry {
    /// Normalizing constructor. Never panics and never stores a NaN.
    #[inline]
    pub fn new(
        center: Point,
        inner_radius: f32,
        outer_radius: f32,
        start_angle: f32,
        sweep_angle: f32,
        cap: StrokeCap,
    ) -> Self {
        if !all_finite([
            center.x,
            center.y,
            inner_radius,
            outer_radius,
            start_angle,
            sweep_angle,
        ]) {
            return Self::DEGENERATE;
        }

        let outer = at_least(outer_radius, 0.0);
        let inner = within(inner_radius, 0.0, outer);

        let (mut start, mut sweep) = if sweep_angle < 0.0 {
            (start_angle + sweep_angle, -sweep_angle)
        } else {
            (start_angle, sweep_angle)
        };
        if sweep >= TAU {
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

        if self.sweep_angle >= TAU && self.cap != StrokeCap::Square {
            let r = self.outer_radius;
            return Rect {
                x: self.center.x - r,
                y: self.center.y - r,
                width: r + r,
                height: r + r,
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
            let (sin, cos) = fast_sin_cos(angle);
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
                    let cx = self.center.x + cos * ra;
                    let cy = self.center.y + sin * ra;
                    include(cx - rb, cy - rb);
                    include(cx + rb, cy + rb);
                }
            }
        }

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

        let pad = (self.outer_radius + rb) * FAST_TRIG_ERR + 0.02;
        Rect {
            x: min_x - pad,
            y: min_y - pad,
            width: (max_x - min_x + pad + pad).max(0.0),
            height: (max_y - min_y + pad + pad).max(0.0),
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
#[inline]
pub fn arc_band(radius: f32, inner_radius: f32, stroke: Option<Stroke>) -> (f32, f32, StrokeCap) {
    match stroke {
        Some(stroke) => {
            if !radius.is_finite() || !stroke.is_visible() {
                return (0.0, 0.0, stroke.cap);
            }
            let half = stroke.half_width();
            let radius = at_least(radius, 0.0);
            (at_least(radius - half, 0.0), radius + half, stroke.cap)
        }
        None => {
            if !radius.is_finite() || !inner_radius.is_finite() {
                return (0.0, 0.0, StrokeCap::Butt);
            }
            let outer = at_least(radius, 0.0);
            let inner = within(inner_radius, 0.0, outer);
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
#[path = "tests/stroke_tests.rs"]
mod tests;
