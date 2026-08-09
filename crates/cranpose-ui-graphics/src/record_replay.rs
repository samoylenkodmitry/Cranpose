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
    CommandRecording, Point, RecordKind, Rect, SolidArcRecord, SolidRoundRectRecord,
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

/// The similarity-checkable view of one tape entry: which typed store it
/// lives in and its index there. `None` marks entries replay cannot carry
/// (plain rects, ordinary primitives) — they break segments wherever they
/// sit.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum ReplayView {
    Arc(usize),
    RoundRect(usize),
}

/// Flattens a recording's tape into per-entry replay views plus per-kind
/// indices, so alignment and verification can address records directly.
fn build_views(recording: &CommandRecording) -> Vec<Option<ReplayView>> {
    let mut arc_idx = 0usize;
    let mut rr_idx = 0usize;
    let mut rect_idx = 0usize;
    let mut other_idx = 0usize;
    recording
        .tape
        .iter()
        .map(|kind| match kind {
            RecordKind::SolidArc => {
                let view = ReplayView::Arc(arc_idx);
                arc_idx += 1;
                Some(view)
            }
            RecordKind::SolidRoundRect => {
                let view = ReplayView::RoundRect(rr_idx);
                rr_idx += 1;
                // Non-circular round rects cannot survive rotation about an
                // external pivot; they stay dynamic.
                if circle_view(&recording.round_rects[rr_idx - 1]).is_some() {
                    Some(view)
                } else {
                    None
                }
            }
            RecordKind::SolidRect => {
                rect_idx += 1;
                let _ = rect_idx;
                None
            }
            RecordKind::Other => {
                other_idx += 1;
                let _ = other_idx;
                None
            }
        })
        .collect()
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
fn align_recordings(
    current: &CommandRecording,
    current_views: &[Option<ReplayView>],
    retained: &CommandRecording,
    retained_views: &[Option<ReplayView>],
) -> Vec<Option<usize>> {
    let pair = |i: usize, j: usize| -> bool {
        views_compatible(current, current_views[i], retained, retained_views[j])
    };
    let mut aligned = vec![None; current_views.len()];
    let (mut i, mut j) = (0usize, 0usize);
    let mut events = 0usize;
    while i < current_views.len() && j < retained_views.len() {
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
            return vec![None; current_views.len()];
        }
        let mut resynced = false;
        'search: for total in 1..=RESYNC_SPAN {
            for di in 0..=total {
                let dj = total - di;
                if i + di < current_views.len() && j + dj < retained_views.len() && pair(i + di, j + dj)
                {
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
fn range_bounds(recording: &CommandRecording, views: &[Option<ReplayView>], range: (usize, usize)) -> Rect {
    let mut min_x = f32::INFINITY;
    let mut min_y = f32::INFINITY;
    let mut max_x = f32::NEG_INFINITY;
    let mut max_y = f32::NEG_INFINITY;
    for view in views[range.0..range.1].iter().flatten() {
        let (center, reach) = match view {
            ReplayView::Arc(i) => {
                let arc = &recording.arcs[*i];
                (
                    arc.center,
                    arc.radius + arc.stroke.map(|stroke| stroke.width).unwrap_or(0.0),
                )
            }
            ReplayView::RoundRect(i) => {
                let record = &recording.round_rects[*i];
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
#[derive(Clone, Debug)]
pub struct CommandSegment {
    pub id: u32,
    pub tape_start: usize,
    pub tape_end: usize,
    /// Loose logical bounds at capture.
    pub bounds: Rect,
}

/// One span of this frame's recording, in tape order.
#[derive(Clone, Debug, PartialEq)]
pub enum ReplaySpan {
    /// The retained segment moved by `transform`; `recolors` are
    /// (segment-relative record offset, new color) patches.
    Retained {
        /// The stable [`CommandSegment::id`].
        segment: u32,
        transform: RecordTransform,
        recolors: Vec<(u32, Color)>,
        /// Segment capture bounds under this frame's transform.
        bounds: Rect,
    },
    /// Materialize these current-tape entries through the ordinary path.
    Dynamic { tape_start: usize, tape_end: usize },
}

/// What one frame of verification decided for a command.
#[derive(Debug)]
pub enum ReplayOutcome {
    /// No retention this frame: materialize the whole recording.
    AllDynamic,
    /// The interleaved retained/dynamic structure of this frame, in exact
    /// tape order.
    Spans(Vec<ReplaySpan>),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum CommandReplayPhase {
    Idle,
    Snapshotted,
    Captured,
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
    snapshot_views: Vec<Option<ReplayView>>,
    segments: Vec<CommandSegment>,
    next_segment_id: u32,
}

impl Default for CommandReplayState {
    fn default() -> Self {
        Self {
            phase: CommandReplayPhase::Idle,
            center: Point::new(0.0, 0.0),
            snapshot: CommandRecording::default(),
            snapshot_views: Vec::new(),
            segments: Vec::new(),
            next_segment_id: 0,
        }
    }
}

impl CommandReplayState {
    pub fn segments(&self) -> &[CommandSegment] {
        &self.segments
    }

    /// Advances the state machine with this frame's recording and returns
    /// what the frame can retain. Phases mirror the flat-list detector:
    /// snapshot on the first sighting, partition into
    /// transform-consistent chains on the second, verify per entry from the
    /// third on. A structural collapse or coverage erosion re-snapshots;
    /// correctness never depends on the detector being right about
    /// stability — a wrong guess costs a frame of ordinary rendering.
    pub fn advance(&mut self, current: &CommandRecording) -> ReplayOutcome {
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
            CommandReplayPhase::Snapshotted => {
                self.partition(current, center);
                ReplayOutcome::AllDynamic
            }
            CommandReplayPhase::Captured => self.verify(current),
        }
    }

    fn retire(&mut self) {
        self.phase = CommandReplayPhase::Idle;
        self.snapshot = CommandRecording::default();
        self.snapshot_views.clear();
        self.segments.clear();
    }

    fn take_snapshot(&mut self, current: &CommandRecording, center: Point) {
        self.snapshot = current.clone();
        self.snapshot_views = build_views(&self.snapshot);
        self.center = center;
        self.segments.clear();
        self.phase = CommandReplayPhase::Snapshotted;
    }

    /// Splits the recording into maximal chains of consecutive entries that
    /// moved from the snapshot by one shared similarity transform, then
    /// re-snapshots at the current values so verification always compares
    /// against the capture frame.
    fn partition(&mut self, current: &CommandRecording, center: Point) {
        let current_views = build_views(current);
        let aligned = align_recordings(
            current,
            &current_views,
            &self.snapshot,
            &self.snapshot_views,
        );
        let mut chains: Vec<(usize, usize)> = Vec::new();
        let mut i = 0;
        while i < current_views.len() {
            let (Some(view), Some(snapshot_view)) = (
                current_views[i],
                aligned[i].and_then(|j| self.snapshot_views[j]),
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
            while end < current_views.len() {
                let (Some(view), Some(snapshot_view)) = (
                    current_views[end],
                    aligned[end].and_then(|j| self.snapshot_views[j]),
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
            return;
        }
        // Re-snapshot at current values: chain ranges are current-tape
        // ranges, which the fresh snapshot preserves verbatim.
        self.take_snapshot(current, center);
        self.segments = chains
            .into_iter()
            .map(|range| {
                let id = self.next_segment_id;
                self.next_segment_id += 1;
                CommandSegment {
                    id,
                    tape_start: range.0,
                    tape_end: range.1,
                    bounds: range_bounds(&self.snapshot, &self.snapshot_views, range),
                }
            })
            .collect();
        self.phase = CommandReplayPhase::Captured;
    }

    /// Verifies this frame's recording against the capture. Each segment
    /// re-locates its anchor by searching forward from the cursor within
    /// [`RESYNC_WINDOW`] — dynamic spans between segments change length
    /// freely — probing a few entries under each candidate transform before
    /// committing to a full-span verification (a wrong candidate from a
    /// different ring fails the probe on its radii). A mismatching segment
    /// dies whole for this frame (splits are a later refinement); eroded
    /// coverage re-snapshots for the next frame.
    fn verify(&mut self, current: &CommandRecording) -> ReplayOutcome {
        let current_views = build_views(current);
        let mut spans: Vec<ReplaySpan> = Vec::new();
        let mut retained_records = 0usize;
        let mut dead_segments: Vec<usize> = Vec::new();
        let mut cursor = 0usize; // current-tape position covered so far
        // How far the current tape has drifted from the capture tape, from
        // the segments located so far. Self-similar rings make a linear
        // scan treacherous — a shifted wrong anchor can pass any short
        // probe — so candidates are tried by distance from the expected
        // position, where the true anchor almost always sits.
        let mut drift = 0isize;
        for (segment_index, segment) in self.segments.iter().enumerate() {
            let len = segment.tape_end - segment.tape_start;
            let search_end = (cursor + RESYNC_WINDOW)
                .min(current_views.len().saturating_sub(len - 1))
                .max(cursor);
            let expected_start = ((segment.tape_start as isize + drift).max(cursor as isize)
                as usize)
                .min(search_end.saturating_sub(1).max(cursor));
            let candidates = (0..RESYNC_WINDOW).flat_map(|d| {
                let after = expected_start.checked_add(d).filter(|s| *s < search_end);
                let before = if d > 0 {
                    expected_start
                        .checked_sub(d)
                        .filter(|s| *s >= cursor)
                } else {
                    None
                };
                after.into_iter().chain(before)
            });
            type Located = (usize, RecordTransform, Vec<(u32, Color)>);
            let mut located: Option<Located> = None;
            let mut attempts = 0usize;
            'search: for start in candidates {
                let (Some(view), Some(snapshot_view)) = (
                    current_views[start],
                    self.snapshot_views[segment.tape_start],
                ) else {
                    continue;
                };
                if !views_compatible(
                    current,
                    Some(view),
                    &self.snapshot,
                    Some(snapshot_view),
                ) {
                    continue;
                }
                let Some((t, _)) =
                    pair_transform(current, view, &self.snapshot, snapshot_view, self.center)
                else {
                    continue;
                };
                // Cheap rejection of wrong anchors before the full span.
                for probe in 0..ANCHOR_PROBE_RECORDS.min(len) {
                    let (Some(view), Some(snapshot_view)) = (
                        current_views[start + probe],
                        self.snapshot_views[segment.tape_start + probe],
                    ) else {
                        continue 'search;
                    };
                    if match_pair(current, view, &self.snapshot, snapshot_view, self.center, t)
                        == RecordMatch::Mismatch
                    {
                        continue 'search;
                    }
                }
                // Committed: verify the whole span. A failure may still be a
                // mislocated anchor (self-similar rings), so the search
                // resumes — a bounded number of times.
                let mut recolors: Vec<(u32, Color)> = Vec::new();
                let mut failed = false;
                for offset in 0..len {
                    let (Some(view), Some(snapshot_view)) = (
                        current_views[start + offset],
                        self.snapshot_views[segment.tape_start + offset],
                    ) else {
                        failed = true;
                        break;
                    };
                    match match_pair(current, view, &self.snapshot, snapshot_view, self.center, t)
                    {
                        RecordMatch::Exact => {}
                        RecordMatch::Recolor => {
                            let color = match view {
                                ReplayView::Arc(a) => current.arcs[a].color,
                                ReplayView::RoundRect(r) => current.round_rects[r].color,
                            };
                            recolors.push((offset as u32, color));
                        }
                        RecordMatch::Mismatch => {
                            failed = true;
                            break;
                        }
                    }
                }
                if failed {
                    attempts += 1;
                    if attempts >= MAX_COMMIT_ATTEMPTS {
                        break 'search;
                    }
                    continue;
                }
                located = Some((start, t, recolors));
                break;
            }
            let Some((span_start, t, recolors)) = located else {
                dead_segments.push(segment_index);
                continue;
            };
            drift = span_start as isize - segment.tape_start as isize;

            if span_start > cursor {
                spans.push(ReplaySpan::Dynamic {
                    tape_start: cursor,
                    tape_end: span_start,
                });
            }
            retained_records += len;
            spans.push(ReplaySpan::Retained {
                segment: segment.id,
                transform: t,
                recolors,
                bounds: t.apply_to_bounds(self.center, segment.bounds),
            });
            cursor = span_start + len;
        }
        if cursor < current.tape.len() {
            spans.push(ReplaySpan::Dynamic {
                tape_start: cursor,
                tape_end: current.tape.len(),
            });
        }

        for segment_index in dead_segments.into_iter().rev() {
            self.segments.remove(segment_index);
        }
        let retained_total: usize = self
            .segments
            .iter()
            .map(|segment| segment.tape_end - segment.tape_start)
            .sum();
        if retained_records == 0
            || (retained_total as f32) < MIN_COVERAGE_FRACTION * current.tape.len() as f32
        {
            // Erosion: re-snapshot so the next two frames re-partition.
            let center = self.center;
            self.take_snapshot(current, center);
            if retained_records == 0 {
                return ReplayOutcome::AllDynamic;
            }
        }
        ReplayOutcome::Spans(spans)
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
        assert!(matches!(
            state.advance(&ring_frame(3, 300, 1, 10)),
            ReplayOutcome::AllDynamic
        ));
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
