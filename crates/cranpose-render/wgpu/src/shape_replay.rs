//! Similarity-replay detection for large shape runs.
//!
//! A scene that redraws thousands of primitives every frame usually is not
//! drawing new content: MEGA-class boss scenes re-issue ~17k arcs whose only
//! frame-over-frame change is a per-ring rotation plus a global breathing
//! scale, both about one fixed center — a similarity transform baked into
//! every primitive's values by the game, invisible to transform-level
//! caching. This module recovers it from the primitive stream itself: it
//! snapshots a run once, partitions it into contiguous segments that move as
//! one similarity transform, has the renderer retain each segment's
//! converted GPU form in a replay slot, and on later frames verifies the
//! incoming entries against the snapshot and replaces whole segments with a
//! single [`DrawOpKind::Retained`](crate::scene::DrawOpKind) op — the
//! per-shape emit/record/convert/upload pipeline never sees those shapes
//! again. On a watch-class core that pipeline costs ~14 µs per primitive;
//! the verification here costs nanoseconds per primitive.
//!
//! Solid-brush color changes (twinkling and hue-shimmering ring dots) do not
//! break a segment: they become 16-byte color patches into the retained
//! buffer. Anything else — new geometry, changed sweep, a clip, a non-SrcOver
//! blend, a changed layer context — fails verification and drops the run
//! back to the normal pipeline, which re-snapshots and re-captures over the
//! following two frames. Correctness therefore never depends on the detector
//! being right about stability; a wrong guess costs a frame of normal
//! rendering, never a wrong pixel.
//!
//! Everything here is CPU state on the render thread; the flush-side driver
//! lives with the shape-run machinery in
//! [`normalized_scene`](crate::normalized_scene). GPU resources live in the
//! renderer's replay slots; the two sides talk through the pending
//! capture/patch/release queues the renderer drains once per frame.

use crate::scene::SimilarityTransform;
use cranpose_ui_graphics::{Brush, CornerRadii, FxHasher, GraphicsLayer, Point, Rect, Stroke};
use std::cell::RefCell;
use std::hash::{Hash, Hasher};

/// Below this many entries a stable stretch is not worth a retained draw.
pub(crate) const MIN_SEGMENT_ENTRIES: usize = 128;
/// Chains longer than this split into multiple captures, bounding the blast
/// radius of any one entry going dynamic later: a segment split keeps its
/// halves but a full segment death drops the whole span back to the normal
/// pipeline.
pub(crate) const MAX_SEGMENT_ENTRIES: usize = 2048;
/// Retained ops per frame are capped by the transform buffer's slot count;
/// segments past the cap emit dynamically for the frame. Mirrors the
/// renderer's `MAX_REPLAY_SLOTS`.
pub(crate) const MAX_RETAINED_OPS: usize = 128;
/// Below this many entries a run is not worth watching at all.
pub(crate) const MIN_REPLAY_RUN_ENTRIES: usize = 1024;
/// When live replay coverage sinks below this fraction of the run — segment
/// deaths and splits have eroded the table — a full rebuild re-partitions
/// from scratch.
pub(crate) const MIN_COVERAGE_FRACTION: f32 = 0.5;
/// Frames between coverage-triggered rebuilds. A heavily-animating phase
/// (boss intros) kills segments faster than partitions can rebuild them;
/// throttling the rebuild keeps those phases at partial coverage instead of
/// burning every third frame on snapshot + capture churn. Context changes
/// (fingerprint, root scale) bypass the cooldown — those are correctness.
pub(crate) const REBUILD_COOLDOWN_FRAMES: u64 = 30;
/// Relative tolerance for similarity verification. Covers float noise in the
/// game's own per-frame trigonometry; a real content change is orders of
/// magnitude larger.
const REL_EPS: f32 = 2e-3;
/// Absolute tolerance for positions/angles near zero, in logical px/radians.
const ABS_EPS: f32 = 2e-2;
/// How far past the expected position a segment anchor may drift when the
/// dynamic spans between segments change length.
pub(crate) const RESYNC_WINDOW: usize = 1024;
/// Entries probed under a candidate anchor transform before committing to a
/// full-segment verification.
pub(crate) const ANCHOR_PROBE_ENTRIES: usize = 4;
/// Patch-queue size that indicates the drain path is not running (retained
/// draws culled for many consecutive frames); the state rebuilds instead of
/// growing without bound.
const MAX_PENDING_PATCHES: usize = 1 << 16;

