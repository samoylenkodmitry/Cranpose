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

use crate::{
    Color, CornerRadii,
    geometry::{
        CommandRecording, Point, RecordKind, Rect, SolidArcRecord, SolidRoundRectRecord, TapeRef,
    },
};

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
    let d = a - b;
    if d.abs() < TAU {
        let wrapped = if d > TAU * 0.5 {
            d - TAU
        } else if d < -TAU * 0.5 {
            d + TAU
        } else {
            d
        };
        return wrapped.abs() <= ABS_EPS;
    }
    let mut d = d % TAU;
    if d > TAU * 0.5 {
        d -= TAU;
    }
    if d < -TAU * 0.5 {
        d += TAU;
    }
    d.abs() <= ABS_EPS
}

#[cfg(test)]
mod close_angle_equivalence {
    use super::*;

    fn close_angle_reference(a: f32, b: f32) -> bool {
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

    #[test]
    fn fast_path_matches_the_fmod_form() {
        use std::f32::consts::{PI, TAU};
        let interesting = [
            0.0_f32,
            -0.0,
            1e-8,
            -1e-8,
            0.019,
            -0.019,
            0.021,
            -0.021,
            1.0,
            -1.0,
            PI - 1e-3,
            PI,
            PI + 1e-3,
            -PI,
            TAU - 0.02,
            TAU - 1e-6,
            TAU,
            TAU + 1e-6,
            TAU + 0.019,
            -TAU,
            -TAU - 0.019,
            3.0 * TAU + 0.01,
            -7.5 * TAU,
            123.456,
            -987.654,
            f32::NAN,
            f32::INFINITY,
            f32::NEG_INFINITY,
            f32::MAX,
        ];
        for &a in &interesting {
            for &b in &interesting {
                assert_eq!(
                    close_angle(a, b),
                    close_angle_reference(a, b),
                    "close_angle({a}, {b}) diverged from the fmod form"
                );
            }
        }
        for anchor in [-500.0_f32, -6.0, 0.0, 6.0, 500.0] {
            for i in -2520..=2520 {
                let d = i as f32 * 0.01;
                assert_eq!(
                    close_angle(anchor + d, anchor),
                    close_angle_reference(anchor + d, anchor),
                    "sweep diverged at anchor {anchor} delta {d}"
                );
            }
        }
    }
}

fn close_point(a: Point, b: Point) -> bool {
    close_rel(a.x, b.x) & close_rel(a.y, b.y)
}

/// One segment's frame-over-frame motion: uniform scale and rotation about
/// a shared external center.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct RecordTransform {
    pub scale: f32,
    pub angle: f32,
}

/// [`RecordTransform::apply`]'s arithmetic with the rotation's sin/cos
/// supplied by the caller. The contiguous round-rect run loop hoists
/// `sin_cos` — a libm call — out of its per-record body through this seam,
/// one call per run instead of one per record, while `apply` stays the one
/// expression of the motion: both forms run this exact arithmetic on the
/// same values, so the hoist is pure common-subexpression reuse and cannot
/// move a float.
#[inline(always)]
fn apply_parts(scale: f32, sin: f32, cos: f32, center: Point, p: Point) -> Point {
    let dx = p.x - center.x;
    let dy = p.y - center.y;
    Point::new(
        center.x + (dx * cos - dy * sin) * scale,
        center.y + (dx * sin + dy * cos) * scale,
    )
}

impl RecordTransform {
    pub const IDENTITY: Self = Self {
        scale: 1.0,
        angle: 0.0,
    };

