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

use crate::geometry::{
    CommandRecording, Point, RecordKind, Rect, SolidArcRecord, SolidRoundRectRecord, TapeRef,
};
use crate::{Color, CornerRadii};

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
    // `&`, not `&&`: both comparisons are pure, so evaluating the second
    // unconditionally cannot change the result — it removes a branch from
    // the hot contiguous-run match loops. NaN anywhere makes its `close_rel`
    // false regardless of evaluation order.
    close_rel(a.x, b.x) & close_rel(a.y, b.y)
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
    // `&`, not `&&`: every term is a pure comparison, so unconditional
    // evaluation is result-identical and keeps the hot run loops branch-light.
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
/// This is THE arc comparison — the serial per-pair path and the contiguous
/// run loop both call it, so the tolerance semantics cannot drift between
/// them. The tolerance terms combine with `&`, not `&&`: each `close_*` is a
/// pure comparison, so evaluating all of them unconditionally is
/// result-identical to the short-circuit form (a NaN in any field makes its
/// own comparison false regardless of order) while the common all-match case
/// takes one branch per record instead of seven.
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
/// Like [`match_arc`], this is the one round-rect comparison both the serial
/// per-pair path and the contiguous run loop use; the tolerance terms
/// combine with `&` because each is pure, so unconditional evaluation is
/// result-identical (NaN included) and the all-match case stays
/// branch-light.
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
        // Non-circular round rects cannot survive rotation about an
        // external pivot; they stay dynamic.
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
/// segment, never a wrong capture.
fn align_recordings(current: &CommandRecording, retained: &CommandRecording) -> Vec<Option<usize>> {
    let pair = |i: usize, j: usize| -> bool {
        views_compatible(current, view_at(current, i), retained, view_at(retained, j))
    };
    let current_len = current.tape.len();
    let retained_len = retained.tape.len();
    let mut aligned = vec![None; current_len];
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
            // Not churn — the structure is gone. An empty alignment makes
            // the caller restart from a fresh snapshot.
            return vec![None; current_len];
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
    aligned
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
}