/// Geometry of one snapshotted entry, in logical layer-local units.
#[derive(Clone, Copy, Debug)]
pub(crate) enum SnapshotShape {
    /// A circular round-rect (corner radius == half extent): the one rect
    /// family that stays itself under rotation about an external pivot.
    Circle {
        center: Point,
        diameter: f32,
        stroke_width: Option<f32>,
    },
    Arc {
        center: Point,
        radius: f32,
        inner_radius: f32,
        start_angle: f32,
        sweep_angle: f32,
        stroke_width: Option<f32>,
    },
}

#[derive(Clone, Debug)]
pub(crate) struct SnapshotEntry {
    pub shape: SnapshotShape,
    pub brush: Brush,
}

/// One replayable stretch of the run: its entries in capture-frame space and
/// the renderer slot holding their converted GPU form. Splits leave siblings
/// sharing one slot, each covering its own shape range of the capture.
pub(crate) struct ReplaySegment {
    pub entries: Vec<SnapshotEntry>,
    /// Bounds covering the entries at capture, logical units.
    pub bounds: Rect,
    /// Renderer slot, once the pending capture has been honored.
    pub slot: Option<u32>,
    /// This segment's first shape within the slot's capture.
    pub slot_offset: u32,
}

/// A capture request for one segment: the shape range the capture frame's
/// normal emission pushed for exactly that segment's entries, in order.
pub(crate) struct PendingCapture {
    pub segment: usize,
    pub shape_start: usize,
    pub shape_count: usize,
}

/// A pending recolor of one retained shape: a new brush whose geometry is
/// identical to the captured one. Solid brushes rewrite the shape's 16-byte
/// color; gradients also rewrite their stop span in the slot's gradient
/// buffer.
pub(crate) struct PendingBrushPatch {
    pub slot: u32,
    pub shape_index: u32,
    pub brush: Brush,
}

/// Renderer slot state for one identity-fed capture, keyed by the
/// (command, slot) pair the scene builder's verifier stamped on the span.
/// Unlike the flat detector's segments, liveness is driven by the graph:
/// spans stop referencing a key and the slot ages out.
pub(crate) struct FeedSlot {
    pub gpu_slot: u32,
    /// Emission context at capture; a differing context falls back to
    /// ordinary drawing (the captured shapes bake the old context in).
    pub fingerprint: u64,
    /// The ambient clip the capture was emitted under; replay must keep the
    /// transformed span inside it (the baked clip moves with the shapes).
    pub capture_clip: Option<Rect>,
    /// Frame that last drew from this slot, for aging out.
    pub last_referenced: u64,
}

/// A capture request from the identity feed: the shape range this frame's
/// ordinary emission pushed for one capture-marked span.
pub(crate) struct PendingFeedCapture {
    pub key: (cranpose_render_common::graph::DrawCommandId, u32),
    pub shape_start: usize,
    pub shape_count: usize,
    pub fingerprint: u64,
    pub capture_clip: Option<Rect>,
}

/// Frames a feed slot may go unreferenced before its buffers are released.
/// Long enough to ride out a recapture cycle, short enough that a vanished
/// command frees its slots within a couple of seconds.
pub(crate) const FEED_SLOT_IDLE_FRAMES: u64 = 120;

/// Where the detector is in its snapshot → partition → replay cycle.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub(crate) enum ReplayPhase {
    /// Nothing snapshotted; the next big run is snapshotted as-is.
    #[default]
    Idle,
    /// A full-run snapshot exists; the next matching run is diffed against
    /// it to partition stable segments and request their captures.
    Snapshotted,
    /// A segment table is live; runs verify against it and replay.
    Active,
}