    pub fn apply(&self, center: Point, p: Point) -> Point {
        let (sin, cos) = self.angle.sin_cos();
        apply_parts(self.scale, sin, cos, center, p)
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
pub fn transforms_group(
    entry: RecordTransform,
    entry_pinned: bool,
    anchor: RecordTransform,
) -> bool {
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
        & close_rel(radii.top_left, half)
        & close_rel(radii.top_right, half)
        & close_rel(radii.bottom_right, half)
        & close_rel(radii.bottom_left, half)
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
///
/// This is the semantically AUTHORITATIVE arc comparison. The serial
/// per-pair path (probes, partition, alignment) calls it directly; the
/// contiguous run loop runs `match_arc_lanes`, its lane-shaped twin,
/// which is pinned to this function verdict-for-verdict by the exhaustive
/// `lane_kernel_equivalence` corpus — so the tolerance semantics cannot
/// drift between them. The tolerance terms combine with `&`, not `&&`: each
/// `close_*` is a pure comparison, so evaluating all of them unconditionally
/// is result-identical to the short-circuit form (a NaN in any field makes
/// its own comparison false regardless of order) while the common all-match
/// case takes one branch per record instead of seven.
pub fn match_arc(
    current: &SolidArcRecord,
    retained: &SolidArcRecord,
    center: Point,
    t: RecordTransform,
) -> RecordMatch {
    let stroke_ok = match (stroke_width(current.stroke), stroke_width(retained.stroke)) {
        (None, None) => true,
        (Some(now), Some(then)) => close_rel(now, then * t.scale),
        _ => false,
    };
    let geometry_ok = close_point(current.center, retained.center)
        & close_point(current.center, center)
        & close_rel(current.radius, retained.radius * t.scale)
        & close_rel(current.inner_radius, retained.inner_radius * t.scale)
        & close_angle(current.start_angle, retained.start_angle + t.angle)
        & close_rel(current.sweep_angle, retained.sweep_angle)
        & stroke_ok;
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
///
/// Like [`match_arc`], this is the semantically authoritative round-rect
/// comparison, called directly by the serial per-pair path; the contiguous
/// run loop runs `match_round_rect_lanes`, its equivalence-pinned
/// lane-shaped twin. The tolerance terms combine with `&` because each is
/// pure, so unconditional evaluation is result-identical (NaN included) and
/// the all-match case stays branch-light.
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
    let stroke_ok = match (stroke_width(current.stroke), stroke_width(retained.stroke)) {
        (None, None) => true,
        (Some(now), Some(then)) => close_rel(now, then * t.scale),
        _ => false,
    };
    let geometry_ok = close_point(c_now, t.apply(center, c_then))
        & close_rel(d_now, d_then * t.scale)
        & stroke_ok;
    if !geometry_ok {
        return RecordMatch::Mismatch;
    }
    if current.color == retained.color {
        RecordMatch::Exact
    } else {
        RecordMatch::Recolor
    }
}

/// Below this many entries a stable stretch is not worth a retained group.
/// Mirrors the flat-list detector.
pub const MIN_SEGMENT_RECORDS: usize = 128;
/// Chains longer than this split into multiple groups, bounding the blast
/// radius of any one entry going dynamic later.
pub const MAX_SEGMENT_RECORDS: usize = 2048;
/// Below this many records a command is not worth watching at all.
pub const MIN_REPLAY_COMMAND_RECORDS: usize = 512;
/// Structural-resync search span when entity churn inserts/removes entries
/// between frames. Mirrors the flat-list detector's bounded resync.
const RESYNC_SPAN: usize = 48;
const MAX_RESYNC_EVENTS: usize = 512;
/// How far past its expected position a segment anchor may drift when the
/// dynamic spans between segments change length.
const RESYNC_WINDOW: usize = 1024;
/// Entries probed under a candidate anchor transform before committing to a
/// full-segment verification.
const ANCHOR_PROBE_RECORDS: usize = 4;
/// Full-span verifications a segment may commit to per frame. Self-similar
/// rings can pass the probe from a wrong anchor (every entry shares the
/// candidate's radius and angle step), so one failed commitment must not
/// abandon the search — but unbounded re-verification of 2048-entry spans
/// must not either.
const MAX_COMMIT_ATTEMPTS: usize = 4;
/// When live coverage sinks below this fraction of the retained records,
/// re-partition from scratch.
const MIN_COVERAGE_FRACTION: f32 = 0.5;
/// Coverage eroding this far below what the capture achieved re-partitions
/// to win dead ranges back — deaths are permanent otherwise, while the
/// content they covered usually stabilizes again a moment later.
const RECAPTURE_EROSION: f32 = 0.05;
/// Frames a capture must survive before erosion alone may retire it. Keeps
/// an inherently churning scene from recapturing in a loop — at worst one
/// two-frame recapture per cooldown.
const RECAPTURE_COOLDOWN_FRAMES: u32 = 180;

/// The similarity-checkable view of one tape entry: which typed store it
/// lives in and its index there. `None` marks entries replay cannot carry
/// (plain rects, ordinary primitives) — they break segments wherever they
/// sit.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum ReplayView {
    Arc(usize),
    RoundRect(usize),
}

/// The replay-checkable view of tape entry `i`, decoded on the fly from the
/// tagged tape: `None` for entries replay cannot carry (plain rects,
/// ordinary primitives, non-circular round rects). This is THE eligibility
/// rule — both the `&CommandRecording` form and the [`TypedRecords`] form
/// delegate here, so they cannot drift.
fn view_at_slices(
    tape: &[TapeRef],
    round_rects: &[SolidRoundRectRecord],
    i: usize,
) -> Option<ReplayView> {
    let entry = tape[i];
    match entry.kind() {
        RecordKind::SolidArc => Some(ReplayView::Arc(entry.index())),
        RecordKind::SolidRoundRect => circle_view(&round_rects[entry.index()])
            .is_some()
            .then_some(ReplayView::RoundRect(entry.index())),
        RecordKind::SolidRect | RecordKind::Other => None,
    }
}

/// [`view_at_slices`] over a whole recording.
fn view_at(recording: &CommandRecording, i: usize) -> Option<ReplayView> {
    view_at_slices(&recording.tape, &recording.round_rects, i)
}

/// The shared rotation/scale pivot of a recording: the first arc's center.
fn detect_center(recording: &CommandRecording) -> Option<Point> {
    recording.arcs.first().map(|arc| arc.center)
}

/// Similarity-invariant compatibility of a current entry with a retained
/// one, for structural pairing under churn. Colors excluded by design.
fn views_compatible(
    current: &CommandRecording,
    current_view: Option<ReplayView>,
    retained: &CommandRecording,
    retained_view: Option<ReplayView>,
) -> bool {
    match (current_view, retained_view) {
        (Some(ReplayView::Arc(i)), Some(ReplayView::Arc(j))) => {
            arcs_anchor_compatible(&current.arcs[i], &retained.arcs[j])
        }
        (Some(ReplayView::RoundRect(i)), Some(ReplayView::RoundRect(j))) => {
            let now = current.round_rects[i].stroke.is_some();
            let then = retained.round_rects[j].stroke.is_some();
            now == then
        }
        (None, None) => true,
        _ => false,
    }
}

/// Pairs current tape entries with retained tape entries, tolerating bounded
/// insertions and deletions (entity churn between frames). Pairing is
/// structural only; transform-consistency during verification decides
/// whether a pair actually moved together, so a wrong pairing costs a
/// segment, never a wrong capture. Fills `aligned` (cleared, then resized
/// to the current tape length): an out-param, so the caller owns a
/// reusable buffer instead of allocating ~tape-length per call.
fn align_recordings(
    current: &CommandRecording,
    retained: &CommandRecording,
    aligned: &mut Vec<Option<usize>>,
) {
    let pair = |i: usize, j: usize| -> bool {
        views_compatible(current, view_at(current, i), retained, view_at(retained, j))
    };
    let current_len = current.tape.len();
    let retained_len = retained.tape.len();
    aligned.clear();
    aligned.resize(current_len, None);
    let (mut i, mut j) = (0usize, 0usize);
    let mut events = 0usize;
    while i < current_len && j < retained_len {
        if pair(i, j) {
            aligned[i] = Some(j);
            i += 1;
            j += 1;
            continue;
        }
        events += 1;
        if events > MAX_RESYNC_EVENTS {
            aligned.fill(None);
            return;
        }
        let mut resynced = false;
        'search: for total in 1..=RESYNC_SPAN {
            for di in 0..=total {
                let dj = total - di;
                if i + di < current_len && j + dj < retained_len && pair(i + di, j + dj) {
                    i += di;
                    j += dj;
                    resynced = true;
                    break 'search;
                }
            }
        }
        if !resynced {
            i += 1;
            j += 1;
        }
    }
}

/// Derives the pair's implied transform, with pinnedness.
fn pair_transform(
    current: &CommandRecording,
    current_view: ReplayView,
    retained: &CommandRecording,
    retained_view: ReplayView,
    center: Point,
) -> Option<(RecordTransform, bool)> {
    match (current_view, retained_view) {
        (ReplayView::Arc(i), ReplayView::Arc(j)) => {
            arc_anchor_transform(&current.arcs[i], &retained.arcs[j]).map(|t| (t, true))
        }
        (ReplayView::RoundRect(i), ReplayView::RoundRect(j)) => {
            let now = circle_view(&current.round_rects[i])?;
            let then = circle_view(&retained.round_rects[j])?;
            circle_anchor_transform_pinned(now, then, center)
        }
        _ => None,
    }
}

/// Verifies one aligned pair under a segment transform.
fn match_pair(
    current: &CommandRecording,
    current_view: ReplayView,
    retained: &CommandRecording,
    retained_view: ReplayView,
    center: Point,
    t: RecordTransform,
) -> RecordMatch {
    match (current_view, retained_view) {
        (ReplayView::Arc(i), ReplayView::Arc(j)) => {
            match_arc(&current.arcs[i], &retained.arcs[j], center, t)
        }
        (ReplayView::RoundRect(i), ReplayView::RoundRect(j)) => {
            match_round_rect(&current.round_rects[i], &retained.round_rects[j], center, t)
        }
        _ => RecordMatch::Mismatch,
    }
}

/// Loose logical bounds of a retained tape range: shapes bound by their full
/// outer circle. Visibility culling only needs containment.
fn range_bounds(recording: &CommandRecording, range: (usize, usize)) -> Rect {
    let mut min_x = f32::INFINITY;
    let mut min_y = f32::INFINITY;
    let mut max_x = f32::NEG_INFINITY;
    let mut max_y = f32::NEG_INFINITY;
    for view in (range.0..range.1).filter_map(|i| view_at(recording, i)) {
        let (center, reach) = match view {
            ReplayView::Arc(i) => {
                let arc = &recording.arcs[i];
                (
                    arc.center,
                    arc.radius + arc.stroke.map(|stroke| stroke.width).unwrap_or(0.0),
                )
            }
            ReplayView::RoundRect(i) => {
                let record = &recording.round_rects[i];
                let Some((center, diameter)) = circle_view(record) else {
                    continue;
                };
                (
                    center,
                    diameter * 0.5 + record.stroke.map(|stroke| stroke.width).unwrap_or(0.0),
                )
            }
        };
        let reach = reach + 2.0;
        min_x = min_x.min(center.x - reach);
        min_y = min_y.min(center.y - reach);
        max_x = max_x.max(center.x + reach);
        max_y = max_y.max(center.y + reach);
    }
    if min_x > max_x {
        return Rect {
            x: 0.0,
            y: 0.0,
            width: 0.0,
            height: 0.0,
        };
    }
    Rect {
        x: min_x,
        y: min_y,
        width: max_x - min_x,
        height: max_y - min_y,
    }
}

/// One retained stretch of a command's recording, addressed by the retained
/// snapshot's tape range. The `id` is stable for the segment's lifetime —
/// renderer-side retained slots key on it, and it survives other segments
/// dying.
#[derive(Clone, Debug, PartialEq)]
pub struct CommandSegment {
    /// The capture identity this segment's content lives under: renderer
    /// retained slots key on the (command, slot) pair. Slot ids are
    /// allocated at partition, whose emission carries the capture content;
    /// split pieces inherit the parent's slot and address into it, so a
    /// split never needs a recapture.
    pub slot: u32,
    /// This segment's first record within the slot's captured content.
    pub slot_offset: usize,
    pub tape_start: usize,
    pub tape_end: usize,
    /// Loose logical bounds at capture.
    pub bounds: Rect,
    /// Span-relative record offsets (ascending) this segment's span
    /// recolored on the PREVIOUS verified frame. The renderer's slot paint
    /// is a patched mirror, never rebuilt, so a record whose color returns
    /// EXACTLY to its capture value needs an explicit restore patch — pure
    /// diff-vs-snapshot emission would leave the mirror stale forever
    /// (see `merge_color_restores`).
    pub prev_recolors: Vec<u32>,
}

/// One span of this frame's recording, in tape order.
#[derive(Clone, Debug, PartialEq)]
pub enum ReplaySpan {
    /// The retained segment moved by `transform`; `recolors` are
    /// (span-relative record offset, new color) patches — including
    /// explicit restores to the capture color for records recolored on the
    /// previous frame and clean again on this one, because the renderer's
    /// slot paint is a patched mirror that never resets on its own.
    Retained {
        /// The capture identity ([`CommandSegment::slot`]).
        slot: u32,
        /// True only on partition frames, where the snapshot IS the current
        /// frame: this span's records are the slot's capture content and
        /// `transform` is identity. Every later frame's transform is motion
        /// since exactly that content — never double-applied.
        capture: bool,
        /// The span's first record within the slot's captured content.
        slot_offset: usize,
        /// Where the span sits in the CURRENT frame's tape.
        tape_start: usize,
        tape_end: usize,
        transform: RecordTransform,
        recolors: Vec<(u32, Color)>,
        /// Segment capture bounds under this frame's transform.
        bounds: Rect,
    },
    /// Materialize these current-tape entries through the ordinary path.
    Dynamic { tape_start: usize, tape_end: usize },
}

/// What one frame of verification decided for a command.
#[derive(Debug, PartialEq)]
pub enum ReplayOutcome {
    /// No retention this frame: materialize the whole recording.
    AllDynamic,
    /// The interleaved retained/dynamic structure of this frame, in exact
    /// tape order.
    Spans(Vec<ReplaySpan>),
}

/// Fans independent verification bodies across worker threads. `run(i)` is
/// called exactly once for every `i in 0..jobs`, from any thread; the call
/// returns only after every job finished (jobs borrow the caller's stack).
/// The renderer wires its frame worker pool in through this seam so the
/// recorder crate stays free of threading machinery.
pub trait VerifyExecutor: Sync {
    fn for_each(&self, jobs: usize, run: &(dyn Fn(usize) + Sync));
}

/// A command's replay verdict translated into the space its consumers see:
/// spans address the run's materialized primitive vector, not the record
/// tape. This is what rides the render graph next to the primitives.
#[derive(Clone, Debug)]
pub struct CommandReplayFrame {
    /// The similarity pivot every span transform rotates and scales about.
    pub center: Point,
    /// Interleaved retained/dynamic structure in exact z order.
    pub spans: Vec<FrameSpan>,
    /// The frame-owned rematerialization source: the exact recording this
    /// frame's spans address, pinned for the frame's lifetime. A bypassed
    /// span (empty primitive range) that cannot draw retained materializes
    /// its `tape_range` from HERE — never from a sweepable ambient registry,
    /// whose contents may have moved on by render time. `None` only before
    /// the recording is published (the builder attaches the published
    /// handle) or on hand-built frames with nothing bypassed. Shared, not
    /// cloned: the handle pins the recording buffers; the depth-one frame
    /// packet will carry this same handle as an `Arc` when the graph goes
    /// `Send`.
    pub fallback: Option<std::rc::Rc<crate::geometry::CommandRecording>>,
}

impl PartialEq for CommandReplayFrame {
    fn eq(&self, other: &Self) -> bool {
        self.center == other.center
            && self.spans == other.spans
            && match (&self.fallback, &other.fallback) {
                (None, None) => true,
                (Some(a), Some(b)) => std::rc::Rc::ptr_eq(a, b),
                _ => false,
            }
    }
}

/// One primitive-space span of a [`CommandReplayFrame`].
#[derive(Clone, Debug, PartialEq)]
pub enum FrameSpan {
    Retained {
        /// The capture identity; renderer retained slots key on the
        /// (command, slot) pair.
        slot: u32,
        /// True only when `range` holds the slot's full capture content
        /// (partition frames, transform identity): retain it under the
        /// slot's identity.
        capture: bool,
        /// The span's first primitive within the slot's captured content.
        slot_offset: u32,
        /// The span's primitives in the run's primitive vector. EMPTY when
        /// the span was bypassed — its records were never materialized and
        /// the renderer draws it from the retained slot, or asks the
        /// recorder to materialize `tape_range` on demand when it cannot.
        range: (u32, u32),
        /// The span's records in the command's recording tape, for
        /// emergency rematerialization of a bypassed span.
        tape_range: (u32, u32),
        transform: RecordTransform,
        /// (span-relative primitive offset, new solid color) patches.
        recolors: Vec<(u32, Color)>,
        /// Capture bounds under this frame's transform.
        bounds: Rect,
    },
    Dynamic {
        /// Ordinary primitives in the run's primitive vector.
        range: (u32, u32),
    },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum CommandReplayPhase {
    Idle,
    Snapshotted,
    Captured,
}

/// A pooled span job's result: the cleanly matched prefix length and the
/// recolors within it. One slot per segment, reused across frames — see
/// [`CommandReplayState::verify_results`].
#[derive(Debug, Default)]
struct SpanResultSlot {
    matched: usize,
    recolors: Vec<(u32, Color)>,
}

/// Per-command replay state: the retained snapshot (previous stable form of
/// the recording) and the segments carved out of it. This is the double
/// buffer sol's plan sanctions — previous and current forms coexist only
/// for comparison.
#[derive(Debug)]
pub struct CommandReplayState {
    phase: CommandReplayPhase,
    center: Point,
    snapshot: CommandRecording,
    segments: Vec<CommandSegment>,
    next_slot_id: u32,
    lifetime_deaths: u64,
    lifetime_splits: u64,
    capture_coverage: f32,
    frames_since_capture: u32,
    optimistic_commits: u64,
    prefix_commits: u64,
    verify_results: Vec<std::sync::Mutex<SpanResultSlot>>,
    recolor_scratch: Vec<(u32, Color)>,
    best_recolor_scratch: Vec<(u32, Color)>,
    verify_pending: std::collections::VecDeque<CommandSegment>,
    verify_survivors: Vec<CommandSegment>,
    align_scratch: Vec<Option<usize>>,
    collapsed_from_captured: bool,
}

impl Default for CommandReplayState {
    fn default() -> Self {
        Self {
            phase: CommandReplayPhase::Idle,
            center: Point::new(0.0, 0.0),
            snapshot: CommandRecording::default(),
            segments: Vec::new(),
            next_slot_id: 0,
            lifetime_deaths: 0,
            lifetime_splits: 0,
            capture_coverage: 0.0,
            frames_since_capture: 0,
            optimistic_commits: 0,
            prefix_commits: 0,
            verify_results: Vec::new(),
            recolor_scratch: Vec::new(),
            best_recolor_scratch: Vec::new(),
            verify_pending: std::collections::VecDeque::new(),
            verify_survivors: Vec::new(),
            align_scratch: Vec::new(),
            collapsed_from_captured: false,
        }
    }
}

impl CommandReplayState {
    pub fn segments(&self) -> &[CommandSegment] {
        &self.segments
    }

    /// Lifetime (deaths, splits) across every verified frame — diagnostics
    /// for judging how churn interacts with retention.
    pub fn stats(&self) -> (u64, u64) {
        (self.lifetime_deaths, self.lifetime_splits)
    }

    /// Frames the pooled fast path fully committed (0 without an executor).
    pub fn optimistic_commits(&self) -> u64 {
        self.optimistic_commits
    }

    /// Frames where the pooled pass committed a non-empty strict prefix of
    /// the segments before handing the serial walk the failure point
    /// (0 without an executor).
    pub fn prefix_commits(&self) -> u64 {
        self.prefix_commits
    }

    /// The similarity pivot all span transforms rotate and scale about.
    pub fn center(&self) -> Point {
        self.center
    }

    /// Whether the last [`Self::advance_pooled`] collapsed out of an
    /// established capture (the `Captured`-phase coverage collapse) — the
    /// expensive full-rematerialization frame the stale-transition serve
    /// can replace with the previous frame's emission. False on every
    /// bootstrap `AllDynamic` frame: an idle snapshot, a short tape, or a
    /// retirement never had a capture to collapse out of.
    pub fn collapsed_from_captured(&self) -> bool {
        self.collapsed_from_captured
    }

    /// Advances the state machine with this frame's recording and returns
    /// what the frame can retain. Phases mirror the flat-list detector:
    /// snapshot on the first sighting, partition into
    /// transform-consistent chains on the second, verify per entry from the
    /// third on. A structural collapse or coverage erosion re-snapshots;
    /// correctness never depends on the detector being right about
    /// stability — a wrong guess costs a frame of ordinary rendering.
    pub fn advance(&mut self, current: &CommandRecording) -> ReplayOutcome {
        self.advance_pooled(current, None)
    }

    /// [`Self::advance`] with an optional executor that verification fans
    /// its per-segment span matching across. Anchors are located in a
    /// serial phase that uses the exact candidate order of the serial walk;
    /// only the span bodies fan out. A frame where every body matches whole
    /// commits without touching the serial walk; any other frame commits
    /// the segments strictly before the first failure — equal by
    /// construction to what the serial walk produces for them — and runs
    /// the serial split/death/re-snapshot machinery from the failure point
    /// on. The outcome is identical with and without an executor.
    pub fn advance_pooled(
        &mut self,
        current: &CommandRecording,
        pool: Option<&dyn VerifyExecutor>,
    ) -> ReplayOutcome {
        self.collapsed_from_captured = false;
        if current.tape.len() < MIN_REPLAY_COMMAND_RECORDS {
            self.retire();
            return ReplayOutcome::AllDynamic;
        }
        let Some(center) = detect_center(current) else {
            self.retire();
            return ReplayOutcome::AllDynamic;
        };
        match self.phase {
            CommandReplayPhase::Idle => {
                self.take_snapshot(current, center);
                ReplayOutcome::AllDynamic
            }
            CommandReplayPhase::Snapshotted => self.partition(current, center),
            CommandReplayPhase::Captured => self.verify(current, pool),
        }
    }