/// One span of this frame's recording, in tape order.
#[derive(Clone, Debug, PartialEq)]
pub enum ReplaySpan {
    /// The retained segment moved by `transform`; `recolors` are
    /// (span-relative record offset, new color) patches.
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
    /// Fraction of the tape the capture covered when it was taken. Dead
    /// segments never come back on their own, so coverage eroding well
    /// below this watermark means stable content sits unwatched — worth
    /// paying a recapture for.
    capture_coverage: f32,
    frames_since_capture: u32,
    /// Frames the pooled fast path fully committed — diagnostics for
    /// judging how often verification actually parallelizes.
    optimistic_commits: u64,
    /// Frames where the pooled pass committed a non-empty strict prefix of
    /// the segments and the serial walk ran only from the first failure —
    /// diagnostics for the churn frames (a brick hit) that used to redo the
    /// whole tape serially.
    prefix_commits: u64,
    /// Reusable per-job result slots for the pooled fast path — one slot
    /// per segment, grown once, recolor capacity retained across frames.
    /// The Mutex is uncontended (each job writes only its own slot once);
    /// what this kills is the per-frame allocation of the results vector,
    /// its mutexes, and every job's recolors vector. Each committed span —
    /// the whole frame, or the prefix before the first failure —
    /// `mem::take`s its slot's recolors: the buffer walks into the graph
    /// and the slot re-grows next frame (accepted: emitting spans do real
    /// work). Uncommitted slots keep their buffers warm.
    verify_results: Vec<std::sync::Mutex<SpanResultSlot>>,
    /// The serial walk's recolor buffer, refilled by every `match_span`
    /// commit attempt. An emitted span `mem::take`s the contents and the
    /// scratch re-grows on the next attempt — same accepted emit-cost as
    /// the pooled slots.
    recolor_scratch: Vec<(u32, Color)>,
    /// The best-prefix recolors during the serial walk's candidate scan,
    /// swapped with `recolor_scratch` whenever a longer prefix turns up.
    best_recolor_scratch: Vec<(u32, Color)>,
    /// Serial-walk segment queues, persistent so their buffers keep their
    /// high-water capacity; refilled per verified frame.
    verify_pending: std::collections::VecDeque<CommandSegment>,
    verify_survivors: Vec<CommandSegment>,
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
        self.snapshot = current.clone();
        self.center = center;
        self.segments.clear();
        self.phase = CommandReplayPhase::Snapshotted;
    }

    /// Splits the recording into maximal chains of consecutive entries that
    /// moved from the snapshot by one shared similarity transform, then
    /// re-snapshots at the current values so verification always compares
    /// against the capture frame. The returned spans carry the capture
    /// content itself (`capture: true`, identity transform): the snapshot
    /// IS this frame, so what the renderer retains equals what later
    /// transforms move.
    fn partition(&mut self, current: &CommandRecording, center: Point) -> ReplayOutcome {
        let aligned = align_recordings(current, &self.snapshot);
        let mut chains: Vec<(usize, usize)> = Vec::new();
        let mut i = 0;
        while i < current.tape.len() {
            let (Some(view), Some(snapshot_view)) = (
                view_at(current, i),
                aligned[i].and_then(|j| view_at(&self.snapshot, j)),
            ) else {
                i += 1;
                continue;
            };
            // A chain anchor must pin rotation itself.
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
                    aligned[end].and_then(|j| view_at(&self.snapshot, j)),
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
        // Re-snapshot at current values: chain ranges are current-tape
        // ranges, which the fresh snapshot preserves verbatim.
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

    /// Verifies this frame's recording against the capture. Each segment
    /// re-locates its anchor by searching forward from the cursor within
    /// [`RESYNC_WINDOW`] — dynamic spans between segments change length
    /// freely — probing a few entries under each candidate transform before
    /// committing to a full-span verification (a wrong candidate from a
    /// different ring fails the probe on its radii). A mismatch mid-span
    /// splits the segment: the matched prefix stays retained, the record
    /// that changed goes dynamic, and the suffix re-enters the location
    /// queue as its own segment — churn costs the records it touched, not
    /// the whole capture. Eroded coverage re-snapshots for the next frame.
    fn verify(
        &mut self,
        current: &CommandRecording,
        pool: Option<&dyn VerifyExecutor>,
    ) -> ReplayOutcome {
        let mut spans: Vec<ReplaySpan> = Vec::new();
        let mut retained_records = 0usize;
        // Current-tape position covered so far.
        let mut cursor = 0usize;
        // Leading segments the pooled pass already committed; the serial
        // walk below runs only from this point on.
        let mut committed = 0usize;
        if let Some(pool) = pool {
            if self.segments.len() >= 2 {
                let commit = self.verify_optimistic(current, pool);
                if commit.committed == self.segments.len() {
                    self.optimistic_commits += 1;
                    return self.finish_verify(current, commit.spans, commit.retained_records);
                }
                // Prefix-commit: the pooled spans for every segment before
                // the first failure are equal by construction to what the
                // serial walk would produce for them (see
                // [`Self::verify_optimistic`]), so they are kept and the
                // serial machinery below is seeded from the failure point
                // instead of redoing the whole tape.
                if commit.committed > 0 {
                    self.prefix_commits += 1;
                }
                spans = commit.spans;
                retained_records = commit.retained_records;
                cursor = commit.cursor;
                committed = commit.committed;
            }
        }
        // Segments awaiting location this frame, tape order. A split pushes
        // the suffix back onto the front so it is located before the next
        // original segment. Both queues are persistent fields refilled per
        // frame, so their buffers keep their high-water capacity.
        self.verify_pending.clear();
        self.verify_pending.extend(self.segments.drain(committed..));
        self.verify_survivors.clear();
        // A committed segment matched whole, so it survives unchanged — in
        // emission order, ahead of whatever the serial walk keeps.
        self.verify_survivors.append(&mut self.segments);
        while let Some(segment) = self.verify_pending.pop_front() {
            let len = segment.tape_end - segment.tape_start;
            let search_end = (cursor + RESYNC_WINDOW)
                .min(current.tape.len().saturating_sub(len - 1))
                .max(cursor);
            // Candidates run LEFT TO RIGHT from the cursor, never by
            // proximity to an expected position: within a self-similar
            // ring, every pairing shifted right of the true anchor passes
            // probes (recolor-tolerant matching even repaints the color
            // pattern) with a sub-tolerance angle residual — the one
            // pairing a distance heuristic must never be allowed to reach
            // first. The true anchor is always the LEFTMOST compatible
            // candidate, exactly the order the flat detector proved out.
            let candidates = cursor..search_end;
            let mut located: Option<(usize, RecordTransform)> = None;
            // The longest cleanly matched prefix among failed commits:
            // (start, transform); its length and the recolors within it
            // live in `best_prefix_len` / `best_recolor_scratch`. A genuine
            // mid-span change surfaces here — the right anchor matches far
            // more than any mislocated one.
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
                // Committed: verify the whole span. A failure may still be a
                // mislocated anchor (self-similar rings), so the search
                // resumes — a bounded number of times.
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
                        // Keep the best prefix's recolors without an
                        // allocation: the two scratches trade places.
                        std::mem::swap(&mut self.recolor_scratch, &mut self.best_recolor_scratch);
                    }
                    // Only failures with a substantial matched prefix
                    // consume the commit budget: those are genuine split
                    // candidates, and re-verifying long spans is the cost
                    // being bounded. A short-prefix failure is just a wrong
                    // anchor (a dead predecessor's entries, a cross-ring
                    // pairing) that the scan must be free to step past —
                    // charging those burned the budget before the true
                    // anchor and killed healthy segments.
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
            // A failed segment splits around the record that changed: the
            // matched prefix is retained now, the suffix re-enters the
            // queue to locate itself past whatever churn displaced it. Only
            // a prefix long enough to prove the anchor was right earns a
            // split — a segment with no solid prefix dies whole, or a weak
            // wrong-anchor prefix would shed one record and re-fail across
            // the whole span. The emitted span `mem::take`s its recolors
            // out of the owning scratch — the buffer walks into the graph
            // and the scratch re-grows on the next attempt (accepted:
            // emitting spans do real work).
            let (span_start, t, recolors, span_len) = match located {
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
                        // The suffix addresses the SAME captured content,
                        // just deeper in: no recapture, only an offset.
                        self.verify_pending.push_front(CommandSegment {
                            slot: segment.slot,
                            slot_offset: segment.slot_offset + (suffix_start - segment.tape_start),
                            tape_start: suffix_start,
                            tape_end: segment.tape_end,
                            bounds: range_bounds(&self.snapshot, (suffix_start, segment.tape_end)),
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
            let survivor = if span_len == len {
                segment
            } else {
                // The prefix keeps its capture identity — it addresses the
                // same slot content from the same offset, just shorter.
                CommandSegment {
                    slot: segment.slot,
                    slot_offset: segment.slot_offset,
                    tape_start: segment.tape_start,
                    tape_end: segment.tape_start + span_len,
                    bounds: range_bounds(
                        &self.snapshot,
                        (segment.tape_start, segment.tape_start + span_len),
                    ),
                }
            };
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

        // Survivors become the live table; swapping (the table was drained
        // above) lets the two buffers ping-pong, both keeping capacity.
        std::mem::swap(&mut self.segments, &mut self.verify_survivors);
        self.finish_verify(current, spans, retained_records)
    }

    /// The verification epilogue shared by the serial and pooled paths:
    /// coverage bookkeeping and the collapse/erosion re-snapshot decision.
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
        if collapsed || eroded {
            // Re-snapshot so the next two frames re-partition. Collapse pays
            // immediately; mere erosion waits out the capture cooldown.
            let center = self.center;
            self.take_snapshot(current, center);
            if retained_records == 0 {
                return ReplayOutcome::AllDynamic;
            }
        }
        ReplayOutcome::Spans(spans)
    }

    /// The pooled fast path: locates segments serially with cheap probes
    /// only (identical candidate order to the serial walk — leftmost from
    /// the cursor), then fans the expensive full-span matching across
    /// `pool` and commits the longest prefix of segments whose bodies
    /// matched whole.
    ///
    /// Each committed span is EQUAL BY CONSTRUCTION to the serial walk's:
    /// the serial walk commits a segment at the first candidate that both
    /// passes [`probe_anchor`] and matches its whole body under
    /// [`match_span`]; for a committed segment here, the first
    /// probe-passing candidate matched whole, no earlier candidate even
    /// probe-passes, and both functions are deterministic over the same
    /// inputs — so anchor, transform, tape range, recolors and bounds all
    /// coincide, as does the cursor both walks carry forward
    /// (`start + len`, by induction from a shared start of zero). The
    /// commit therefore ends at the first failure — a segment with no
    /// probe-passing candidate in its window, or a body that matched short
    /// (a genuine change, or a mislocated anchor on a self-similar ring):
    /// from that segment on, only the serial walk's candidate-scan budget
    /// and split/death machinery can decide the frame, starting from the
    /// identical cursor. Uncommitted result slots are left untouched so
    /// their recolor buffers stay warm.
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
            // No probe-passing candidate: the serial walk would scan these
            // same candidates, find none, and kill the segment — machinery
            // this pass does not carry. Job collection stops here; the
            // jobs already collected are still worth their pooled bodies.
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
        // One reusable result slot per job, grown once and kept across
        // frames; every job writes only its own slot, filling the slot's
        // own recolor buffer in place via the out-param.
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
        // The commit ends at the first body that matched short. Slots from
        // that job on are not taken, so their recolor buffers stay warm
        // for the serial rerun and later frames.
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
            .iter()
            .zip(jobs.iter().zip(&self.verify_results))
            .take(committed)
        {
            // Committing: each emitted span takes its slot's buffer — the
            // capacity walks into the graph and the slot re-grows next
            // frame (accepted: emitting spans do real work).
            let recolors =
                std::mem::take(&mut result.lock().expect("verify span job lock").recolors);
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
        // The trailing dynamic span belongs to whichever path covers the
        // tape's tail: this one only when every segment committed.
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
    /// Leading segments committed exactly as the serial walk would have.
    committed: usize,
    /// Current-tape position after the last committed span.
    cursor: usize,
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
    /// [`view_at`] over the POD slices — the same [`view_at_slices`]
    /// implementation, so eligibility cannot drift between the two forms.
    /// The shipping span match no longer decodes per entry (it walks
    /// contiguous per-store runs); only the tests' naive reference walk
    /// still reads entries this way.
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
    // Invariant: the predicate holds for every d < lo and fails for every
    // d >= hi. P(0) is trivially true; the run is the first failing d.
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

/// The tight arc loop over one contiguous run pair: no tape decode, no kind
/// dispatch — direct slice indexing with [`match_arc`] (the same function
/// the serial per-pair path uses, so tolerance semantics cannot drift).
/// Returns the cleanly matched length; recolors are pushed as
/// (`span_offset` + run-relative index, color), exactly the entries the
/// per-entry walk would have produced.
fn match_arc_run(
    current: &[SolidArcRecord],
    snapshot: &[SolidArcRecord],
    center: Point,
    t: RecordTransform,
    span_offset: usize,
    recolors: &mut Vec<(u32, Color)>,
) -> usize {
    for (i, (now, then)) in current.iter().zip(snapshot).enumerate() {
        match match_arc(now, then, center, t) {
            RecordMatch::Exact => {}
            RecordMatch::Recolor => recolors.push(((span_offset + i) as u32, now.color)),
            RecordMatch::Mismatch => return i,
        }
    }
    current.len()
}

/// [`match_arc_run`]'s round-rect twin. [`match_round_rect`] itself rejects
/// non-circular round rects, which is the same verdict the per-entry walk's
/// eligibility check produced for them (`view_at` maps a non-circle to
/// `None`, and any `None` pairing is a mismatch), so no separate
/// eligibility pass is needed here.
fn match_round_rect_run(
    current: &[SolidRoundRectRecord],
    snapshot: &[SolidRoundRectRecord],
    center: Point,
    t: RecordTransform,
    span_offset: usize,
    recolors: &mut Vec<(u32, Color)>,
) -> usize {
    for (i, (now, then)) in current.iter().zip(snapshot).enumerate() {
        match match_round_rect(now, then, center, t) {
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
            // Rects, `Other`s, and cross-kind pairings are exactly what the
            // eligibility rule rejects: the joint run's first record is a
            // mismatch.
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

    use crate::geometry::{DrawScopeDefault, Size};
    use crate::{Brush, DrawScope as _};

    /// Records one MEGA-shaped frame: `rings` rings of `per_ring` arcs, each
    /// ring rotated by its own step × `frame`, breathing scale applied to
    /// every radius, plus `tail` dynamic circles whose count varies.
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
            // Dynamic entities: different positions every frame.
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
        // The partition frame itself emits the capture: snapshot == current,
        // so every span is capture:true under an identity transform.
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
        // The tail circles are dynamic.
        assert!(spans
            .iter()
            .any(|span| matches!(span, ReplaySpan::Dynamic { .. })));
        // Retained spans carry the per-ring rotations, not a shared one.
        let transforms: Vec<RecordTransform> = spans
            .iter()
            .filter_map(|span| match span {
                ReplaySpan::Retained { transform, .. } => Some(*transform),
                _ => None,
            })
            .collect();
        assert!(transforms.windows(2).any(|w| w[0].angle != w[1].angle));
    }

    #[test]
    fn entity_churn_between_frames_still_retains_rings() {
        let mut state = CommandReplayState::default();
        state.advance(&ring_frame(2, 400, 0, 8));
        state.advance(&ring_frame(2, 400, 1, 13)); // tail length changed
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
            // Twinkle: 40 dots change color every frame, geometry untouched.
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
        // A brick hit: one entry in the middle ring changes sweep.
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
        // Both pieces address the SAME captured slot — a split never
        // recaptures, it re-addresses: the suffix starts one record past
        // the prefix within the capture.
        assert_eq!(retained[0].0, retained[1].0);
        assert_eq!(retained[0].1, 0);
        assert_eq!(retained[1].1, 451);
        assert!(retained.iter().all(|(_, _, capture)| !capture));
        assert_eq!(dynamic, 1, "only the changed record goes dynamic");
        assert_eq!(state.stats(), (0, 1), "one split, no deaths");

        // The pieces keep retaining on later frames, the changed record's
        // slot staying dynamic between them.
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
        // The middle ring changes shape permanently: its segment dies, and
        // only a recapture can watch the new shape.
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

    /// The naive per-entry walk [`match_span`] replaced: decode both
    /// sides' views at every offset, dispatch on the pair, decode again for
    /// a recolor's color. Kept verbatim as the reference the run-decomposed
    /// path is checked against, verdict for verdict, recolor for recolor.
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

    /// One mixed frame, in tape order: a run of arcs, a non-circular round
    /// rect, a solid rect, a gradient rect (an `Other` entry), a run of
    /// circles, a second run of arcs, and a closing pair of circles — every
    /// run-breaking shape on one tape, PLUS matchable runs that directly
    /// follow another matchable run (circles→arcs and arcs→circles). Those
    /// adjacencies matter: a span entered mid-tape reaches its second run
    /// at a non-zero span offset, so a recolor there catches any confusion
    /// between run-relative and span-relative offsets. `t` moves the
    /// movable content so a span match under `t` sees Exact/Recolor
    /// entries, not wall-to-wall mismatches; `recolored` repaints one entry
    /// in each matchable run.
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
        // A run read from the middle is that run's remainder, and a kind
        // that leaves and returns starts a NEW run — index continuity
        // across the gap must not fuse the two stretches.
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
        // A genuine geometry change inside the second arc run, and a
        // NaN-carrying record right after it: both must fail through both
        // paths at the same offset.
        current_rec.arcs[11].sweep_angle *= 3.0;
        current_rec.arcs[12].start_angle = f32::NAN;
        let current = TypedRecords::from(&current_rec);
        let snapshot = TypedRecords::from(&snapshot_rec);
        let n = current_rec.tape.len();
        assert_eq!(n, snapshot_rec.tape.len());
        assert_eq!(n, 22);
        let mut fast: Vec<(u32, Color)> = Vec::new();
        let mut naive: Vec<(u32, Color)> = Vec::new();
        // Every (start, seg_start) pairing — aligned, shifted into other
        // runs, cross-kind — at several lengths including the longest one
        // both sides can carry.
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
        // The sweep must actually exercise the positive paths, not agree
        // on wall-to-wall mismatches: the aligned leading arc run matches
        // whole with its recolor, the aligned circles with theirs, and a
        // span crossing circles into the second arc run carries recolors
        // from BOTH runs — the arc one at span offset 4 (run offset 1), the
        // pairing any run-relative recolor bookkeeping would get wrong —
        // then stops exactly at the changed record. A span crossing the
        // last arc into the closing circles pins the same offset arithmetic
        // for the round-rect loop.
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
        // A NaN circle, both as a poisoned position (still circle-eligible,
        // fails the point comparison) and as poisoned extents (loses circle
        // eligibility itself).
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
        // And at span level: the poisoned record ends the clean prefix at
        // the same offset through the run-decomposed path and the naive
        // walk.
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

    /// A real multi-threaded executor for the equivalence test: lane 0 is
    /// the caller, the rest are scoped threads, jobs stride across lanes —
    /// the same distribution the renderer's frame pool uses.
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
        // Every verification path in one long churning sequence: multi-ring
        // retention under rotation, tail churn, twinkle recolors, brick-hit
        // single-record changes (the pooled pass commits the segments
        // before the failure and hands the serial machinery the failure
        // point), a multi-segment change, whole-ring deaths behind a
        // committed prefix, and coverage collapses that force re-snapshots
        // and fresh partitions mid-sequence.
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
                    // A brick hit: one record inside the middle ring. The
                    // pooled pass fails there, commits the ring before it,
                    // and the serial machinery splits from the failure.
                    recording.arcs[450].sweep_angle = 0.15;
                }
                8 => {
                    // Changes in the first and last rings at once: the
                    // first segment fails, so the pooled prefix is empty
                    // and the serial walk decides the whole frame.
                    recording.arcs[100].sweep_angle = 0.15;
                    recording.arcs[750].sweep_angle = 0.15;
                }
                12 => {
                    // A hit inside a segment created by the frame-5 split.
                    recording.arcs[500].sweep_angle = 0.15;
                }
                16..=39 => {
                    // The last ring changes shape wholesale and stays
                    // changed: its segment dies (no candidate even
                    // probes) behind the still-committing leading rings,
                    // and whatever coverage bookkeeping decides — death or
                    // collapse into a re-snapshot — both paths must agree.
                    for arc in &mut recording.arcs[600..900] {
                        arc.sweep_angle = 0.06;
                    }
                }
                40..=45 => {
                    // Nearly everything changes: coverage collapses below
                    // the floor, the state re-snapshots and re-partitions
                    // mid-sequence, then retains the changed shape.
                    for arc in &mut recording.arcs[150..900] {
                        arc.sweep_angle = 0.08;
                    }
                }
                52 => {
                    // A brick hit against the post-collapse capture.
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