#[derive(Default)]
pub(crate) struct ShapeReplayState {
    /// Set by the renderer each frame; false skips detection entirely
    /// (uniform-mode shape batches, slot exhaustion, non-direct scenes).
    pub supported: bool,
    /// Frame ordinal, bumped by the renderer at collection start.
    pub frame: u64,
    /// Root scale for the frame being collected; retained transforms are in
    /// device pixels while run entries are logical.
    pub root_scale: f32,
    /// One run per frame gets watched: the first one big enough to matter.
    pub big_run_claimed: bool,
    pub phase: ReplayPhase,
    /// A miss was seen while replaying; the next frame re-bootstraps from
    /// scratch instead of replaying (set only after retained ops for the
    /// frame were already pushed, when slots can no longer be released).
    pub rebuild_pending: bool,
    /// Hash of the emission context the snapshot was taken under; any drift
    /// (layer bounds, clip, alpha, motion flags) forces a rebuild.
    pub fingerprint: u64,
    /// The ambient clip every captured shape was emitted under. Replay must
    /// keep each transformed segment inside it, because the retained batch
    /// evaluates its baked clip in capture space, where it moves with the
    /// shapes instead of staying fixed on screen.
    pub capture_clip: Option<Rect>,
    /// Rotation/scale pivot shared by the whole run, layer-local units.
    pub center: Point,
    /// Full-run lockstep snapshot (`Snapshotted` phase); `None` entries are
    /// shapes replay cannot carry (plain rects, clips, non-SrcOver blends).
    pub snapshot: Vec<Option<SnapshotEntry>>,
    pub segments: Vec<ReplaySegment>,
    /// Live references per renderer slot — split siblings share one slot,
    /// which is released only when its last segment dies.
    pub slot_refs: std::collections::HashMap<u32, u32, cranpose_ui_graphics::FxBuildHasher>,
    pub captured_root_scale: Option<f32>,
    /// Frame of the last coverage-triggered rebuild, for the cooldown.
    pub last_rebuild_frame: u64,
    /// Cumulative diagnostics, reported periodically under `OB_LOG`.
    pub stat_deaths: u64,
    pub stat_splits: u64,
    pub stat_patches: u64,
    /// Queues drained by the renderer once per frame.
    pub pending_captures: Vec<PendingCapture>,
    pub pending_patches: Vec<PendingBrushPatch>,
    pub pending_releases: Vec<u32>,
    /// Identity-fed retained slots (see [`FeedSlot`]) and their capture
    /// queue, driven by [`CommandReplayFrame`]s the graph carries instead
    /// of flush-time similarity detection.
    pub feed_slots: std::collections::HashMap<
        (cranpose_render_common::graph::DrawCommandId, u32),
        FeedSlot,
        cranpose_ui_graphics::FxBuildHasher,
    >,
    pub pending_feed_captures: Vec<PendingFeedCapture>,
}

thread_local! {
    pub(crate) static SHAPE_REPLAY: RefCell<ShapeReplayState> =
        RefCell::new(ShapeReplayState::default());
}

/// Kill switch: `CRANPOSE_SIMILARITY_REPLAY=0` disables detection without a
/// rebuild, for A/B parity and performance comparisons. Read per frame (not
/// cached) so a comparison can flip it mid-process; one environment lookup
/// per frame is noise.
pub(crate) fn replay_enabled() -> bool {
    std::env::var("CRANPOSE_SIMILARITY_REPLAY").as_deref() != Ok("0")
}

/// Opt-in switch for the identity feed while it runs alongside the flat
/// detector: `CRANPOSE_COMMAND_FEED=1` makes runs carrying a replay frame
/// draw from identity-keyed slots; anything else leaves the flat detector
/// in charge. Flips to default-on (and then the flag dies with the flat
/// detector) once parity is proven on the game.
pub(crate) fn command_feed_enabled() -> bool {
    std::env::var("CRANPOSE_COMMAND_FEED").as_deref() == Ok("1")
}

/// Test/diagnostic view of the identity feed on this thread: live feed
/// slots and lifetime patch count.
#[doc(hidden)]
pub fn feed_live_stats() -> (usize, u64) {
    SHAPE_REPLAY.with(|state| {
        let state = state.borrow();
        (state.feed_slots.len(), state.stat_patches)
    })
}

/// Test/diagnostic view of the live replay state on this thread: segments
/// currently held, lifetime patch count, frames seen, whether the last frame
/// supported replay, and the phase (0 idle / 1 snapshotted / 2 active).
#[doc(hidden)]
pub fn live_stats() -> (usize, u64, u64, bool, u8) {
    SHAPE_REPLAY.with(|state| {
        let state = state.borrow();
        (
            state.segments.len(),
            state.stat_patches,
            state.frame,
            state.supported,
            match state.phase {
                ReplayPhase::Idle => 0,
                ReplayPhase::Snapshotted => 1,
                ReplayPhase::Active => 2,
            },
        )
    })
}