    fn retire(&mut self) {
        self.phase = CommandReplayPhase::Idle;
        self.snapshot = CommandRecording::default();
        self.segments.clear();
    }

    fn take_snapshot(&mut self, current: &CommandRecording, center: Point) {
        self.snapshot.clone_records_from(current);
        self.center = center;
        self.segments.clear();
        self.phase = CommandReplayPhase::Snapshotted;
    }

    fn partition(&mut self, current: &CommandRecording, center: Point) -> ReplayOutcome {
        align_recordings(current, &self.snapshot, &mut self.align_scratch);
        let mut chains: Vec<(usize, usize)> = Vec::new();
        let mut i = 0;
        while i < current.tape.len() {
            let (Some(view), Some(snapshot_view)) = (
                view_at(current, i),
                self.align_scratch[i].and_then(|j| view_at(&self.snapshot, j)),
            ) else {
                i += 1;
                continue;
            };
            let Some((t, true)) =
                pair_transform(current, view, &self.snapshot, snapshot_view, self.center)
            else {
                i += 1;
                continue;
            };
            if match_pair(current, view, &self.snapshot, snapshot_view, self.center, t)
                == RecordMatch::Mismatch
            {
                i += 1;
                continue;
            }
            let start = i;
            let mut end = i + 1;
            while end < current.tape.len() {
                let (Some(view), Some(snapshot_view)) = (
                    view_at(current, end),
                    self.align_scratch[end].and_then(|j| view_at(&self.snapshot, j)),
                ) else {
                    break;
                };
                let Some((entry_t, pinned)) =
                    pair_transform(current, view, &self.snapshot, snapshot_view, self.center)
                else {
                    break;
                };
                if !transforms_group(entry_t, pinned, t) {
                    break;
                }
                if match_pair(current, view, &self.snapshot, snapshot_view, self.center, t)
                    == RecordMatch::Mismatch
                {
                    break;
                }
                end += 1;
            }
            if end - start >= MIN_SEGMENT_RECORDS {
                let mut piece_start = start;
                while piece_start < end {
                    let piece_end = (piece_start + MAX_SEGMENT_RECORDS).min(end);
                    if piece_end - piece_start >= MIN_SEGMENT_RECORDS {
                        chains.push((piece_start, piece_end));
                    }
                    piece_start = piece_end;
                }
            }
            i = end.max(i + 1);
        }

        if chains.is_empty() {
            self.take_snapshot(current, center);
            return ReplayOutcome::AllDynamic;
        }
        self.take_snapshot(current, center);
        self.segments = chains
            .into_iter()
            .map(|range| {
                let slot = self.next_slot_id;
                self.next_slot_id += 1;
                CommandSegment {
                    slot,
                    slot_offset: 0,
                    tape_start: range.0,
                    tape_end: range.1,
                    bounds: range_bounds(&self.snapshot, range),
                    prev_recolors: Vec::new(),
                }
            })
            .collect();
        let covered: usize = self
            .segments
            .iter()
            .map(|segment| segment.tape_end - segment.tape_start)
            .sum();
        self.capture_coverage = covered as f32 / current.tape.len().max(1) as f32;
        self.frames_since_capture = 0;
        self.phase = CommandReplayPhase::Captured;

        let mut spans: Vec<ReplaySpan> = Vec::with_capacity(self.segments.len() * 2 + 1);
        let mut cursor = 0usize;
        for segment in &self.segments {
            if segment.tape_start > cursor {
                spans.push(ReplaySpan::Dynamic {
                    tape_start: cursor,
                    tape_end: segment.tape_start,
                });
            }
            spans.push(ReplaySpan::Retained {
                slot: segment.slot,
                capture: true,
                slot_offset: 0,
                tape_start: segment.tape_start,
                tape_end: segment.tape_end,
                transform: RecordTransform::IDENTITY,
                recolors: Vec::new(),
                bounds: segment.bounds,
            });
            cursor = segment.tape_end;
        }
        if cursor < current.tape.len() {
            spans.push(ReplaySpan::Dynamic {
                tape_start: cursor,
                tape_end: current.tape.len(),
            });
        }
        ReplayOutcome::Spans(spans)
    }

    fn verify(
        &mut self,
        current: &CommandRecording,
        pool: Option<&dyn VerifyExecutor>,
    ) -> ReplayOutcome {
        let mut spans: Vec<ReplaySpan> = Vec::new();
        let mut retained_records = 0usize;
        let mut cursor = 0usize;
        let mut committed = 0usize;
        if let Some(pool) = pool
            && self.segments.len() >= 2
        {
            let commit = self.verify_optimistic(current, pool);
            if commit.committed == self.segments.len() {
                self.optimistic_commits += 1;
                return self.finish_verify(current, commit.spans, commit.retained_records);
            }
            if commit.committed > 0 {
                self.prefix_commits += 1;
            }
            spans = commit.spans;
            retained_records = commit.retained_records;
            cursor = commit.cursor;
            committed = commit.committed;
        }
        self.verify_pending.clear();
        self.verify_pending.extend(self.segments.drain(committed..));
        self.verify_survivors.clear();
        self.verify_survivors.append(&mut self.segments);
        while let Some(segment) = self.verify_pending.pop_front() {
            let len = segment.tape_end - segment.tape_start;
            let search_end = (cursor + RESYNC_WINDOW)
                .min(current.tape.len().saturating_sub(len - 1))
                .max(cursor);
            let candidates = cursor..search_end;
            let mut located: Option<(usize, RecordTransform)> = None;
            let mut best_prefix: Option<(usize, RecordTransform)> = None;
            let mut best_prefix_len = 0usize;
            let mut attempts = 0usize;
            'search: for start in candidates {
                let Some(t) = probe_anchor(
                    current,
                    &self.snapshot,
                    self.center,
                    segment.tape_start,
                    len,
                    start,
                ) else {
                    continue;
                };
                let matched = match_span(
                    TypedRecords::from(current),
                    TypedRecords::from(&self.snapshot),
                    self.center,
                    start,
                    segment.tape_start,
                    len,
                    t,
                    &mut self.recolor_scratch,
                );
                if matched < len {
                    if matched > best_prefix_len {
                        best_prefix_len = matched;
                        best_prefix = Some((start, t));
                        std::mem::swap(&mut self.recolor_scratch, &mut self.best_recolor_scratch);
                    }
                    if matched >= MIN_SEGMENT_RECORDS {
                        attempts += 1;
                        if attempts >= MAX_COMMIT_ATTEMPTS {
                            break 'search;
                        }
                    }
                    continue;
                }
                located = Some((start, t));
                break;
            }
            let (span_start, t, mut recolors, span_len) = match located {
                Some((start, t)) => (start, t, std::mem::take(&mut self.recolor_scratch), len),
                None => {
                    let split = best_prefix_len >= MIN_SEGMENT_RECORDS;
                    let Some((start, t)) = best_prefix.filter(|_| split) else {
                        self.lifetime_deaths += 1;
                        continue;
                    };
                    let suffix_start = segment.tape_start + best_prefix_len + 1;
                    if segment.tape_end > suffix_start
                        && segment.tape_end - suffix_start >= MIN_SEGMENT_RECORDS
                    {
                        let rebase = (best_prefix_len + 1) as u32;
                        let cut = segment.prev_recolors.partition_point(|&p| p < rebase);
                        self.verify_pending.push_front(CommandSegment {
                            slot: segment.slot,
                            slot_offset: segment.slot_offset + (suffix_start - segment.tape_start),
                            tape_start: suffix_start,
                            tape_end: segment.tape_end,
                            bounds: range_bounds(&self.snapshot, (suffix_start, segment.tape_end)),
                            prev_recolors: segment.prev_recolors[cut..]
                                .iter()
                                .map(|&p| p - rebase)
                                .collect(),
                        });
                    }
                    self.lifetime_splits += 1;
                    (
                        start,
                        t,
                        std::mem::take(&mut self.best_recolor_scratch),
                        best_prefix_len,
                    )
                }
            };
            let mut survivor = if span_len == len {
                segment
            } else {
                CommandSegment {
                    slot: segment.slot,
                    slot_offset: segment.slot_offset,
                    tape_start: segment.tape_start,
                    tape_end: segment.tape_start + span_len,
                    bounds: range_bounds(
                        &self.snapshot,
                        (segment.tape_start, segment.tape_start + span_len),
                    ),
                    prev_recolors: segment.prev_recolors,
                }
            };
            merge_color_restores(
                &self.snapshot,
                survivor.tape_start,
                span_len,
                &mut survivor.prev_recolors,
                &mut recolors,
            );
            if span_start > cursor {
                spans.push(ReplaySpan::Dynamic {
                    tape_start: cursor,
                    tape_end: span_start,
                });
            }
            retained_records += span_len;
            spans.push(ReplaySpan::Retained {
                slot: survivor.slot,
                capture: false,
                slot_offset: survivor.slot_offset,
                tape_start: span_start,
                tape_end: span_start + span_len,
                transform: t,
                recolors,
                bounds: t.apply_to_bounds(self.center, survivor.bounds),
            });
            cursor = span_start + span_len;
            self.verify_survivors.push(survivor);
        }
        if cursor < current.tape.len() {
            spans.push(ReplaySpan::Dynamic {
                tape_start: cursor,
                tape_end: current.tape.len(),
            });
        }

        std::mem::swap(&mut self.segments, &mut self.verify_survivors);
        self.finish_verify(current, spans, retained_records)
    }

    fn finish_verify(
        &mut self,
        current: &CommandRecording,
        spans: Vec<ReplaySpan>,
        retained_records: usize,
    ) -> ReplayOutcome {
        self.frames_since_capture += 1;
        let retained_total: usize = self
            .segments
            .iter()
            .map(|segment| segment.tape_end - segment.tape_start)
            .sum();
        let coverage = retained_total as f32 / current.tape.len().max(1) as f32;
        let collapsed = retained_records == 0 || coverage < MIN_COVERAGE_FRACTION;
        let eroded = coverage + RECAPTURE_EROSION < self.capture_coverage
            && self.frames_since_capture >= RECAPTURE_COOLDOWN_FRAMES;
        self.collapsed_from_captured = collapsed;
        if collapsed || eroded {
            let center = self.center;
            self.take_snapshot(current, center);
            if retained_records == 0 {
                return ReplayOutcome::AllDynamic;
            }
        }
        ReplayOutcome::Spans(spans)
    }

