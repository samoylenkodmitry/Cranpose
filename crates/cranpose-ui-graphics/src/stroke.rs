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
        Self::with_angles(center, inner, outer, start_angle, sweep_angle, cap)
    }

    /// [`Self::new`] for the radii [`arc_band`] returns, which already hold
    /// `0 <= inner <= outer`: the same geometry without clamping them again.
    #[inline]
    pub(crate) fn of_band(
        center: Point,
        (inner, outer, cap): (f32, f32, StrokeCap),
        start_angle: f32,
        sweep_angle: f32,
    ) -> Self {
        if !all_finite([center.x, center.y, inner, outer, start_angle, sweep_angle]) {
            return Self::DEGENERATE;
        }
        Self::with_angles(center, inner, outer, start_angle, sweep_angle, cap)
    }

    /// The geometry of normalized radii with its sweep made positive and at
    /// most a full turn, and its start wrapped into `[0, TAU)`.
    #[inline(always)]
    fn with_angles(
        center: Point,
        inner: f32,
        outer: f32,
        start_angle: f32,
        sweep_angle: f32,
        cap: StrokeCap,
    ) -> Self {
        // A sweep in (0, TAU) from a start in [0, TAU) is already normal:
        // told apart by the bits alone, with no float compare, each an
        // FPSCR transfer on armv7 (a positive finite float's bits order as
        // its value; zero, negatives, infinity and NaN fall outside).
        if sweep_angle.to_bits().wrapping_sub(1) < TAU.to_bits() - 1
            && start_angle.to_bits() < TAU.to_bits()
        {
            return Self {
                center,
                inner_radius: inner,
                outer_radius: outer,
                start_angle,
                sweep_angle,
                cap,
            };
        }
        Self::normalizing_angles(center, inner, outer, start_angle, sweep_angle, cap)
    }

    /// [`Self::with_angles`] by comparing the angles as floats.
    #[inline]
    fn normalizing_angles(
        center: Point,
        inner: f32,
        outer: f32,
        start_angle: f32,
        sweep_angle: f32,
        cap: StrokeCap,
    ) -> Self {
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

/// A stroked straight segment: its two ends, half its width and its cap,
/// in the units it is drawn in. [`crate::DrawPrimitive::Line`] lowers to it,
/// and the GPU and CPU renderers take their coverage from the same frame.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct LineGeometry {
    pub start: Point,
    pub end: Point,
    pub half_width: f32,
    pub cap: StrokeCap,
}

/// Where a pixel sits relative to a [`LineGeometry`]: its midpoint, the unit
/// direction from start to end, and half its length.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct LineFrame {
    pub center: Point,
    pub direction: Point,
    pub half_length: f32,
}

impl LineGeometry {
    /// The segment from `start` to `end` stroked with `stroke`.
    pub fn new(start: Point, end: Point, stroke: Stroke) -> Self {
        Self {
            start,
            end,
            half_width: stroke.half_width(),
            cap: stroke.cap,
        }
    }

    /// True when the segment covers nothing: a non-finite input, no width,
    /// or no length between butt caps, as Compose's `drawLine` draws nothing
    /// there.
    pub fn is_degenerate(&self) -> bool {
        !all_finite([self.start.x, self.start.y, self.end.x, self.end.y])
            || !(self.half_width > 0.0 && self.half_width.is_finite())
            || (self.cap == StrokeCap::Butt && self.start == self.end)
    }

    /// The midpoint, unit direction and half length coverage is measured
    /// in. A segment of no length points along +X, so its round or square
    /// cap still draws a dot.
    pub fn frame(&self) -> LineFrame {
        let (dx, dy) = (self.end.x - self.start.x, self.end.y - self.start.y);
        let length = (dx * dx + dy * dy).sqrt();
        let direction = if length > 0.0 {
            Point::new(dx / length, dy / length)
        } else {
            Point::new(1.0, 0.0)
        };
        LineFrame {
            center: Point::new(
                (self.start.x + self.end.x) * 0.5,
                (self.start.y + self.end.y) * 0.5,
            ),
            direction,
            half_length: length * 0.5,
        }
    }

    /// How far past each end the stroke reaches: half the width for round
    /// and square caps, nothing for butt caps.
    pub fn cap_reach(&self) -> f32 {
        if self.cap == StrokeCap::Butt {
            0.0
        } else {
            self.half_width
        }
    }

    /// The axis-aligned box of the ends, the rect the primitive carries.
    pub fn end_bounds(&self) -> Rect {
        let min_x = self.start.x.min(self.end.x);
        let min_y = self.start.y.min(self.end.y);
        Rect {
            x: min_x,
            y: min_y,
            width: self.start.x.max(self.end.x) - min_x,
            height: self.start.y.max(self.end.y) - min_y,
        }
    }

    /// How far past the ends' box the stroke reaches on each axis: a round
    /// end's disc bounds it by half the width every way; a butt or square
    /// end's corners reach along the segment by the cap and across it by
    /// half the width, which on a slant is more than half the width.
    pub fn reach(&self) -> Point {
        if self.cap == StrokeCap::Round {
            return Point::new(self.half_width, self.half_width);
        }
        let frame = self.frame();
        let (along_x, along_y) = (frame.direction.x.abs(), frame.direction.y.abs());
        let cap = self.cap_reach();
        Point::new(
            along_x * cap + along_y * self.half_width,
            along_y * cap + along_x * self.half_width,
        )
    }

    /// The pixels the stroke can reach: the ends' box grown by
    /// [`Self::reach`].
    pub fn bounds(&self) -> Rect {
        let ends = self.end_bounds();
        let reach = self.reach();
        Rect {
            x: ends.x - reach.x,
            y: ends.y - reach.y,
            width: ends.width + reach.x * 2.0,
            height: ends.height + reach.y * 2.0,
        }
    }

    /// The same segment with its ends at `start` and `end` and its width
    /// scaled by `scale`, as a layer's transform places it.
    pub fn placed(&self, start: Point, end: Point, scale: f32) -> Self {
        Self {
            start,
            end,
            half_width: self.half_width * scale,
            ..*self
        }
    }

    /// The share of the pixel centred at `point` the stroke covers: exact
    /// box coverage across the segment and along it, as a rect's edges are
    /// covered, so a thin line keeps its weight at any sub-pixel offset;
    /// past the ends of a round cap, the distance to the cap's disc.
    /// `shape.wgsl` mirrors it.
    pub fn coverage(&self, point: Point) -> f32 {
        let frame = self.frame();
        let (dx, dy) = (point.x - frame.center.x, point.y - frame.center.y);
        let along = (dx * frame.direction.x + dy * frame.direction.y).abs();
        let across = (dx * frame.direction.y - dy * frame.direction.x).abs();
        let across_coverage = (self.half_width + 0.5 - across).clamp(0.0, 1.0);
        if self.cap == StrokeCap::Round {
            if along <= frame.half_length {
                return across_coverage;
            }
            let (past, side) = (along - frame.half_length, across);
            let distance = (past * past + side * side).sqrt() - self.half_width;
            return (0.5 - distance).clamp(0.0, 1.0);
        }
        let reach = frame.half_length + self.cap_reach();
        across_coverage * (reach + 0.5 - along).clamp(0.0, 1.0)
    }
}

#[cfg(test)]
#[path = "tests/stroke_tests.rs"]
mod tests;