impl ShapeReplayState {
    /// Drops all replay state, queueing the release of any live slots. Only
    /// safe while the current frame has pushed no retained ops — the renderer
    /// frees released slots before it encodes.
    pub(crate) fn retire_all(&mut self) {
        self.segments.clear();
        for (slot, _) in self.slot_refs.drain() {
            self.pending_releases.push(slot);
        }
        self.snapshot.clear();
        self.pending_captures.clear();
        self.pending_patches.clear();
        self.phase = ReplayPhase::Idle;
        self.rebuild_pending = false;
        self.captured_root_scale = None;
    }

    /// Drops one segment's claim on its slot, releasing the slot when this
    /// was its last claimant.
    pub(crate) fn drop_slot_ref(&mut self, slot: u32) {
        match self.slot_refs.get_mut(&slot) {
            Some(refs) if *refs > 1 => *refs -= 1,
            Some(_) => {
                self.slot_refs.remove(&slot);
                self.pending_releases.push(slot);
            }
            None => {}
        }
    }

    /// Renderer-side frame handshake, called once when a scene collection
    /// begins. `supported` is false whenever this frame cannot host retained
    /// draws; a root-scale change retires everything because the slots bake
    /// device pixels.
    pub(crate) fn begin_frame(&mut self, supported: bool, root_scale: f32) {
        self.frame = self.frame.wrapping_add(1);
        self.big_run_claimed = false;
        let feed_scale_changed = !self.feed_slots.is_empty() && self.root_scale != root_scale;
        self.root_scale = root_scale;
        self.supported = supported && replay_enabled();
        let scale_changed = self
            .captured_root_scale
            .is_some_and(|captured| captured != root_scale);
        if (!self.supported && self.phase != ReplayPhase::Idle)
            || scale_changed
            || self.pending_patches.len() > MAX_PENDING_PATCHES
        {
            self.retire_all();
        }
        if (!self.supported && !self.feed_slots.is_empty()) || feed_scale_changed {
            self.retire_feed();
        }
    }

    /// Releases every identity-fed slot and moves the feed to a fresh
    /// epoch, so scene building restarts each command's verification
    /// instead of referencing buffers that no longer exist.
    pub(crate) fn retire_feed(&mut self) {
        for (_, slot) in self.feed_slots.drain() {
            self.pending_releases.push(slot.gpu_slot);
        }
        self.pending_feed_captures.clear();
        cranpose_render_common::scene_builder::clear_retained_slot_confirmations();
        crate::pipeline::bump_retained_feed_generation();
    }
}

/// The per-frame transform of one matched segment, relative to its capture.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct SegmentTransform {
    pub scale: f32,
    pub angle: f32,
}

impl SegmentTransform {
    pub(crate) fn to_similarity(self, center: Point, root_scale: f32) -> SimilarityTransform {
        SimilarityTransform::new(
            [center.x * root_scale, center.y * root_scale],
            self.angle,
            self.scale,
        )
    }

    fn apply(&self, center: Point, p: Point) -> Point {
        let (sin, cos) = self.angle.sin_cos();
        let dx = p.x - center.x;
        let dy = p.y - center.y;
        Point::new(
            center.x + (dx * cos - dy * sin) * self.scale,
            center.y + (dx * sin + dy * cos) * self.scale,
        )
    }

