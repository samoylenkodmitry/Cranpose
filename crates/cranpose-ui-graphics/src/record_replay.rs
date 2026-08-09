//! Similarity verification over typed draw records.
//!
//! This is the record-level home of the math the wgpu flat-list replay
//! detector applies to materialized primitives: deriving a per-segment
//! similarity transform (uniform scale + rotation about a fixed center) from
//! an anchor pair, and verifying that a freshly recorded entry is the
//! retained entry moved by exactly that transform. Operating on
//! [`SolidArcRecord`]/[`SolidRoundRectRecord`] means the comparison sees the
//! RAW values the app drew with, before arc bands, tight bounds, or
//! `DrawPrimitive` construction — all of which a confirmed match makes
//! unnecessary.
//!
//! Records are solid-brush by construction, so the brush half of
//! verification collapses to a color comparison: geometry match + equal
//! color is [`RecordMatch::Exact`], geometry match + different color is
//! [`RecordMatch::Recolor`] (a retained buffer patch), anything else is
//! [`RecordMatch::Mismatch`] and must take the ordinary path in the same
//! frame. Tolerances are identical to the flat-list detector's; they cover
//! the game's own per-frame float noise, and a real content change is orders
//! of magnitude larger.

use crate::geometry::{Point, Rect, SolidArcRecord, SolidRoundRectRecord};
use crate::CornerRadii;

/// Relative tolerance for similarity verification.
const REL_EPS: f32 = 2e-3;
/// Absolute tolerance for positions/angles near zero, logical px/radians.
const ABS_EPS: f32 = 2e-2;
/// How far apart two entries' implied per-frame transforms may sit while
/// still being grouped into one segment. Much tighter than verification:
/// entries of one ring share literally the same baked rotation step, while
/// neighboring rings differ by a speed delta that accumulates every frame.
const GROUP_SCALE_EPS: f32 = 1e-4;
const GROUP_ANGLE_EPS: f32 = 2e-4;

fn close_rel(a: f32, b: f32) -> bool {
    (a - b).abs() <= ABS_EPS + REL_EPS * a.abs().max(b.abs())
}

fn close_angle(a: f32, b: f32) -> bool {
    use std::f32::consts::TAU;
    let mut d = (a - b) % TAU;
    if d > TAU * 0.5 {
        d -= TAU;
    }
    if d < -TAU * 0.5 {
        d += TAU;
    }
    d.abs() <= ABS_EPS
}

fn close_point(a: Point, b: Point) -> bool {
    close_rel(a.x, b.x) && close_rel(a.y, b.y)
}

/// One segment's frame-over-frame motion: uniform scale and rotation about
/// a shared external center.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct RecordTransform {
    pub scale: f32,
    pub angle: f32,
}

impl RecordTransform {
    pub const IDENTITY: Self = Self {
        scale: 1.0,
        angle: 0.0,
    };

    pub fn apply(&self, center: Point, p: Point) -> Point {
        let (sin, cos) = self.angle.sin_cos();
        let dx = p.x - center.x;
        let dy = p.y - center.y;
        Point::new(
            center.x + (dx * cos - dy * sin) * self.scale,
            center.y + (dx * sin + dy * cos) * self.scale,
        )
    }

    /// Axis-aligned bounds of `bounds` after the transform: the four
    /// transformed corners' box. This is the once-per-group bound transform
    /// that replaces per-entry tight-bounds recomputation for retained
    /// content.
    pub fn apply_to_bounds(&self, center: Point, bounds: Rect) -> Rect {
        let corners = [
            Point::new(bounds.x, bounds.y),
            Point::new(bounds.x + bounds.width, bounds.y),
            Point::new(bounds.x, bounds.y + bounds.height),
            Point::new(bounds.x + bounds.width, bounds.y + bounds.height),
        ];
        let mut min_x = f32::INFINITY;
        let mut min_y = f32::INFINITY;
        let mut max_x = f32::NEG_INFINITY;
        let mut max_y = f32::NEG_INFINITY;
        for corner in corners {
            let p = self.apply(center, corner);
            min_x = min_x.min(p.x);
            min_y = min_y.min(p.y);
            max_x = max_x.max(p.x);
            max_y = max_y.max(p.y);
        }
        Rect {
            x: min_x,
            y: min_y,
            width: max_x - min_x,
            height: max_y - min_y,
        }
    }
}