    fn verify_optimistic(
        &mut self,
        current: &CommandRecording,
        pool: &dyn VerifyExecutor,
    ) -> PooledCommit {
        struct SpanJob {
            start: usize,
            seg_start: usize,
            len: usize,
            t: RecordTransform,
        }
        let mut jobs: Vec<SpanJob> = Vec::with_capacity(self.segments.len());
        let mut cursor = 0usize;
        for segment in &self.segments {
            let len = segment.tape_end - segment.tape_start;
            let search_end = (cursor + RESYNC_WINDOW)
                .min(current.tape.len().saturating_sub(len - 1))
                .max(cursor);
            let mut found = None;
            for start in cursor..search_end {
                if let Some(t) = probe_anchor(
                    current,
                    &self.snapshot,
                    self.center,
                    segment.tape_start,
                    len,
                    start,
                ) {
                    found = Some((start, t));
                    break;
                }
            }
            let Some((start, t)) = found else {
                break;
            };
            jobs.push(SpanJob {
                start,
                seg_start: segment.tape_start,
                len,
                t,
            });
            cursor = start + len;
        }
        if jobs.is_empty() {
            return PooledCommit {
                spans: Vec::new(),
                retained_records: 0,
                committed: 0,
                cursor: 0,
            };
        }
        if self.verify_results.len() < jobs.len() {
            self.verify_results
                .resize_with(jobs.len(), Default::default);
        }
        {
            let current = TypedRecords::from(current);
            let snapshot = TypedRecords::from(&self.snapshot);
            let center = self.center;
            let jobs = &jobs;
            let results = &self.verify_results;
            pool.for_each(jobs.len(), &|i| {
                let job = &jobs[i];
                let mut guard = results[i].lock().expect("verify span job lock");
                let slot = &mut *guard;
                slot.matched = match_span(
                    current,
                    snapshot,
                    center,
                    job.start,
                    job.seg_start,
                    job.len,
                    job.t,
                    &mut slot.recolors,
                );
            });
        }
        let mut committed = jobs.len();
        for (i, (job, result)) in jobs.iter().zip(&self.verify_results).enumerate() {
            if result.lock().expect("verify span job lock").matched < job.len {
                committed = i;
                break;
            }
        }
        let mut spans: Vec<ReplaySpan> = Vec::with_capacity(committed * 2 + 1);
        let mut retained_records = 0usize;
        let mut cursor = 0usize;
        for (segment, (job, result)) in self
            .segments
            .iter_mut()
            .zip(jobs.iter().zip(&self.verify_results))
            .take(committed)
        {
            let mut recolors =
                std::mem::take(&mut result.lock().expect("verify span job lock").recolors);
            merge_color_restores(
                &self.snapshot,
                segment.tape_start,
                job.len,
                &mut segment.prev_recolors,
                &mut recolors,
            );
            if job.start > cursor {
                spans.push(ReplaySpan::Dynamic {
                    tape_start: cursor,
                    tape_end: job.start,
                });
            }
            retained_records += job.len;
            spans.push(ReplaySpan::Retained {
                slot: segment.slot,
                capture: false,
                slot_offset: segment.slot_offset,
                tape_start: job.start,
                tape_end: job.start + job.len,
                transform: job.t,
                recolors,
                bounds: job.t.apply_to_bounds(self.center, segment.bounds),
            });
            cursor = job.start + job.len;
        }
        if committed == self.segments.len() && cursor < current.tape.len() {
            spans.push(ReplaySpan::Dynamic {
                tape_start: cursor,
                tape_end: current.tape.len(),
            });
        }
        PooledCommit {
            spans,
            retained_records,
            committed,
            cursor,
        }
    }
}

/// What one pooled pass committed: the emitted spans and survivor count of
/// the leading segments whose bodies matched whole, plus the current-tape
/// cursor after the last committed span — exactly the state the serial
/// walk needs to take over from the first failure. `committed` equal to
/// the segment count is a fully pooled frame; zero means the pass salvaged
/// nothing and the serial walk redoes the frame from the top.
struct PooledCommit {
    spans: Vec<ReplaySpan>,
    retained_records: usize,
    committed: usize,
    cursor: usize,
}

/// The snapshot's color for one retained record — the value the renderer's
/// slot paint was seeded with at capture (the snapshot IS the capture
/// content for as long as the segment lives; splits only re-address it).
fn snapshot_record_color(snapshot: &CommandRecording, i: usize) -> Option<Color> {
    match view_at(snapshot, i)? {
        ReplayView::Arc(index) => Some(snapshot.arcs[index].color),
        ReplayView::RoundRect(index) => Some(snapshot.round_rects[index].color),
    }
}

/// Rolls one emitted span's recolor memory a frame forward and emits the
/// restore patches the renderer needs: for every span-relative offset in
/// `prev` (last frame's recolors, ascending) that this frame's `recolors`
/// (ascending) do NOT patch, appends `(offset, snapshot color)`, then
/// replaces `prev` with this frame's patched offsets. The renderer's slot
/// paint is a patched mirror, never rebuilt, so a record whose color
/// returns EXACTLY to its capture value would otherwise keep the previous
/// frame's patch indefinitely — pure diff-vs-snapshot emission goes silent
/// on exactly that frame (the parity scene's mod-11 twinkle wrap). Restores
/// append after the ordinary patches; every patched offset is distinct and
/// patches write disjoint records, so order across the two groups cannot
/// matter. Offsets at or past `span_len` (a split's suffix share) are the
/// caller's to hand to the suffix segment.
fn merge_color_restores(
    snapshot: &CommandRecording,
    seg_tape_start: usize,
    span_len: usize,
    prev: &mut Vec<u32>,
    recolors: &mut Vec<(u32, Color)>,
) {
    let patched = recolors.len();
    let mut cursor = 0usize;
    for &offset in prev.iter() {
        if offset as usize >= span_len {
            break;
        }
        while cursor < patched && recolors[cursor].0 < offset {
            cursor += 1;
        }
        if cursor < patched && recolors[cursor].0 == offset {
            continue;
        }
        if let Some(color) = snapshot_record_color(snapshot, seg_tape_start + offset as usize) {
            recolors.push((offset, color));
        }
    }
    prev.clear();
    prev.extend(recolors[..patched].iter().map(|&(offset, _)| offset));
}

/// The cheap anchor test shared by the serial walk and the pooled fast
/// path: view compatibility, transform derivation from the anchor pair, and
/// [`ANCHOR_PROBE_RECORDS`] probe matches. `None` means this candidate
/// cannot be the segment's anchor.
fn probe_anchor(
    current: &CommandRecording,
    snapshot: &CommandRecording,
    center: Point,
    seg_start: usize,
    len: usize,
    start: usize,
) -> Option<RecordTransform> {
    let (Some(view), Some(snapshot_view)) = (view_at(current, start), view_at(snapshot, seg_start))
    else {
        return None;
    };
    if !views_compatible(current, Some(view), snapshot, Some(snapshot_view)) {
        return None;
    }
    let (t, _) = pair_transform(current, view, snapshot, snapshot_view, center)?;
    for probe in 0..ANCHOR_PROBE_RECORDS.min(len) {
        let (Some(view), Some(snapshot_view)) = (
            view_at(current, start + probe),
            view_at(snapshot, seg_start + probe),
        ) else {
            return None;
        };
        if match_pair(current, view, snapshot, snapshot_view, center, t) == RecordMatch::Mismatch {
            return None;
        }
    }
    Some(t)
}

/// The typed-record arrays a span match reads — the POD slice view of a
/// [`CommandRecording`] that is `Sync` (the recording itself is not: its
/// `others` vector may hold `Rc`-carrying primitives), which is what lets
/// [`match_span`] calls cross worker threads.
#[derive(Clone, Copy)]
struct TypedRecords<'a> {
    tape: &'a [TapeRef],
    arcs: &'a [SolidArcRecord],
    round_rects: &'a [SolidRoundRectRecord],
}

impl<'a> From<&'a CommandRecording> for TypedRecords<'a> {
    fn from(recording: &'a CommandRecording) -> Self {
        Self {
            tape: &recording.tape,
            arcs: &recording.arcs,
            round_rects: &recording.round_rects,
        }
    }
}

impl TypedRecords<'_> {
    #[cfg(test)]
    fn view_at(&self, i: usize) -> Option<ReplayView> {
        view_at_slices(self.tape, self.round_rects, i)
    }
}

/// The length of the contiguous same-store run starting at `tape[at]`: the
/// maximal `d` such that every entry in `tape[at..at + d]` is the same kind
/// with consecutive store indices. Per-store indices appear on the tape in
/// strictly increasing order (the [`CommandRecording`] tape invariant), so
/// `tape[at + e].raw() == tape[at].raw() + e` holds exactly when all `e`
/// entries after `at` are that kind — the predicate is monotone in `e`
/// (true up to the run's end, false after, and never true again: a kind
/// that leaves and returns has advanced its index by less than the tape
/// distance). That monotonicity is what lets the boundary be binary-searched
/// instead of walked: a 2048-entry single-kind span costs ~11 word compares,
/// not 2048 decodes. The compare widens to u64 so `base + e` cannot wrap
/// for probes past a large-index run.
fn typed_run_len(tape: &[TapeRef], at: usize) -> usize {
    let rest = &tape[at..];
    let base = rest[0].raw() as u64;
    let mut lo = 1usize;
    let mut hi = rest.len();
    while lo < hi {
        let mid = lo + (hi - lo) / 2;
        if rest[mid].raw() as u64 == base + mid as u64 {
            lo = mid + 1;
        } else {
            hi = mid;
        }
    }
    lo
}

/// The exact [`close_rel`] verdict for `N` independent lane pairs, folded
/// with `&`. Fixed-size arrays, a constant trip count, and a branch-free
/// fold are the SLP-friendly shape (see the section comment above); each
/// lane delegates to [`close_rel`] itself, so the per-lane float expression
/// is the scalar one verbatim.
#[inline(always)]
fn close_rel_all<const N: usize>(a: [f32; N], b: [f32; N]) -> bool {
    let mut ok = [false; N];
    for ((lane, &a), &b) in ok.iter_mut().zip(&a).zip(&b) {
        *lane = close_rel(a, b);
    }
    ok.into_iter().fold(true, |all, lane| all & lane)
}

/// The stroke comparison's lane inputs: the width pair the lane compares
/// and whether the `Option` shapes agree. Both-`None` yields `(0.0, 0.0)` —
/// a trivially true lane, exactly the scalar arm's `true` — both-`Some`
/// yields the scalar arm's exact operands, and a shape mismatch fails on
/// the flag with the lane value unused.
#[inline(always)]
fn stroke_lane(
    current: Option<crate::Stroke>,
    retained: Option<crate::Stroke>,
    scale: f32,
) -> (f32, f32, bool) {
    match (current, retained) {
        (None, None) => (0.0, 0.0, true),
        (Some(now), Some(then)) => (now.width, then.width * scale, true),
        _ => (0.0, 0.0, false),
    }
}

/// [`match_arc`]'s lane-shaped twin for the contiguous run loop: the same
/// seven `close_rel` checks plus the stroke lane, shaped as two
/// f32x4-sized groups for [`close_rel_all`], with the scalar
/// [`close_angle`] alongside. Equal by construction — every lane evaluates
/// the identical float expression on the identical values, and `&` over
/// pure booleans is order-free — and pinned by `lane_kernel_equivalence`.
#[inline(always)]
fn match_arc_lanes(
    current: &SolidArcRecord,
    retained: &SolidArcRecord,
    center: Point,
    scale: f32,
    angle: f32,
) -> RecordMatch {
    let (stroke_now, stroke_then, stroke_shape_ok) =
        stroke_lane(current.stroke, retained.stroke, scale);
    let a = [
        current.center.x,
        current.center.y,
        current.center.x,
        current.center.y,
        current.radius,
        current.inner_radius,
        current.sweep_angle,
        stroke_now,
    ];
    let b = [
        retained.center.x,
        retained.center.y,
        center.x,
        center.y,
        retained.radius * scale,
        retained.inner_radius * scale,
        retained.sweep_angle,
        stroke_then,
    ];
    let geometry_ok = close_rel_all(a, b)
        & stroke_shape_ok
        & close_angle(current.start_angle, retained.start_angle + angle);
    if !geometry_ok {
        return RecordMatch::Mismatch;
    }
    if current.color == retained.color {
        RecordMatch::Exact
    } else {
        RecordMatch::Recolor
    }
}

/// [`match_round_rect`]'s lane-shaped twin. Both sides' [`circle_view`]
/// derivations run unconditionally — centers and halves are plain
/// arithmetic on any input, and a non-circle fails its `is_circle` lanes
/// below, the same Mismatch the scalar `let ... else` takes, decided
/// without a branch. The caller supplies the transform's parts (`sin_cos`
/// hoisted per run — see [`apply_parts`]). Fourteen `close_rel` lanes:
/// three clean f32x4 quads (current radii vs half, retained radii vs half,
/// then extents and the moved center) and a two-lane tail (diameter,
/// stroke).
#[inline(always)]
fn match_round_rect_lanes(
    current: &SolidRoundRectRecord,
    retained: &SolidRoundRectRecord,
    center: Point,
    scale: f32,
    sin: f32,
    cos: f32,
) -> RecordMatch {
    let half_now = current.rect.width * 0.5;
    let half_then = retained.rect.width * 0.5;
    let c_now = Point::new(
        current.rect.x + current.rect.width * 0.5,
        current.rect.y + current.rect.height * 0.5,
    );
    let c_then = Point::new(
        retained.rect.x + retained.rect.width * 0.5,
        retained.rect.y + retained.rect.height * 0.5,
    );
    let moved = apply_parts(scale, sin, cos, center, c_then);
    let (stroke_now, stroke_then, stroke_shape_ok) =
        stroke_lane(current.stroke, retained.stroke, scale);
    let a = [
        current.radii.top_left,
        current.radii.top_right,
        current.radii.bottom_right,
        current.radii.bottom_left,
        retained.radii.top_left,
        retained.radii.top_right,
        retained.radii.bottom_right,
        retained.radii.bottom_left,
        current.rect.width,
        retained.rect.width,
        c_now.x,
        c_now.y,
        current.rect.width,
        stroke_now,
    ];
    let b = [
        half_now,
        half_now,
        half_now,
        half_now,
        half_then,
        half_then,
        half_then,
        half_then,
        current.rect.height,
        retained.rect.height,
        moved.x,
        moved.y,
        retained.rect.width * scale,
        stroke_then,
    ];
    let geometry_ok = close_rel_all(a, b) & stroke_shape_ok;
    if !geometry_ok {
        return RecordMatch::Mismatch;
    }
    if current.color == retained.color {
        RecordMatch::Exact
    } else {
        RecordMatch::Recolor
    }
}

/// The tight arc loop over one contiguous run pair: no tape decode, no kind
/// dispatch — direct slice indexing with [`match_arc_lanes`],
/// [`match_arc`]'s equivalence-pinned lane-shaped twin, the transform's
/// parts hoisted once per run. Returns the cleanly matched length; recolors
/// are pushed as (`span_offset` + run-relative index, color), exactly the
/// entries the per-entry walk would have produced.
fn match_arc_run(
    current: &[SolidArcRecord],
    snapshot: &[SolidArcRecord],
    center: Point,
    t: RecordTransform,
    span_offset: usize,
    recolors: &mut Vec<(u32, Color)>,
) -> usize {
    let (scale, angle) = (t.scale, t.angle);
    for (i, (now, then)) in current.iter().zip(snapshot).enumerate() {
        match match_arc_lanes(now, then, center, scale, angle) {
            RecordMatch::Exact => {}
            RecordMatch::Recolor => recolors.push(((span_offset + i) as u32, now.color)),
            RecordMatch::Mismatch => return i,
        }
    }
    current.len()
}