    /// Axis-aligned bounds of `bounds` after the transform.
    pub(crate) fn apply_to_bounds(&self, center: Point, bounds: Rect) -> Rect {
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

/// Conservative logical-space bounds of a span of snapshot entries, for
/// segments created by splits (which have no emitted rects to union). Arcs
/// bound by their full outer circle — loose, but visibility bounds only need
/// to contain the content.
pub(crate) fn entries_bounds(entries: &[SnapshotEntry]) -> Rect {
    let mut min_x = f32::INFINITY;
    let mut min_y = f32::INFINITY;
    let mut max_x = f32::NEG_INFINITY;
    let mut max_y = f32::NEG_INFINITY;
    for entry in entries {
        let (center, reach) = match entry.shape {
            SnapshotShape::Circle {
                center,
                diameter,
                stroke_width,
            } => (center, diameter * 0.5 + stroke_width.unwrap_or(0.0)),
            SnapshotShape::Arc {
                center,
                radius,
                stroke_width,
                ..
            } => (center, radius + stroke_width.unwrap_or(0.0)),
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

pub(crate) fn rect_contains(outer: Rect, inner: Rect) -> bool {
    inner.x >= outer.x
        && inner.y >= outer.y
        && inner.x + inner.width <= outer.x + outer.width
        && inner.y + inner.height <= outer.y + outer.height
}

/// Whether corner radii + extents describe a circle (radius == half extent
/// on every corner).
pub(crate) fn is_circle(rect: Rect, radii: CornerRadii) -> bool {
    let half = rect.width * 0.5;
    close_rel(rect.width, rect.height)
        && close_rel(radii.top_left, half)
        && close_rel(radii.top_right, half)
        && close_rel(radii.bottom_right, half)
        && close_rel(radii.bottom_left, half)
}

pub(crate) fn stroke_width(stroke: &Option<Stroke>) -> Option<f32> {
    stroke.as_ref().map(|s| s.width)
}

/// Similarity-invariant compatibility of a run entry with a segment anchor,
/// used to re-locate segments when dynamic spans change length. Brushes are
/// deliberately excluded — a twinkling or hue-shifted anchor must still
/// re-anchor its segment. False positives cost nothing but a failed probe.
pub(crate) fn anchor_compatible(current: &SnapshotShape, anchor: &SnapshotShape) -> bool {
    match (current, anchor) {
        (
            SnapshotShape::Arc {
                sweep_angle: s_now,
                stroke_width: w_now,
                ..
            },
            SnapshotShape::Arc {
                sweep_angle: s_then,
                stroke_width: w_then,
                ..
            },
        ) => close_rel(*s_now, *s_then) && w_now.is_some() == w_then.is_some(),
        (
            SnapshotShape::Circle {
                stroke_width: w_now,
                ..
            },
            SnapshotShape::Circle {
                stroke_width: w_then,
                ..
            },
        ) => w_now.is_some() == w_then.is_some(),
        _ => false,
    }
}

/// The result of verifying one incoming entry against its snapshot under a
/// segment transform. `Recolor` means the geometry matched and only brush
/// colors moved — replayable with a patch.
pub(crate) enum EntryMatch {
    Exact,
    Recolor,
    Mismatch,
}

/// Whether two brushes are equal apart from color changes. Solid brushes may
/// change color freely; gradients may change stop colors as long as their
/// geometry (start/end/center/radius/tile mode) and stop positions hold —
/// twinkling ring dots recolor their radial glows without ever moving them.
fn brush_match(current: &Brush, snapshot: &Brush) -> EntryMatch {
    // One walk decides Exact vs Recolor vs Mismatch. A full equality check
    // first would re-compare geometry and stops on every recoloring entry —
    // and a slow device recolors thousands per frame.
    let recolorable = match (current, snapshot) {
        (Brush::Solid(now), Brush::Solid(then)) => {
            if now == then {
                return EntryMatch::Exact;
            }
            true
        }
        (
            Brush::LinearGradient {
                colors: c_now,
                stops: s_now,
                start: start_now,
                end: end_now,
                tile_mode: t_now,
            },
            Brush::LinearGradient {
                colors: c_then,
                stops: s_then,
                start: start_then,
                end: end_then,
                tile_mode: t_then,
            },
        ) => {
            if c_now.len() == c_then.len()
                && s_now == s_then
                && start_now == start_then
                && end_now == end_then
                && t_now == t_then
            {
                if c_now == c_then {
                    return EntryMatch::Exact;
                }
                true
            } else {
                false
            }
        }
        (
            Brush::RadialGradient {
                colors: c_now,
                stops: s_now,
                center: center_now,
                radius: r_now,
                tile_mode: t_now,
            },
            Brush::RadialGradient {
                colors: c_then,
                stops: s_then,
                center: center_then,
                radius: r_then,
                tile_mode: t_then,
            },
        ) => {
            if c_now.len() == c_then.len()
                && s_now == s_then
                && center_now == center_then
                && r_now == r_then
                && t_now == t_then
            {
                if c_now == c_then {
                    return EntryMatch::Exact;
                }
                true
            } else {
                false
            }
        }
        (
            Brush::SweepGradient {
                colors: c_now,
                stops: s_now,
                center: center_now,
            },
            Brush::SweepGradient {
                colors: c_then,
                stops: s_then,
                center: center_then,
            },
        ) => {
            if c_now.len() != c_then.len() || s_now != s_then || center_now != center_then {
                false
            } else if c_now == c_then {
                return EntryMatch::Exact;
            } else {
                true
            }
        }
        _ => false,
    };
    if recolorable {
        EntryMatch::Recolor
    } else {
        EntryMatch::Mismatch
    }
}

/// Verifies geometry + brush of `current` against `snapshot` under `t`.
pub(crate) fn match_entry(
    current: &SnapshotShape,
    current_brush: &Brush,
    snapshot: &SnapshotEntry,
    center: Point,
    t: SegmentTransform,
) -> EntryMatch {
    let geometry_ok = match (current, &snapshot.shape) {
        (
            SnapshotShape::Arc {
                center: c_now,
                radius: r_now,
                inner_radius: i_now,
                start_angle: a_now,
                sweep_angle: s_now,
                stroke_width: w_now,
            },
            SnapshotShape::Arc {
                center: c_then,
                radius: r_then,
                inner_radius: i_then,
                start_angle: a_then,
                sweep_angle: s_then,
                stroke_width: w_then,
            },
        ) => {
            close_point(*c_now, *c_then)
                && close_point(*c_now, center)
                && close_rel(*r_now, r_then * t.scale)
                && close_rel(*i_now, i_then * t.scale)
                && close_angle(*a_now, a_then + t.angle)
                && close_rel(*s_now, *s_then)
                && match (w_now, w_then) {
                    (None, None) => true,
                    (Some(now), Some(then)) => close_rel(*now, then * t.scale),
                    _ => false,
                }
        }
        (
            SnapshotShape::Circle {
                center: c_now,
                diameter: d_now,
                stroke_width: w_now,
            },
            SnapshotShape::Circle {
                center: c_then,
                diameter: d_then,
                stroke_width: w_then,
            },
        ) => {
            close_point(*c_now, t.apply(center, *c_then))
                && close_rel(*d_now, d_then * t.scale)
                && match (w_now, w_then) {
                    (None, None) => true,
                    (Some(now), Some(then)) => close_rel(*now, then * t.scale),
                    _ => false,
                }
        }
        _ => false,
    };
    if !geometry_ok {
        return EntryMatch::Mismatch;
    }
    brush_match(current_brush, &snapshot.brush)
}

/// How far apart two entries' implied per-frame transforms may sit while
/// still being grouped into one segment at partition time. Much tighter than
/// the verification epsilons: entries of one ring share literally the same
/// baked rotation step (float rounding apart), while neighboring rings
/// differ by their speed difference — which accumulates against the capture
/// every frame after, so grouping across it would make segments that decay
/// into misses.
const GROUP_SCALE_EPS: f32 = 1e-4;
const GROUP_ANGLE_EPS: f32 = 2e-4;

/// Whether an entry's own implied transform is tightly consistent with a
/// chain's anchor transform. `pinned` marks transforms whose angle is
/// meaningful — an on-pivot circle pins no rotation and joins any chain.
pub(crate) fn transforms_group(
    entry: SegmentTransform,
    entry_pinned: bool,
    anchor: SegmentTransform,
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

/// Derives the segment transform from an anchor pair. Arcs pin both scale
/// and rotation exactly; circles pin scale, plus rotation from their orbit
/// when they sit measurably off the pivot. The second value is that
/// pinnedness — see [`transforms_group`].
pub(crate) fn anchor_transform_pinned(
    current: &SnapshotShape,
    snapshot: &SnapshotShape,
    center: Point,
) -> Option<(SegmentTransform, bool)> {
    match (current, snapshot) {
        (SnapshotShape::Arc { .. }, SnapshotShape::Arc { .. }) => {
            anchor_transform(current, snapshot, center).map(|t| (t, true))
        }
        (SnapshotShape::Circle { .. }, SnapshotShape::Circle { center: c_then, .. }) => {
            let dx = c_then.x - center.x;
            let dy = c_then.y - center.y;
            let pinned = dx * dx + dy * dy > 1.0;
            anchor_transform(current, snapshot, center).map(|t| (t, pinned))
        }
        _ => None,
    }
}

pub(crate) fn anchor_transform(
    current: &SnapshotShape,
    snapshot: &SnapshotShape,
    center: Point,
) -> Option<SegmentTransform> {
    match (current, snapshot) {
        (
            SnapshotShape::Arc {
                radius: r_now,
                start_angle: a_now,
                ..
            },
            SnapshotShape::Arc {
                radius: r_then,
                start_angle: a_then,
                ..
            },
        ) => {
            if *r_then <= f32::EPSILON {
                return None;
            }
            Some(SegmentTransform {
                scale: r_now / r_then,
                angle: a_now - a_then,
            })
        }
        (
            SnapshotShape::Circle {
                center: c_now,
                diameter: d_now,
                ..
            },
            SnapshotShape::Circle {
                center: c_then,
                diameter: d_then,
                ..
            },
        ) => {
            if *d_then <= f32::EPSILON {
                return None;
            }
            let scale = d_now / d_then;
            let then_dx = c_then.x - center.x;
            let then_dy = c_then.y - center.y;
            let orbit = (then_dx * then_dx + then_dy * then_dy).sqrt();
            let angle = if orbit > 1.0 {
                let now_dx = c_now.x - center.x;
                let now_dy = c_now.y - center.y;
                (now_dy.atan2(now_dx)) - (then_dy.atan2(then_dx))
            } else {
                0.0
            };
            Some(SegmentTransform { scale, angle })
        }
        _ => None,
    }
}

/// Whether the layer context can host replay at all: the shape-params affine
/// must be the identity (translation lives in `layer_bounds`, which the
/// fingerprint covers) with no quad-deforming rotation and no color filter,
/// and draws must carry no snap anchor a retained batch would skip.
pub(crate) fn layer_supports_replay(layer: &GraphicsLayer) -> bool {
    layer.scale == 1.0
        && layer.scale_x == 1.0
        && layer.scale_y == 1.0
        && layer.translation_x == 0.0
        && layer.translation_y == 0.0
        && layer.rotation_x == 0.0
        && layer.rotation_y == 0.0
        && layer.rotation_z == 0.0
        && layer.color_filter.is_none()
}

/// Hash of every emission-context input that must hold constant between the
/// capture frame and each replayed frame.
pub(crate) fn context_fingerprint(
    layer_bounds: Rect,
    visual_clip: Option<Rect>,
    layer_alpha: f32,
    motion: bool,
) -> u64 {
    let mut hasher = FxHasher::default();
    let rect_bits = |rect: Rect, hasher: &mut FxHasher| {
        rect.x.to_bits().hash(hasher);
        rect.y.to_bits().hash(hasher);
        rect.width.to_bits().hash(hasher);
        rect.height.to_bits().hash(hasher);
    };
    rect_bits(layer_bounds, &mut hasher);
    match visual_clip {
        Some(clip) => {
            1u8.hash(&mut hasher);
            rect_bits(clip, &mut hasher);
        }
        None => 0u8.hash(&mut hasher),
    }
    layer_alpha.to_bits().hash(&mut hasher);
    motion.hash(&mut hasher);
    hasher.finish()
}

#[cfg(test)]
mod tests {
    use super::*;
    use cranpose_ui_graphics::Color;

    fn arc(center: Point, radius: f32, start: f32) -> SnapshotShape {
        SnapshotShape::Arc {
            center,
            radius,
            inner_radius: radius * 0.9,
            start_angle: start,
            sweep_angle: 0.25,
            stroke_width: Some(radius * 0.01),
        }
    }

    #[test]
    fn arcs_match_under_their_ring_rotation_and_breathing_scale() {
        let center = Point::new(204.0, 204.0);
        let snapshot = SnapshotEntry {
            shape: arc(center, 160.0, 1.0),
            brush: Brush::solid(Color(0.2, 0.4, 0.6, 0.5)),
        };
        let current = SnapshotShape::Arc {
            center,
            radius: 160.0 * 0.9994,
            inner_radius: 160.0 * 0.9 * 0.9994,
            start_angle: 1.0 + 0.0067,
            sweep_angle: 0.25,
            stroke_width: Some(160.0 * 0.01 * 0.9994),
        };
        let t = anchor_transform(&current, &snapshot.shape, center).unwrap();
        assert!((t.scale - 0.9994).abs() < 1e-5);
        assert!((t.angle - 0.0067).abs() < 1e-5);
        assert!(matches!(
            match_entry(
                &current,
                &Brush::solid(Color(0.2, 0.4, 0.6, 0.5)),
                &snapshot,
                center,
                t
            ),
            EntryMatch::Exact
        ));
        // An alpha twinkle patches instead of mismatching...
        assert!(matches!(
            match_entry(
                &current,
                &Brush::solid(Color(0.2, 0.4, 0.6, 0.75)),
                &snapshot,
                center,
                t
            ),
            EntryMatch::Recolor
        ));
        // ...but a sweep change is a real mismatch.
        let resized = SnapshotShape::Arc {
            center,
            radius: 160.0 * 0.9994,
            inner_radius: 160.0 * 0.9 * 0.9994,
            start_angle: 1.0 + 0.0067,
            sweep_angle: 0.3,
            stroke_width: Some(160.0 * 0.01 * 0.9994),
        };
        assert!(matches!(
            match_entry(
                &resized,
                &Brush::solid(Color(0.2, 0.4, 0.6, 0.5)),
                &snapshot,
                center,
                t
            ),
            EntryMatch::Mismatch
        ));
    }

    #[test]
    fn orbiting_circles_pin_the_rotation_from_their_positions() {
        let center = Point::new(204.0, 204.0);
        let delta = 0.05f32;
        let orbit = 80.0f32;
        let then = SnapshotShape::Circle {
            center: Point::new(center.x + orbit, center.y),
            diameter: 20.0,
            stroke_width: None,
        };
        let now = SnapshotShape::Circle {
            center: Point::new(
                center.x + orbit * delta.cos(),
                center.y + orbit * delta.sin(),
            ),
            diameter: 20.0,
            stroke_width: None,
        };
        let t = anchor_transform(&now, &then, center).unwrap();
        assert!((t.angle - delta).abs() < 1e-4);
        let snapshot = SnapshotEntry {
            shape: then,
            brush: Brush::solid(Color(1.0, 1.0, 1.0, 1.0)),
        };
        assert!(matches!(
            match_entry(
                &now,
                &Brush::solid(Color(1.0, 1.0, 1.0, 1.0)),
                &snapshot,
                center,
                t
            ),
            EntryMatch::Exact
        ));
    }

    #[test]
    fn transformed_bounds_cover_the_rotated_rect() {
        let center = Point::new(0.0, 0.0);
        let t = SegmentTransform {
            scale: 1.0,
            angle: std::f32::consts::FRAC_PI_2,
        };
        let bounds = t.apply_to_bounds(
            center,
            Rect {
                x: 10.0,
                y: 0.0,
                width: 10.0,
                height: 2.0,
            },
        );
        // A quarter turn clockwise in y-down space maps +X onto +Y.
        assert!((bounds.y - 10.0).abs() < 1e-4);
        assert!((bounds.height - 10.0).abs() < 1e-4);
    }

    #[test]
    fn retire_all_queues_live_slots_for_release() {
        let mut state = ShapeReplayState::default();
        state.segments.push(ReplaySegment {
            entries: Vec::new(),
            bounds: Rect {
                x: 0.0,
                y: 0.0,
                width: 1.0,
                height: 1.0,
            },
            slot: Some(7),
            slot_offset: 0,
        });
        state.slot_refs.insert(7, 1);
        state.phase = ReplayPhase::Active;
        state.retire_all();
        assert_eq!(state.pending_releases, vec![7]);
        assert_eq!(state.phase, ReplayPhase::Idle);
        assert!(state.segments.is_empty());
    }

    #[test]
    fn split_siblings_share_a_slot_until_the_last_dies() {
        let mut state = ShapeReplayState::default();
        state.slot_refs.insert(3, 2);
        state.drop_slot_ref(3);
        assert!(state.pending_releases.is_empty());
        state.drop_slot_ref(3);
        assert_eq!(state.pending_releases, vec![3]);
        state.drop_slot_ref(3);
        assert_eq!(state.pending_releases, vec![3]);
    }
}