/// Whether an entry's own implied transform is tightly consistent with a
/// chain's anchor transform. `pinned` marks transforms whose angle is
/// meaningful — an on-pivot circle pins no rotation and joins any chain.
pub fn transforms_group(entry: RecordTransform, entry_pinned: bool, anchor: RecordTransform) -> bool {
    use std::f32::consts::TAU;
    if (entry.scale - anchor.scale).abs() > GROUP_SCALE_EPS * anchor.scale.abs().max(1.0) {
        return false;
    }
    if !entry_pinned {
        return true;
    }
    let mut d = (entry.angle - anchor.angle) % TAU;
    if d > TAU * 0.5 {
        d -= TAU;
    }
    if d < -TAU * 0.5 {
        d += TAU;
    }
    d.abs() <= GROUP_ANGLE_EPS
}

/// The result of verifying one incoming record against its retained
/// counterpart under a segment transform.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RecordMatch {
    Exact,
    /// Geometry matched; only the solid color moved. Replayable with a
    /// 16-byte patch into the retained buffer.
    Recolor,
    Mismatch,
}

/// A circular round-rect (corner radius == half extent on every corner):
/// the one rect family that stays itself under rotation about an external
/// pivot. Returns `(center, diameter)`.
pub fn circle_view(record: &SolidRoundRectRecord) -> Option<(Point, f32)> {
    if !is_circle(record.rect, record.radii) {
        return None;
    }
    Some((
        Point::new(
            record.rect.x + record.rect.width * 0.5,
            record.rect.y + record.rect.height * 0.5,
        ),
        record.rect.width,
    ))
}

/// Whether corner radii + extents describe a circle.
pub fn is_circle(rect: Rect, radii: CornerRadii) -> bool {
    let half = rect.width * 0.5;
    close_rel(rect.width, rect.height)
        && close_rel(radii.top_left, half)
        && close_rel(radii.top_right, half)
        && close_rel(radii.bottom_right, half)
        && close_rel(radii.bottom_left, half)
}

fn stroke_width(record_stroke: Option<crate::Stroke>) -> Option<f32> {
    record_stroke.map(|stroke| stroke.width)
}

/// Similarity-invariant compatibility of a fresh arc with a retained one,
/// for re-locating a segment when dynamic spans change length. Colors are
/// deliberately excluded — a twinkling anchor must still re-anchor its
/// segment. A false positive costs a failed probe, never a wrong pixel.
pub fn arcs_anchor_compatible(current: &SolidArcRecord, anchor: &SolidArcRecord) -> bool {
    close_rel(current.sweep_angle, anchor.sweep_angle)
        && current.stroke.is_some() == anchor.stroke.is_some()
}

/// Derives the segment transform from an arc anchor pair. Arcs pin both
/// scale and rotation exactly.
pub fn arc_anchor_transform(
    current: &SolidArcRecord,
    retained: &SolidArcRecord,
) -> Option<RecordTransform> {
    if retained.radius <= f32::EPSILON {
        return None;
    }
    Some(RecordTransform {
        scale: current.radius / retained.radius,
        angle: current.start_angle - retained.start_angle,
    })
}

/// Derives the segment transform from a circle anchor pair, with its
/// pinnedness (an on-pivot circle pins no rotation).
pub fn circle_anchor_transform_pinned(
    current: (Point, f32),
    retained: (Point, f32),
    center: Point,
) -> Option<(RecordTransform, bool)> {
    let (c_now, d_now) = current;
    let (c_then, d_then) = retained;
    if d_then <= f32::EPSILON {
        return None;
    }
    let scale = d_now / d_then;
    let dx_then = c_then.x - center.x;
    let dy_then = c_then.y - center.y;
    let pinned = dx_then * dx_then + dy_then * dy_then > 1.0;
    let angle = if pinned {
        let dx_now = c_now.x - center.x;
        let dy_now = c_now.y - center.y;
        dy_now.atan2(dx_now) - dy_then.atan2(dx_then)
    } else {
        0.0
    };
    Some((RecordTransform { scale, angle }, pinned))
}