/// [`match_arc_run`]'s round-rect twin, on [`match_round_rect_lanes`]. The
/// lane kernel rejects non-circular round rects through its `is_circle`
/// lanes, which is the same verdict the per-entry walk's eligibility check
/// produced for them (`view_at` maps a non-circle to `None`, and any `None`
/// pairing is a mismatch), so no separate eligibility pass is needed here.
/// The rotation's `sin_cos` — a libm call the scalar path pays per record
/// inside [`RecordTransform::apply`] — is hoisted to once per run.
fn match_round_rect_run(
    current: &[SolidRoundRectRecord],
    snapshot: &[SolidRoundRectRecord],
    center: Point,
    t: RecordTransform,
    span_offset: usize,
    recolors: &mut Vec<(u32, Color)>,
) -> usize {
    let scale = t.scale;
    let (sin, cos) = t.angle.sin_cos();
    for (i, (now, then)) in current.iter().zip(snapshot).enumerate() {
        match match_round_rect_lanes(now, then, center, scale, sin, cos) {
            RecordMatch::Exact => {}
            RecordMatch::Recolor => recolors.push(((span_offset + i) as u32, now.color)),
            RecordMatch::Mismatch => return i,
        }
    }
    current.len()
}

/// The full-span commit body: matches `len` records of `current` from
/// `start` against the snapshot span at `seg_start` under `t`. Fills
/// `recolors` (cleared at entry) with the recolors inside the cleanly
/// matched prefix and returns that prefix's length — the out-param lets
/// callers own reusable buffers instead of allocating per call. It operates
/// on the typed slices so one call per segment can run on a worker thread.
///
/// Instead of decoding every tape entry, the span decomposes into
/// contiguous per-store runs (see [`typed_run_len`]): both sides' tape
/// ranges are cut at kind transitions, each joint stretch where both sides
/// stay in one store is matched by a tight per-kind loop over the store
/// slices, and any stretch that is not arc-vs-arc or round-rect-vs-
/// round-rect mismatches at its first record — exactly the verdict the
/// per-entry dispatch gave every pairing involving a rect, an `Other`, or
/// mixed kinds. Kind transitions are rare in real tapes (a ring is one long
/// arc run), so the per-entry decode cost collapses to a few binary
/// searches per span.
#[allow(clippy::too_many_arguments)]
fn match_span(
    current: TypedRecords<'_>,
    snapshot: TypedRecords<'_>,
    center: Point,
    start: usize,
    seg_start: usize,
    len: usize,
    t: RecordTransform,
    recolors: &mut Vec<(u32, Color)>,
) -> usize {
    recolors.clear();
    let current_tape = &current.tape[start..start + len];
    let snapshot_tape = &snapshot.tape[seg_start..seg_start + len];
    let mut offset = 0usize;
    while offset < len {
        let current_ref = current_tape[offset];
        let snapshot_ref = snapshot_tape[offset];
        let run = typed_run_len(current_tape, offset).min(typed_run_len(snapshot_tape, offset));
        let matched = match (current_ref.kind(), snapshot_ref.kind()) {
            (RecordKind::SolidArc, RecordKind::SolidArc) => {
                let (a, b) = (current_ref.index(), snapshot_ref.index());
                match_arc_run(
                    &current.arcs[a..a + run],
                    &snapshot.arcs[b..b + run],
                    center,
                    t,
                    offset,
                    recolors,
                )
            }
            (RecordKind::SolidRoundRect, RecordKind::SolidRoundRect) => {
                let (a, b) = (current_ref.index(), snapshot_ref.index());
                match_round_rect_run(
                    &current.round_rects[a..a + run],
                    &snapshot.round_rects[b..b + run],
                    center,
                    t,
                    offset,
                    recolors,
                )
            }
            _ => 0,
        };
        offset += matched;
        if matched < run {
            return offset;
        }
    }
    len
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
        assert_eq!(
            match_arc(&current, &retained, CENTER, t),
            RecordMatch::Exact
        );

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
        let (derived, pinned) = circle_anchor_transform_pinned(
            circle_view(&current).unwrap(),
            (c_then, d_then),
            CENTER,
        )
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
        retained.rect.width = 14.0;
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

    use crate::{
        Brush, DrawScope as _,
        geometry::{DrawScopeDefault, Size},
    };

    fn ring_frame(rings: usize, per_ring: usize, frame: usize, tail: usize) -> CommandRecording {
        let mut scope = DrawScopeDefault::new(Size::new(408.0, 408.0));
        let scale = 0.9994f32.powi(frame as i32);
        for ring in 0..rings {
            let step = 0.01 + ring as f32 * 0.005;
            let rotation = step * frame as f32;
            let radius = (60.0 + ring as f32 * 30.0) * scale;
            for slot in 0..per_ring {
                let start = slot as f32 * (std::f32::consts::TAU / per_ring as f32) + rotation;
                scope.draw_annular_sector(
                    Brush::solid(Color::WHITE),
                    CENTER,
                    radius * 0.8,
                    radius,
                    start,
                    0.02,
                );
            }
        }
        for i in 0..tail {
            let x = 40.0 + (frame * 17 + i * 31) as f32 % 300.0;
            scope.draw_circle(Brush::solid(Color::RED), Point::new(x, 50.0), 3.0);
        }
        scope.recorded().clone()
    }

    #[test]
    fn ring_scene_reaches_retention_by_the_third_frame() {
        let mut state = CommandReplayState::default();
        assert!(matches!(
            state.advance(&ring_frame(3, 300, 0, 10)),
            ReplayOutcome::AllDynamic
        ));
        let ReplayOutcome::Spans(capture_spans) = state.advance(&ring_frame(3, 300, 1, 10)) else {
            panic!("partition frame should emit the capture");
        };
        assert!(capture_spans.iter().all(|span| match span {
            ReplaySpan::Retained {
                capture, transform, ..
            } => *capture && *transform == RecordTransform::IDENTITY,
            ReplaySpan::Dynamic { .. } => true,
        }));
        assert!(!state.segments().is_empty(), "partition found the rings");

        let ReplayOutcome::Spans(spans) = state.advance(&ring_frame(3, 300, 2, 10)) else {
            panic!("third frame should retain");
        };
        let retained: usize = spans
            .iter()
            .filter(|span| matches!(span, ReplaySpan::Retained { .. }))
            .count();
        assert!(retained >= 3, "each ring retains, got {spans:?}");
        assert!(
            spans
                .iter()
                .any(|span| matches!(span, ReplaySpan::Dynamic { .. }))
        );
        let transforms: Vec<RecordTransform> = spans
            .iter()
            .filter_map(|span| match span {
                ReplaySpan::Retained { transform, .. } => Some(*transform),
                _ => None,
            })
            .collect();
        assert!(transforms.windows(2).any(|w| w[0].angle != w[1].angle));
    }

    fn flipped_ring_frame(frame: usize) -> CommandRecording {
        let mut scope = DrawScopeDefault::new(Size::new(408.0, 408.0));
        for ring in 0..4 {
            let rotation = (0.02 + ring as f32 * 0.007) * frame as f32;
            let radius = 75.0 + ring as f32 * 27.0;
            for slot in 0..260 {
                let start = slot as f32 * (std::f32::consts::TAU / 260.0) + rotation;
                scope.draw_annular_sector(
                    Brush::solid(Color::WHITE),
                    CENTER,
                    radius * 0.75,
                    radius,
                    start,
                    0.015,
                );
            }
        }
        scope.recorded().clone()
    }

    #[test]
    fn only_a_collapse_out_of_capture_sets_the_transition_flag() {
        let mut state = CommandReplayState::default();
        assert!(matches!(
            state.advance(&ring_frame(3, 300, 0, 10)),
            ReplayOutcome::AllDynamic
        ));
        assert!(!state.collapsed_from_captured());
        assert!(matches!(
            state.advance(&ring_frame(3, 300, 1, 10)),
            ReplayOutcome::Spans(_)
        ));
        assert!(!state.collapsed_from_captured());
        assert!(matches!(
            state.advance(&ring_frame(3, 300, 2, 10)),
            ReplayOutcome::Spans(_)
        ));
        assert!(!state.collapsed_from_captured());
        assert!(matches!(
            state.advance(&flipped_ring_frame(3)),
            ReplayOutcome::AllDynamic
        ));
        assert!(state.collapsed_from_captured());
        let _ = state.advance(&flipped_ring_frame(4));
        assert!(!state.collapsed_from_captured());
        let _ = state.advance(&flipped_ring_frame(5));
        let short = ring_frame(1, 40, 0, 0);
        assert!(short.len() < MIN_REPLAY_COMMAND_RECORDS);
        assert!(matches!(state.advance(&short), ReplayOutcome::AllDynamic));
        assert!(!state.collapsed_from_captured());
    }

    #[test]
    fn entity_churn_between_frames_still_retains_rings() {
        let mut state = CommandReplayState::default();
        state.advance(&ring_frame(2, 400, 0, 8));
        state.advance(&ring_frame(2, 400, 1, 13));
        let ReplayOutcome::Spans(spans) = state.advance(&ring_frame(2, 400, 2, 5)) else {
            panic!("churned tail must not break ring retention");
        };
        let retained_records: usize = spans
            .iter()
            .filter_map(|span| match span {
                ReplaySpan::Retained { .. } => Some(1),
                _ => None,
            })
            .sum();
        assert!(retained_records >= 2);
    }

    #[test]
    fn recolors_are_patches_not_mismatches() {
        let recolored_frame = |frame: usize| {
            let mut recording = ring_frame(1, 600, frame, 0);
            for i in (0..recording.arcs.len()).step_by(15) {
                recording.arcs[i].color = if frame.is_multiple_of(2) {
                    Color::rgb(1.0, 0.5, 0.1)
                } else {
                    Color::rgb(0.1, 0.5, 1.0)
                };
            }
            recording
        };
        let mut state = CommandReplayState::default();
        state.advance(&recolored_frame(0));
        state.advance(&recolored_frame(1));
        let ReplayOutcome::Spans(spans) = state.advance(&recolored_frame(2)) else {
            panic!("twinkles must not break retention");
        };
        let recolor_count: usize = spans
            .iter()
            .filter_map(|span| match span {
                ReplaySpan::Retained { recolors, .. } => Some(recolors.len()),
                _ => None,
            })
            .sum();
        assert!(recolor_count >= 30, "twinkles surface as patches");
    }

    #[test]
    fn geometry_change_kills_only_its_segment() {
        let mut state = CommandReplayState::default();
        state.advance(&ring_frame(3, 300, 0, 0));
        state.advance(&ring_frame(3, 300, 1, 0));
        let mut broken = ring_frame(3, 300, 2, 0);
        broken.arcs[450].sweep_angle *= 3.0;
        let ReplayOutcome::Spans(spans) = state.advance(&broken) else {
            panic!("one changed entry must not drop the whole command");
        };
        let retained: usize = spans
            .iter()
            .filter(|span| matches!(span, ReplaySpan::Retained { .. }))
            .count();
        assert!(
            retained >= 2,
            "the untouched rings keep retaining, got {spans:?}"
        );
    }

    #[test]
    fn mid_segment_change_splits_and_retains_both_halves() {
        let mut state = CommandReplayState::default();
        state.advance(&ring_frame(1, 900, 0, 0));
        state.advance(&ring_frame(1, 900, 1, 0));
        assert_eq!(state.segments().len(), 1, "one ring is one segment");
        let mut broken = ring_frame(1, 900, 2, 0);
        broken.arcs[450].sweep_angle *= 3.0;
        let ReplayOutcome::Spans(spans) = state.advance(&broken) else {
            panic!("a single changed record must not drop retention");
        };
        let dynamic: usize = spans
            .iter()
            .filter_map(|span| match span {
                ReplaySpan::Dynamic {
                    tape_start,
                    tape_end,
                } => Some(tape_end - tape_start),
                _ => None,
            })
            .sum();
        let retained: Vec<(u32, usize, bool)> = spans
            .iter()
            .filter_map(|span| match span {
                ReplaySpan::Retained {
                    slot,
                    slot_offset,
                    capture,
                    ..
                } => Some((*slot, *slot_offset, *capture)),
                _ => None,
            })
            .collect();
        assert_eq!(
            retained.len(),
            2,
            "prefix and suffix both retain: {spans:?}"
        );
        assert_eq!(retained[0].0, retained[1].0);
        assert_eq!(retained[0].1, 0);
        assert_eq!(retained[1].1, 451);
        assert!(retained.iter().all(|(_, _, capture)| !capture));
        assert_eq!(dynamic, 1, "only the changed record goes dynamic");
        assert_eq!(state.stats(), (0, 1), "one split, no deaths");

        let ReplayOutcome::Spans(spans) = state.advance(&ring_frame(1, 900, 3, 0)) else {
            panic!("split pieces must keep retaining");
        };
        let retained = spans
            .iter()
            .filter(|span| matches!(span, ReplaySpan::Retained { .. }))
            .count();
        assert_eq!(retained, 2, "both pieces relocate next frame: {spans:?}");
    }

    #[test]
    fn erosion_recaptures_dead_ranges_after_the_cooldown() {
        let mut state = CommandReplayState::default();
        state.advance(&ring_frame(3, 300, 0, 0));
        state.advance(&ring_frame(3, 300, 1, 0));
        let mutated = |frame: usize| {
            let mut recording = ring_frame(3, 300, frame, 0);
            for arc in &mut recording.arcs[300..600] {
                arc.sweep_angle *= 3.0;
            }
            recording
        };
        let dynamic_records = |outcome: &ReplayOutcome| -> usize {
            match outcome {
                ReplayOutcome::AllDynamic => usize::MAX,
                ReplayOutcome::Spans(spans) => spans
                    .iter()
                    .filter_map(|span| match span {
                        ReplaySpan::Dynamic {
                            tape_start,
                            tape_end,
                        } => Some(tape_end - tape_start),
                        _ => None,
                    })
                    .sum(),
            }
        };
        let after_death = state.advance(&mutated(2));
        let lost = dynamic_records(&after_death);
        assert!(
            (250..=400).contains(&lost),
            "the changed ring goes dynamic, got {lost}"
        );
        for frame in 3..(3 + RECAPTURE_COOLDOWN_FRAMES as usize + 4) {
            state.advance(&mutated(frame));
        }
        let recovered = state.advance(&mutated(200));
        let residue = dynamic_records(&recovered);
        assert!(
            residue < 50,
            "the recapture watches the ring's new shape, got {residue} dynamic"
        );
    }

    #[test]
    fn small_commands_are_not_watched() {
        let mut state = CommandReplayState::default();
        for frame in 0..4 {
            assert!(matches!(
                state.advance(&ring_frame(1, 40, frame, 0)),
                ReplayOutcome::AllDynamic
            ));
        }
        assert!(state.segments().is_empty());
    }

    #[allow(clippy::too_many_arguments)]
    fn match_span_reference(
        current: TypedRecords<'_>,
        snapshot: TypedRecords<'_>,
        center: Point,
        start: usize,
        seg_start: usize,
        len: usize,
        t: RecordTransform,
        recolors: &mut Vec<(u32, Color)>,
    ) -> usize {
        recolors.clear();
        for offset in 0..len {
            let entry_match = match (
                current.view_at(start + offset),
                snapshot.view_at(seg_start + offset),
            ) {
                (Some(ReplayView::Arc(i)), Some(ReplayView::Arc(j))) => {
                    match_arc(&current.arcs[i], &snapshot.arcs[j], center, t)
                }
                (Some(ReplayView::RoundRect(i)), Some(ReplayView::RoundRect(j))) => {
                    match_round_rect(&current.round_rects[i], &snapshot.round_rects[j], center, t)
                }
                _ => RecordMatch::Mismatch,
            };
            match entry_match {
                RecordMatch::Exact => {}
                RecordMatch::Recolor => {
                    let color = match current.view_at(start + offset) {
                        Some(ReplayView::Arc(a)) => current.arcs[a].color,
                        Some(ReplayView::RoundRect(r)) => current.round_rects[r].color,
                        None => unreachable!("recolor requires a view"),
                    };
                    recolors.push((offset as u32, color));
                }
                RecordMatch::Mismatch => return offset,
            }
        }
        len
    }

    fn mixed_frame(t: RecordTransform, recolored: bool) -> CommandRecording {
        let mut scope = DrawScopeDefault::new(Size::new(408.0, 408.0));
        for slot in 0..8 {
            let color = if recolored && slot == 3 {
                Color::rgb(1.0, 0.5, 0.1)
            } else {
                Color::WHITE
            };
            scope.draw_annular_sector(
                Brush::solid(color),
                CENTER,
                80.0 * t.scale * 0.8,
                80.0 * t.scale,
                slot as f32 * 0.7 + t.angle,
                0.02,
            );
        }
        scope.draw_round_rect_at(
            Rect {
                x: 10.0,
                y: 10.0,
                width: 40.0,
                height: 20.0,
            },
            Brush::solid(Color::WHITE),
            CornerRadii::uniform(4.0),
        );
        scope.draw_rect_at(
            Rect {
                x: 60.0,
                y: 10.0,
                width: 20.0,
                height: 20.0,
            },
            Brush::solid(Color::WHITE),
        );
        scope.draw_rect_at(
            Rect {
                x: 90.0,
                y: 10.0,
                width: 20.0,
                height: 20.0,
            },
            Brush::linear_gradient(vec![Color::WHITE, Color::RED]),
        );
        for slot in 0..3 {
            let base = Point::new(304.0, 204.0 + slot as f32 * 20.0);
            let color = if recolored && slot == 1 {
                Color::rgb(0.1, 0.5, 1.0)
            } else {
                Color::WHITE
            };
            scope.draw_circle(Brush::solid(color), t.apply(CENTER, base), 5.0 * t.scale);
        }
        for slot in 0..6 {
            let color = if recolored && slot == 1 {
                Color::rgb(0.9, 0.2, 0.4)
            } else {
                Color::WHITE
            };
            scope.draw_annular_sector(
                Brush::solid(color),
                CENTER,
                120.0 * t.scale * 0.8,
                120.0 * t.scale,
                slot as f32 * 0.9 + 0.1 + t.angle,
                0.03,
            );
        }
        for slot in 0..2 {
            let base = Point::new(104.0, 204.0 + slot as f32 * 24.0);
            let color = if recolored && slot == 1 {
                Color::rgb(0.2, 0.9, 0.3)
            } else {
                Color::WHITE
            };
            scope.draw_circle(Brush::solid(color), t.apply(CENTER, base), 4.0 * t.scale);
        }
        scope.recorded().clone()
    }

    #[test]
    fn interleaved_tape_decomposes_into_exact_runs() {
        let recording = mixed_frame(RecordTransform::IDENTITY, false);
        let tape = &recording.tape;
        let mut runs: Vec<(RecordKind, usize, usize)> = Vec::new();
        let mut at = 0usize;
        while at < tape.len() {
            let len = typed_run_len(tape, at);
            runs.push((tape[at].kind(), tape[at].index(), len));
            at += len;
        }
        assert_eq!(
            runs,
            vec![
                (RecordKind::SolidArc, 0, 8),
                (RecordKind::SolidRoundRect, 0, 1),
                (RecordKind::SolidRect, 0, 1),
                (RecordKind::Other, 0, 1),
                (RecordKind::SolidRoundRect, 1, 3),
                (RecordKind::SolidArc, 8, 6),
                (RecordKind::SolidRoundRect, 4, 2),
            ],
            "run decomposition must cut exactly at kind transitions"
        );
        assert_eq!(typed_run_len(tape, 3), 5);
        assert_eq!(typed_run_len(tape, 8), 1);
        assert_eq!(typed_run_len(tape, 12), 2);
        assert_eq!(typed_run_len(tape, 14), 6);
        assert_eq!(typed_run_len(tape, 20), 2);
    }

    #[test]
    fn run_decomposed_span_match_equals_the_per_entry_walk() {
        let t = RecordTransform {
            scale: 0.9994,
            angle: 0.0123,
        };
        let snapshot_rec = mixed_frame(RecordTransform::IDENTITY, false);
        let mut current_rec = mixed_frame(t, true);
        current_rec.arcs[11].sweep_angle *= 3.0;
        current_rec.arcs[12].start_angle = f32::NAN;
        let current = TypedRecords::from(&current_rec);
        let snapshot = TypedRecords::from(&snapshot_rec);
        let n = current_rec.tape.len();
        assert_eq!(n, snapshot_rec.tape.len());
        assert_eq!(n, 22);
        let mut fast: Vec<(u32, Color)> = Vec::new();
        let mut naive: Vec<(u32, Color)> = Vec::new();
        for start in 0..n {
            for seg_start in 0..n {
                let longest = n - start.max(seg_start);
                for len in [0usize, 1, 2, 5, longest] {
                    if start + len > n || seg_start + len > n {
                        continue;
                    }
                    let matched = match_span(
                        current, snapshot, CENTER, start, seg_start, len, t, &mut fast,
                    );
                    let reference = match_span_reference(
                        current, snapshot, CENTER, start, seg_start, len, t, &mut naive,
                    );
                    assert_eq!(
                        (matched, &fast),
                        (reference, &naive),
                        "diverged at start={start} seg_start={seg_start} len={len}"
                    );
                }
            }
        }
        let matched = match_span(current, snapshot, CENTER, 0, 0, 8, t, &mut fast);
        assert_eq!(
            (matched, fast.as_slice()),
            (8, &[(3, Color::rgb(1.0, 0.5, 0.1))][..])
        );
        let matched = match_span(current, snapshot, CENTER, 11, 11, 3, t, &mut fast);
        assert_eq!(
            (matched, fast.as_slice()),
            (3, &[(1, Color::rgb(0.1, 0.5, 1.0))][..])
        );
        let matched = match_span(current, snapshot, CENTER, 11, 11, 9, t, &mut fast);
        assert_eq!(
            (matched, fast.as_slice()),
            (
                6,
                &[
                    (1, Color::rgb(0.1, 0.5, 1.0)),
                    (4, Color::rgb(0.9, 0.2, 0.4)),
                ][..]
            ),
            "the changed arc ends the clean prefix behind the circles"
        );
        let matched = match_span(current, snapshot, CENTER, 19, 19, 3, t, &mut fast);
        assert_eq!(
            (matched, fast.as_slice()),
            (3, &[(2, Color::rgb(0.2, 0.9, 0.3))][..])
        );
    }

    #[test]
    fn nan_records_mismatch_through_both_paths() {
        let t = RecordTransform::IDENTITY;
        let base = arc(80.0, 0.2, Color::WHITE);
        let poisoned_arcs: [fn(&mut SolidArcRecord); 6] = [
            |a| a.center.x = f32::NAN,
            |a| a.center.y = f32::NAN,
            |a| a.radius = f32::NAN,
            |a| a.inner_radius = f32::NAN,
            |a| a.start_angle = f32::NAN,
            |a| a.sweep_angle = f32::NAN,
        ];
        for poison in poisoned_arcs {
            let mut poisoned = base;
            poison(&mut poisoned);
            assert_eq!(
                match_arc(&poisoned, &base, CENTER, t),
                RecordMatch::Mismatch
            );
            assert_eq!(
                match_arc(&base, &poisoned, CENTER, t),
                RecordMatch::Mismatch
            );
        }
        let good = circle(304.0, 204.0, 10.0, Color::WHITE);
        let poisoned_circles: [fn(&mut SolidRoundRectRecord); 2] =
            [|r| r.rect.x = f32::NAN, |r| r.rect.width = f32::NAN];
        for poison in poisoned_circles {
            let mut poisoned = good;
            poison(&mut poisoned);
            assert_eq!(
                match_round_rect(&poisoned, &good, CENTER, t),
                RecordMatch::Mismatch
            );
            assert_eq!(
                match_round_rect(&good, &poisoned, CENTER, t),
                RecordMatch::Mismatch
            );
        }
        let snapshot_rec = mixed_frame(RecordTransform::IDENTITY, false);
        let mut current_rec = mixed_frame(RecordTransform::IDENTITY, false);
        current_rec.arcs[4].sweep_angle = f32::NAN;
        let current = TypedRecords::from(&current_rec);
        let snapshot = TypedRecords::from(&snapshot_rec);
        let mut fast: Vec<(u32, Color)> = Vec::new();
        let mut naive: Vec<(u32, Color)> = Vec::new();
        let matched = match_span(current, snapshot, CENTER, 0, 0, 8, t, &mut fast);
        let reference = match_span_reference(current, snapshot, CENTER, 0, 0, 8, t, &mut naive);
        assert_eq!(matched, 4, "the NaN record is a mismatch, not a match");
        assert_eq!((matched, &fast), (reference, &naive));
    }

    struct ThreadedExec {
        lanes: usize,
    }

    impl VerifyExecutor for ThreadedExec {
        fn for_each(&self, jobs: usize, run: &(dyn Fn(usize) + Sync)) {
            std::thread::scope(|s| {
                for lane in 1..self.lanes {
                    s.spawn(move || {
                        let mut i = lane;
                        while i < jobs {
                            run(i);
                            i += self.lanes;
                        }
                    });
                }
                let mut i = 0;
                while i < jobs {
                    run(i);
                    i += self.lanes;
                }
            });
        }
    }

    #[test]
    fn pooled_verification_matches_serial_exactly() {
        let exec = ThreadedExec { lanes: 3 };
        let frame = |f: usize| -> CommandRecording {
            let tail = [10usize, 13, 5, 8, 11, 6, 9, 12][f % 8];
            let mut recording = ring_frame(3, 300, f, tail);
            if f >= 3 {
                for i in (0..recording.arcs.len()).step_by(17) {
                    recording.arcs[i].color = if f.is_multiple_of(2) {
                        Color::rgb(1.0, 0.5, 0.1)
                    } else {
                        Color::rgb(0.1, 0.5, 1.0)
                    };
                }
            }
            match f {
                5 => {
                    recording.arcs[450].sweep_angle = 0.15;
                }
                8 => {
                    recording.arcs[100].sweep_angle = 0.15;
                    recording.arcs[750].sweep_angle = 0.15;
                }
                12 => {
                    recording.arcs[500].sweep_angle = 0.15;
                }
                16..=39 => {
                    for arc in &mut recording.arcs[600..900] {
                        arc.sweep_angle = 0.06;
                    }
                }
                40..=45 => {
                    for arc in &mut recording.arcs[150..900] {
                        arc.sweep_angle = 0.08;
                    }
                }
                52 => {
                    recording.arcs[450].sweep_angle = 0.15;
                }
                _ => {}
            }
            recording
        };
        let mut serial = CommandReplayState::default();
        let mut pooled = CommandReplayState::default();
        for f in 0..60 {
            let recording = frame(f);
            let serial_outcome = serial.advance(&recording);
            let pooled_outcome = pooled.advance_pooled(&recording, Some(&exec));
            assert_eq!(
                serial_outcome, pooled_outcome,
                "outcome diverged at frame {f}"
            );
            assert_eq!(
                serial.segments(),
                pooled.segments(),
                "segments diverged at frame {f}"
            );
            assert_eq!(
                serial.stats(),
                pooled.stats(),
                "stats diverged at frame {f}"
            );
        }
        let (deaths, splits) = serial.stats();
        assert!(
            !serial.segments().is_empty() && deaths > 0 && splits > 0,
            "sequence must exercise retention, deaths, and splits, \
             got {deaths} deaths {splits} splits {} segments",
            serial.segments().len()
        );
        assert_eq!(serial.optimistic_commits(), 0);
        assert_eq!(serial.prefix_commits(), 0);
        assert!(
            pooled.optimistic_commits() >= 10,
            "the pooled fast path must actually commit steady frames, got {}",
            pooled.optimistic_commits()
        );
        assert!(
            pooled.prefix_commits() >= 3,
            "churn frames must commit their pooled prefix, got {}",
            pooled.prefix_commits()
        );
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

#[cfg(test)]
mod lane_kernel_equivalence {
    use std::f32::consts::TAU;

    use super::*;
    use crate::{Color, Stroke};

    const KNIFE: f32 = 5.0e-4;

    const PIVOT: Point = Point { x: 204.0, y: 204.0 };
    const T: RecordTransform = RecordTransform {
        scale: 0.9994,
        angle: 0.0123,
    };
    const DENORMAL: f32 = 1.0e-40;
    const POISONS: [f32; 4] = [f32::NAN, f32::INFINITY, f32::NEG_INFINITY, DENORMAL];

    fn transforms() -> [RecordTransform; 5] {
        [
            RecordTransform::IDENTITY,
            T,
            RecordTransform {
                scale: 1.37,
                angle: 3.0,
            },
            RecordTransform {
                scale: f32::NAN,
                angle: f32::NAN,
            },
            RecordTransform {
                scale: f32::INFINITY,
                angle: 0.0,
            },
        ]
    }

    fn bits(recolors: &[(u32, Color)]) -> Vec<(u32, [u32; 4])> {
        recolors
            .iter()
            .map(|&(i, Color(r, g, b, a))| {
                (i, [r.to_bits(), g.to_bits(), b.to_bits(), a.to_bits()])
            })
            .collect()
    }

    struct Case<R> {
        label: String,
        current: R,
        retained: R,
        expected: Option<RecordMatch>,
    }

    fn arc_base(stroke: Option<f32>) -> SolidArcRecord {
        SolidArcRecord {
            center: PIVOT,
            radius: 120.0,
            start_angle: 1.0,
            sweep_angle: 0.4,
            inner_radius: 96.0,
            color: Color::WHITE,
            stroke: stroke.map(Stroke::new),
        }
    }

    fn arc_moved(retained: &SolidArcRecord) -> SolidArcRecord {
        SolidArcRecord {
            center: retained.center,
            radius: retained.radius * T.scale,
            start_angle: retained.start_angle + T.angle,
            sweep_angle: retained.sweep_angle,
            inner_radius: retained.inner_radius * T.scale,
            color: retained.color,
            stroke: retained.stroke.map(|stroke| Stroke {
                width: stroke.width * T.scale,
                ..stroke
            }),
        }
    }

    type ArcGet = fn(&SolidArcRecord) -> f32;
    type ArcSet = fn(&mut SolidArcRecord, f32);

    fn arc_fields() -> [(&'static str, ArcGet, ArcSet); 6] {
        [
            ("center.x", |a| a.center.x, |a, v| a.center.x = v),
            ("center.y", |a| a.center.y, |a, v| a.center.y = v),
            ("radius", |a| a.radius, |a, v| a.radius = v),
            (
                "inner_radius",
                |a| a.inner_radius,
                |a, v| a.inner_radius = v,
            ),
            ("start_angle", |a| a.start_angle, |a, v| a.start_angle = v),
            ("sweep_angle", |a| a.sweep_angle, |a, v| a.sweep_angle = v),
        ]
    }

    fn arc_corpus() -> Vec<Case<SolidArcRecord>> {
        let mut corpus: Vec<Case<SolidArcRecord>> = Vec::new();
        for stroke in [None, Some(5.0_f32)] {
            let retained = arc_base(stroke);
            let matched = arc_moved(&retained);
            corpus.push(Case {
                label: format!("exact, stroke {stroke:?}"),
                current: matched,
                retained,
                expected: Some(RecordMatch::Exact),
            });
            let mut recolored = matched;
            recolored.color = Color::rgb(0.9, 0.3, 0.2);
            corpus.push(Case {
                label: format!("recolor, stroke {stroke:?}"),
                current: recolored,
                retained,
                expected: Some(RecordMatch::Recolor),
            });
        }

        let retained = arc_base(None);
        let matched = arc_moved(&retained);
        for (name, get, set) in arc_fields() {
            let base = get(&matched);
            let tolerance = if name == "start_angle" {
                ABS_EPS
            } else {
                ABS_EPS + REL_EPS * base.abs()
            };
            for sign in [1.0_f32, -1.0] {
                let mut inside = matched;
                set(&mut inside, base + sign * 0.9 * tolerance);
                corpus.push(Case {
                    label: format!("{name} just inside, sign {sign}"),
                    current: inside,
                    retained,
                    expected: Some(RecordMatch::Exact),
                });
                let mut outside = matched;
                set(&mut outside, base + sign * 1.1 * tolerance);
                corpus.push(Case {
                    label: format!("{name} just outside, sign {sign}"),
                    current: outside,
                    retained,
                    expected: Some(RecordMatch::Mismatch),
                });
            }
            for poison in POISONS {
                let mut current = matched;
                set(&mut current, poison);
                corpus.push(Case {
                    label: format!("{name} current {poison:e}"),
                    current,
                    retained,
                    expected: None,
                });
                let mut poisoned = retained;
                set(&mut poisoned, poison);
                corpus.push(Case {
                    label: format!("{name} retained {poison:e}"),
                    current: matched,
                    retained: poisoned,
                    expected: None,
                });
                let mut both_current = matched;
                set(&mut both_current, poison);
                corpus.push(Case {
                    label: format!("{name} both {poison:e}"),
                    current: both_current,
                    retained: poisoned,
                    expected: None,
                });
            }
            for knife in [tolerance - KNIFE, tolerance, tolerance + KNIFE] {
                for sign in [1.0_f32, -1.0] {
                    let mut edge = matched;
                    set(&mut edge, base + sign * knife);
                    corpus.push(Case {
                        label: format!("{name} knife edge {knife:e}, sign {sign}"),
                        current: edge,
                        retained,
                        expected: None,
                    });
                }
            }
        }

        for (label, delta, expected) in [
            ("start_angle +TAU", TAU, RecordMatch::Exact),
            ("start_angle -TAU", -TAU, RecordMatch::Exact),
            ("start_angle +3 turns", 3.0 * TAU, RecordMatch::Exact),
            (
                "start_angle short of +TAU, inside",
                TAU - 0.9 * ABS_EPS,
                RecordMatch::Exact,
            ),
            (
                "start_angle past +TAU, outside",
                TAU + 1.1 * ABS_EPS,
                RecordMatch::Mismatch,
            ),
        ] {
            let mut wrapped = matched;
            wrapped.start_angle += delta;
            corpus.push(Case {
                label: label.to_string(),
                current: wrapped,
                retained,
                expected: Some(expected),
            });
        }

        let stroked = arc_base(Some(5.0));
        let moved_stroked = arc_moved(&stroked);
        let mut some_vs_none = matched;
        some_vs_none.stroke = Some(Stroke::new(5.0 * T.scale));
        corpus.push(Case {
            label: "stroke Some vs None".to_string(),
            current: some_vs_none,
            retained,
            expected: Some(RecordMatch::Mismatch),
        });
        corpus.push(Case {
            label: "stroke None vs Some".to_string(),
            current: matched,
            retained: stroked,
            expected: Some(RecordMatch::Mismatch),
        });
        let width = 5.0 * T.scale;
        let tolerance = ABS_EPS + REL_EPS * width.abs();
        for sign in [1.0_f32, -1.0] {
            let mut inside = moved_stroked;
            inside.stroke = Some(Stroke::new(width + sign * 0.9 * tolerance));
            corpus.push(Case {
                label: format!("stroke width just inside, sign {sign}"),
                current: inside,
                retained: stroked,
                expected: Some(RecordMatch::Exact),
            });
            let mut outside = moved_stroked;
            outside.stroke = Some(Stroke::new(width + sign * 1.1 * tolerance));
            corpus.push(Case {
                label: format!("stroke width just outside, sign {sign}"),
                current: outside,
                retained: stroked,
                expected: Some(RecordMatch::Mismatch),
            });
            for knife in [tolerance - KNIFE, tolerance, tolerance + KNIFE] {
                let mut edge = moved_stroked;
                edge.stroke = Some(Stroke::new(width + sign * knife));
                corpus.push(Case {
                    label: format!("stroke width knife edge {knife:e}, sign {sign}"),
                    current: edge,
                    retained: stroked,
                    expected: None,
                });
            }
        }
        for poison in POISONS {
            let mut current = moved_stroked;
            current.stroke = Some(Stroke::new(poison));
            corpus.push(Case {
                label: format!("stroke width current {poison:e}"),
                current,
                retained: stroked,
                expected: None,
            });
            let mut poisoned = stroked;
            poisoned.stroke = Some(Stroke::new(poison));
            corpus.push(Case {
                label: format!("stroke width retained {poison:e}"),
                current: moved_stroked,
                retained: poisoned,
                expected: None,
            });
        }

        let mut nan_color = matched;
        nan_color.color = Color(f32::NAN, 0.5, 0.5, 1.0);
        corpus.push(Case {
            label: "NaN color".to_string(),
            current: nan_color,
            retained,
            expected: Some(RecordMatch::Recolor),
        });
        corpus
    }

    #[test]
    fn the_arc_corpus_exercises_what_it_claims() {
        for case in arc_corpus() {
            if let Some(expected) = case.expected {
                assert_eq!(
                    match_arc(&case.current, &case.retained, PIVOT, T),
                    expected,
                    "scalar verdict for `{}`",
                    case.label
                );
            }
        }
    }

    #[test]
    fn arc_kernel_equals_the_scalar_authority_cross_paired() {
        let corpus = arc_corpus();
        for t in transforms() {
            for a in &corpus {
                for b in &corpus {
                    assert_eq!(
                        match_arc_lanes(&a.current, &b.retained, PIVOT, t.scale, t.angle),
                        match_arc(&a.current, &b.retained, PIVOT, t),
                        "arc kernel diverged: current `{}` vs retained `{}` under {t:?}",
                        a.label,
                        b.label
                    );
                }
            }
        }
    }

    #[test]
    fn arc_run_equals_a_scalar_reference_from_every_start() {
        let corpus = arc_corpus();
        let current: Vec<SolidArcRecord> = corpus.iter().map(|case| case.current).collect();
        let snapshot: Vec<SolidArcRecord> = corpus.iter().map(|case| case.retained).collect();
        let mut fast: Vec<(u32, Color)> = Vec::new();
        let mut naive: Vec<(u32, Color)> = Vec::new();
        for start in 0..current.len() {
            fast.clear();
            naive.clear();
            let matched = match_arc_run(
                &current[start..],
                &snapshot[start..],
                PIVOT,
                T,
                7,
                &mut fast,
            );
            let mut mismatch = None;
            for (i, (now, then)) in current[start..].iter().zip(&snapshot[start..]).enumerate() {
                match match_arc(now, then, PIVOT, T) {
                    RecordMatch::Exact => {}
                    RecordMatch::Recolor => naive.push(((7 + i) as u32, now.color)),
                    RecordMatch::Mismatch => {
                        mismatch = Some(i);
                        break;
                    }
                }
            }
            let reference = mismatch.unwrap_or(current.len() - start);
            assert_eq!(
                (matched, bits(&fast)),
                (reference, bits(&naive)),
                "arc run diverged from start {start}"
            );
        }
    }

    fn rr_base(stroke: Option<f32>) -> SolidRoundRectRecord {
        SolidRoundRectRecord {
            rect: Rect {
                x: 299.0,
                y: 199.0,
                width: 10.0,
                height: 10.0,
            },
            radii: CornerRadii::uniform(5.0),
            color: Color::WHITE,
            stroke: stroke.map(Stroke::new),
        }
    }

    fn rr_moved(retained: &SolidRoundRectRecord) -> SolidRoundRectRecord {
        let (c_then, d_then) = circle_view(retained).expect("the base is a circle");
        let c_now = T.apply(PIVOT, c_then);
        let d_now = d_then * T.scale;
        SolidRoundRectRecord {
            rect: Rect {
                x: c_now.x - d_now * 0.5,
                y: c_now.y - d_now * 0.5,
                width: d_now,
                height: d_now,
            },
            radii: CornerRadii::uniform(d_now * 0.5),
            color: retained.color,
            stroke: retained.stroke.map(|stroke| Stroke {
                width: stroke.width * T.scale,
                ..stroke
            }),
        }
    }

    type RrGet = fn(&SolidRoundRectRecord) -> f32;
    type RrSet = fn(&mut SolidRoundRectRecord, f32);

    fn rr_fields() -> [(&'static str, RrGet, RrSet); 8] {
        [
            ("rect.x", |r| r.rect.x, |r, v| r.rect.x = v),
            ("rect.y", |r| r.rect.y, |r, v| r.rect.y = v),
            ("rect.width", |r| r.rect.width, |r, v| r.rect.width = v),
            ("rect.height", |r| r.rect.height, |r, v| r.rect.height = v),
            (
                "radii.top_left",
                |r| r.radii.top_left,
                |r, v| r.radii.top_left = v,
            ),
            (
                "radii.top_right",
                |r| r.radii.top_right,
                |r, v| r.radii.top_right = v,
            ),
            (
                "radii.bottom_right",
                |r| r.radii.bottom_right,
                |r, v| r.radii.bottom_right = v,
            ),
            (
                "radii.bottom_left",
                |r| r.radii.bottom_left,
                |r, v| r.radii.bottom_left = v,
            ),
        ]
    }

    fn rr_corpus() -> Vec<Case<SolidRoundRectRecord>> {
        let mut corpus: Vec<Case<SolidRoundRectRecord>> = Vec::new();
        for stroke in [None, Some(3.0_f32)] {
            let retained = rr_base(stroke);
            let matched = rr_moved(&retained);
            corpus.push(Case {
                label: format!("exact, stroke {stroke:?}"),
                current: matched,
                retained,
                expected: Some(RecordMatch::Exact),
            });
            let mut recolored = matched;
            recolored.color = Color::rgb(0.2, 0.8, 0.4);
            corpus.push(Case {
                label: format!("recolor, stroke {stroke:?}"),
                current: recolored,
                retained,
                expected: Some(RecordMatch::Recolor),
            });
        }

        let retained = rr_base(None);
        let matched = rr_moved(&retained);
        for (name, get, set) in rr_fields() {
            let base = get(&matched);
            let tolerance = ABS_EPS + REL_EPS * base.abs();
            for sign in [1.0_f32, -1.0] {
                let mut inside = matched;
                set(&mut inside, base + sign * 0.9 * tolerance);
                corpus.push(Case {
                    label: format!("{name} just inside, sign {sign}"),
                    current: inside,
                    retained,
                    expected: Some(RecordMatch::Exact),
                });
                let mut outside = matched;
                set(&mut outside, base + sign * 1.1 * tolerance);
                corpus.push(Case {
                    label: format!("{name} just outside, sign {sign}"),
                    current: outside,
                    retained,
                    expected: Some(RecordMatch::Mismatch),
                });
            }
            for poison in POISONS {
                let mut current = matched;
                set(&mut current, poison);
                corpus.push(Case {
                    label: format!("{name} current {poison:e}"),
                    current,
                    retained,
                    expected: None,
                });
                let mut poisoned = retained;
                set(&mut poisoned, poison);
                corpus.push(Case {
                    label: format!("{name} retained {poison:e}"),
                    current: matched,
                    retained: poisoned,
                    expected: None,
                });
                let mut both_current = matched;
                set(&mut both_current, poison);
                corpus.push(Case {
                    label: format!("{name} both {poison:e}"),
                    current: both_current,
                    retained: poisoned,
                    expected: None,
                });
            }
            for knife in [tolerance - KNIFE, tolerance, tolerance + KNIFE] {
                for sign in [1.0_f32, -1.0] {
                    let mut edge = matched;
                    set(&mut edge, base + sign * knife);
                    corpus.push(Case {
                        label: format!("{name} knife edge {knife:e}, sign {sign}"),
                        current: edge,
                        retained,
                        expected: None,
                    });
                }
            }
        }

        let mut squashed = matched;
        squashed.rect.height = squashed.rect.width * 2.0;
        corpus.push(Case {
            label: "current non-circle (squashed)".to_string(),
            current: squashed,
            retained,
            expected: Some(RecordMatch::Mismatch),
        });
        let mut loose_radii = retained;
        loose_radii.radii = CornerRadii::uniform(2.0);
        corpus.push(Case {
            label: "retained non-circle (loose radii)".to_string(),
            current: matched,
            retained: loose_radii,
            expected: Some(RecordMatch::Mismatch),
        });
        corpus.push(Case {
            label: "non-circle vs itself".to_string(),
            current: loose_radii,
            retained: loose_radii,
            expected: Some(RecordMatch::Mismatch),
        });

        let stroked = rr_base(Some(3.0));
        let moved_stroked = rr_moved(&stroked);
        let mut some_vs_none = matched;
        some_vs_none.stroke = Some(Stroke::new(3.0 * T.scale));
        corpus.push(Case {
            label: "stroke Some vs None".to_string(),
            current: some_vs_none,
            retained,
            expected: Some(RecordMatch::Mismatch),
        });
        corpus.push(Case {
            label: "stroke None vs Some".to_string(),
            current: matched,
            retained: stroked,
            expected: Some(RecordMatch::Mismatch),
        });
        let width = 3.0 * T.scale;
        let tolerance = ABS_EPS + REL_EPS * width.abs();
        for sign in [1.0_f32, -1.0] {
            let mut inside = moved_stroked;
            inside.stroke = Some(Stroke::new(width + sign * 0.9 * tolerance));
            corpus.push(Case {
                label: format!("stroke width just inside, sign {sign}"),
                current: inside,
                retained: stroked,
                expected: Some(RecordMatch::Exact),
            });
            let mut outside = moved_stroked;
            outside.stroke = Some(Stroke::new(width + sign * 1.1 * tolerance));
            corpus.push(Case {
                label: format!("stroke width just outside, sign {sign}"),
                current: outside,
                retained: stroked,
                expected: Some(RecordMatch::Mismatch),
            });
            for knife in [tolerance - KNIFE, tolerance, tolerance + KNIFE] {
                let mut edge = moved_stroked;
                edge.stroke = Some(Stroke::new(width + sign * knife));
                corpus.push(Case {
                    label: format!("stroke width knife edge {knife:e}, sign {sign}"),
                    current: edge,
                    retained: stroked,
                    expected: None,
                });
            }
        }
        for poison in POISONS {
            let mut current = moved_stroked;
            current.stroke = Some(Stroke::new(poison));
            corpus.push(Case {
                label: format!("stroke width current {poison:e}"),
                current,
                retained: stroked,
                expected: None,
            });
        }

        let mut nan_color = matched;
        nan_color.color = Color(f32::NAN, 0.5, 0.5, 1.0);
        corpus.push(Case {
            label: "NaN color".to_string(),
            current: nan_color,
            retained,
            expected: Some(RecordMatch::Recolor),
        });
        corpus
    }

    #[test]
    fn the_round_rect_corpus_exercises_what_it_claims() {
        for case in rr_corpus() {
            if let Some(expected) = case.expected {
                assert_eq!(
                    match_round_rect(&case.current, &case.retained, PIVOT, T),
                    expected,
                    "scalar verdict for `{}`",
                    case.label
                );
            }
        }
    }

    #[test]
    fn round_rect_kernel_equals_the_scalar_authority_cross_paired() {
        let corpus = rr_corpus();
        for t in transforms() {
            let (sin, cos) = t.angle.sin_cos();
            for a in &corpus {
                for b in &corpus {
                    assert_eq!(
                        match_round_rect_lanes(&a.current, &b.retained, PIVOT, t.scale, sin, cos),
                        match_round_rect(&a.current, &b.retained, PIVOT, t),
                        "round-rect kernel diverged: current `{}` vs retained `{}` under {t:?}",
                        a.label,
                        b.label
                    );
                }
            }
        }
    }

    #[test]
    fn round_rect_run_equals_a_scalar_reference_from_every_start() {
        let corpus = rr_corpus();
        let current: Vec<SolidRoundRectRecord> = corpus.iter().map(|case| case.current).collect();
        let snapshot: Vec<SolidRoundRectRecord> = corpus.iter().map(|case| case.retained).collect();
        let mut fast: Vec<(u32, Color)> = Vec::new();
        let mut naive: Vec<(u32, Color)> = Vec::new();
        for start in 0..current.len() {
            fast.clear();
            naive.clear();
            let matched = match_round_rect_run(
                &current[start..],
                &snapshot[start..],
                PIVOT,
                T,
                7,
                &mut fast,
            );
            let mut mismatch = None;
            for (i, (now, then)) in current[start..].iter().zip(&snapshot[start..]).enumerate() {
                match match_round_rect(now, then, PIVOT, T) {
                    RecordMatch::Exact => {}
                    RecordMatch::Recolor => naive.push(((7 + i) as u32, now.color)),
                    RecordMatch::Mismatch => {
                        mismatch = Some(i);
                        break;
                    }
                }
            }
            let reference = mismatch.unwrap_or(current.len() - start);
            assert_eq!(
                (matched, bits(&fast)),
                (reference, bits(&naive)),
                "round-rect run diverged from start {start}"
            );
        }
    }

    struct XorShift(u32);

    impl XorShift {
        fn next(&mut self) -> u32 {
            let mut x = self.0;
            x ^= x << 13;
            x ^= x >> 17;
            x ^= x << 5;
            self.0 = x;
            x
        }

        fn f32(&mut self) -> f32 {
            if self.next() & 1 == 0 {
                (self.next() as f32 / u32::MAX as f32) * 1000.0 - 500.0
            } else {
                f32::from_bits(self.next())
            }
        }

        fn stroke(&mut self) -> Option<Stroke> {
            (self.next() & 1 == 0).then(|| Stroke::new(self.f32()))
        }

        fn arc(&mut self) -> SolidArcRecord {
            SolidArcRecord {
                center: Point::new(self.f32(), self.f32()),
                radius: self.f32(),
                start_angle: self.f32(),
                sweep_angle: self.f32(),
                inner_radius: self.f32(),
                color: Color::WHITE,
                stroke: self.stroke(),
            }
        }

        fn round_rect(&mut self) -> SolidRoundRectRecord {
            SolidRoundRectRecord {
                rect: Rect {
                    x: self.f32(),
                    y: self.f32(),
                    width: self.f32(),
                    height: self.f32(),
                },
                radii: CornerRadii {
                    top_left: self.f32(),
                    top_right: self.f32(),
                    bottom_right: self.f32(),
                    bottom_left: self.f32(),
                },
                color: Color::WHITE,
                stroke: self.stroke(),
            }
        }
    }

    #[test]
    fn kernels_equal_the_authorities_on_arbitrary_bit_patterns() {
        let mut rng = XorShift(0x9e37_79b9);
        for _ in 0..4000 {
            let t = RecordTransform {
                scale: rng.f32(),
                angle: rng.f32(),
            };
            let (sin, cos) = t.angle.sin_cos();
            let (a, b) = (rng.arc(), rng.arc());
            assert_eq!(
                match_arc_lanes(&a, &b, PIVOT, t.scale, t.angle),
                match_arc(&a, &b, PIVOT, t),
                "arc kernel diverged: {a:?} vs {b:?} under {t:?}"
            );
            let (c, d) = (rng.round_rect(), rng.round_rect());
            assert_eq!(
                match_round_rect_lanes(&c, &d, PIVOT, t.scale, sin, cos),
                match_round_rect(&c, &d, PIVOT, t),
                "round-rect kernel diverged: {c:?} vs {d:?} under {t:?}"
            );
        }
    }
}