/// Verifies a fresh arc record against the retained one under `t`. Arc
/// centers must sit on the shared pivot — that is what makes rotation a
/// value change instead of a position change.
pub fn match_arc(
    current: &SolidArcRecord,
    retained: &SolidArcRecord,
    center: Point,
    t: RecordTransform,
) -> RecordMatch {
    let geometry_ok = close_point(current.center, retained.center)
        && close_point(current.center, center)
        && close_rel(current.radius, retained.radius * t.scale)
        && close_rel(current.inner_radius, retained.inner_radius * t.scale)
        && close_angle(current.start_angle, retained.start_angle + t.angle)
        && close_rel(current.sweep_angle, retained.sweep_angle)
        && match (stroke_width(current.stroke), stroke_width(retained.stroke)) {
            (None, None) => true,
            (Some(now), Some(then)) => close_rel(now, then * t.scale),
            _ => false,
        };
    if !geometry_ok {
        return RecordMatch::Mismatch;
    }
    if current.color == retained.color {
        RecordMatch::Exact
    } else {
        RecordMatch::Recolor
    }
}

/// Verifies a fresh circular round-rect against the retained one under `t`.
/// Non-circular round rects never match — they do not survive rotation
/// about an external pivot.
pub fn match_round_rect(
    current: &SolidRoundRectRecord,
    retained: &SolidRoundRectRecord,
    center: Point,
    t: RecordTransform,
) -> RecordMatch {
    let (Some((c_now, d_now)), Some((c_then, d_then))) =
        (circle_view(current), circle_view(retained))
    else {
        return RecordMatch::Mismatch;
    };
    let geometry_ok = close_point(c_now, t.apply(center, c_then))
        && close_rel(d_now, d_then * t.scale)
        && match (stroke_width(current.stroke), stroke_width(retained.stroke)) {
            (None, None) => true,
            (Some(now), Some(then)) => close_rel(now, then * t.scale),
            _ => false,
        };
    if !geometry_ok {
        return RecordMatch::Mismatch;
    }
    if current.color == retained.color {
        RecordMatch::Exact
    } else {
        RecordMatch::Recolor
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Color, Stroke};

    const CENTER: Point = Point { x: 204.0, y: 204.0 };

    fn arc(radius: f32, start: f32, color: Color) -> SolidArcRecord {
        SolidArcRecord {
            center: CENTER,
            radius,
            start_angle: start,
            sweep_angle: 0.4,
            inner_radius: radius * 0.8,
            color,
            stroke: None,
        }
    }

    fn moved_arc(base: &SolidArcRecord, t: RecordTransform) -> SolidArcRecord {
        SolidArcRecord {
            center: base.center,
            radius: base.radius * t.scale,
            start_angle: base.start_angle + t.angle,
            sweep_angle: base.sweep_angle,
            inner_radius: base.inner_radius * t.scale,
            color: base.color,
            stroke: base.stroke.map(|stroke| Stroke {
                width: stroke.width * t.scale,
                ..stroke
            }),
        }
    }

    fn circle(cx: f32, cy: f32, diameter: f32, color: Color) -> SolidRoundRectRecord {
        SolidRoundRectRecord {
            rect: Rect {
                x: cx - diameter * 0.5,
                y: cy - diameter * 0.5,
                width: diameter,
                height: diameter,
            },
            radii: CornerRadii::uniform(diameter * 0.5),
            color,
            stroke: None,
        }
    }

    #[test]
    fn arc_anchor_recovers_the_baked_transform() {
        let t = RecordTransform {
            scale: 0.9994,
            angle: 0.0123,
        };
        let retained = arc(120.0, 1.0, Color::WHITE);
        let current = moved_arc(&retained, t);
        let derived = arc_anchor_transform(&current, &retained).expect("derivable");
        assert!((derived.scale - t.scale).abs() < 1e-6);
        assert!((derived.angle - t.angle).abs() < 1e-6);
        assert_eq!(
            match_arc(&current, &retained, CENTER, derived),
            RecordMatch::Exact
        );
    }

    #[test]
    fn recolored_arc_matches_as_recolor() {
        let t = RecordTransform {
            scale: 1.0,
            angle: 0.05,
        };
        let retained = arc(80.0, 0.2, Color::WHITE);
        let mut current = moved_arc(&retained, t);
        current.color = Color::rgb(0.5, 0.1, 0.9);
        assert_eq!(
            match_arc(&current, &retained, CENTER, t),
            RecordMatch::Recolor
        );
    }

    #[test]
    fn changed_sweep_is_a_mismatch() {
        let t = RecordTransform::IDENTITY;
        let retained = arc(80.0, 0.2, Color::WHITE);
        let mut current = retained;
        current.sweep_angle += 0.1;
        assert_eq!(
            match_arc(&current, &retained, CENTER, t),
            RecordMatch::Mismatch
        );
    }

    #[test]
    fn stroked_arc_scales_its_width_with_the_segment() {
        let t = RecordTransform {
            scale: 0.98,
            angle: 0.0,
        };
        let mut retained = arc(60.0, 0.0, Color::WHITE);
        retained.stroke = Some(Stroke::new(5.0));
        let current = moved_arc(&retained, t);
        assert_eq!(match_arc(&current, &retained, CENTER, t), RecordMatch::Exact);

        // An unscaled stroke under a scaling segment is a real change: 2%
        // of 5px is well past the noise tolerance.
        let mut stale = current;
        stale.stroke = Some(Stroke::new(5.0));
        assert_eq!(
            match_arc(&stale, &retained, CENTER, t),
            RecordMatch::Mismatch
        );
    }

    #[test]
    fn orbiting_circle_matches_under_rotation() {
        let t = RecordTransform {
            scale: 1.0,
            angle: 0.3,
        };
        let retained = circle(304.0, 204.0, 10.0, Color::WHITE);
        let (c_then, d_then) = circle_view(&retained).expect("circle");
        let c_now = t.apply(CENTER, c_then);
        let current = circle(c_now.x, c_now.y, d_then * t.scale, Color::WHITE);
        let (derived, pinned) =
            circle_anchor_transform_pinned(circle_view(&current).unwrap(), (c_then, d_then), CENTER)
                .expect("derivable");
        assert!(pinned, "an off-pivot circle pins rotation");
        assert!((derived.angle - t.angle).abs() < 1e-4);
        assert_eq!(
            match_round_rect(&current, &retained, CENTER, derived),
            RecordMatch::Exact
        );
    }

    #[test]
    fn non_circular_round_rect_never_matches() {
        let mut retained = circle(304.0, 204.0, 10.0, Color::WHITE);
        retained.rect.width = 14.0; // no longer a circle
        assert_eq!(
            match_round_rect(&retained, &retained, CENTER, RecordTransform::IDENTITY),
            RecordMatch::Mismatch
        );
    }

    #[test]
    fn grouping_is_tighter_than_verification() {
        let anchor = RecordTransform {
            scale: 1.0,
            angle: 0.010,
        };
        let same_ring = RecordTransform {
            scale: 1.0,
            angle: 0.0100001,
        };
        let next_ring = RecordTransform {
            scale: 1.0,
            angle: 0.011,
        };
        assert!(transforms_group(same_ring, true, anchor));
        assert!(
            !transforms_group(next_ring, true, anchor),
            "a 1e-3 rotation-step difference is another ring, not float noise"
        );
        let unpinned = RecordTransform {
            scale: 1.0,
            angle: 0.0,
        };
        assert!(transforms_group(unpinned, false, anchor));
    }

    #[test]
    fn transformed_bounds_contain_the_moved_content() {
        let t = RecordTransform {
            scale: 1.1,
            angle: 0.5,
        };
        let bounds = Rect {
            x: 150.0,
            y: 150.0,
            width: 100.0,
            height: 30.0,
        };
        let moved = t.apply_to_bounds(CENTER, bounds);
        for corner in [
            Point::new(bounds.x, bounds.y),
            Point::new(bounds.x + bounds.width, bounds.y + bounds.height),
        ] {
            let p = t.apply(CENTER, corner);
            assert!(p.x >= moved.x - 1e-3 && p.x <= moved.x + moved.width + 1e-3);
            assert!(p.y >= moved.y - 1e-3 && p.y <= moved.y + moved.height + 1e-3);
        }
    }
}
