//! Retained replay state for the command feed.
//!
//! A scene that redraws thousands of primitives every frame usually is not
//! drawing new content: MEGA-class boss scenes re-issue ~17k arcs whose only
//! frame-over-frame change is a per-ring rotation plus a global breathing
//! scale — a similarity transform baked into every primitive's values by the
//! game. The recording layer detects it per draw command
//! ([`CommandReplayFrame`](cranpose_ui_graphics::CommandReplayFrame)) and the
//! graph carries the verified spans to collection; the renderer retains each
//! captured span's converted GPU form in a replay slot and later frames
//! replace the whole span with a single
//! [`DrawOpKind::Retained`](crate::scene::DrawOpKind) op — the per-shape
//! emit/record/convert/upload pipeline never sees those shapes again.
//!
//! Solid-brush color changes (twinkling and hue-shimmering ring dots) do not
//! break a span: they become 16-byte color patches into the retained buffer.
//! Anything the feed cannot serve falls back to the normal pipeline, which
//! re-captures over the following frames. Correctness therefore never
//! depends on retention being available; an unserved span costs a frame of
//! normal rendering, never a wrong pixel.
//!
//! Everything here is CPU state on the render thread; the flush-side driver
//! lives with the shape-run machinery in
//! [`normalized_scene`](crate::normalized_scene). GPU resources live in the
//! renderer's replay slots; the two sides talk through the pending
//! capture/patch/release queues the renderer drains once per frame.

use crate::scene::SimilarityTransform;
use cranpose_ui_graphics::{FxHasher, GraphicsLayer, Point, Rect};
use std::cell::RefCell;
use std::hash::{Hash, Hasher};

/// Retained ops per frame are capped by the transform buffer's slot count;
/// spans past the cap emit dynamically for the frame. Mirrors the
/// renderer's `MAX_REPLAY_SLOTS`.
pub(crate) const MAX_RETAINED_OPS: usize = 128;

/// A pending solid recolor of one retained shape: a bare 16-byte color
/// write into the slot's paint mirror.
#[derive(Clone, Copy)]
pub(crate) struct ColorPatch {
    pub slot: u32,
    pub shape_index: u32,
    pub color: [f32; 4],
}

/// Renderer slot state for one identity-fed capture, keyed by the
/// (command, slot) pair the scene builder's verifier stamped on the span.
/// Liveness is driven by the graph: spans stop referencing a key and the
/// slot ages out.
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
    /// Frame ordinal at queue time. The drain honors a capture only against
    /// that same frame's scene — its shape indices are meaningless in any
    /// other, and capturing there would retain wrong content under a
    /// confirmed identity.
    pub frame: u64,
}

/// Frames a feed slot may go unreferenced before its buffers are released.
/// Long enough to ride out a recapture cycle, short enough that a vanished
/// command frees its slots within a couple of seconds.
pub(crate) const FEED_SLOT_IDLE_FRAMES: u64 = 120;

#[derive(Default)]
pub(crate) struct ShapeReplayState {
    /// Set by the renderer each frame; false means this frame cannot host
    /// retained draws (uniform-mode shape batches, non-direct scenes).
    pub supported: bool,
    /// Frame ordinal, bumped by the renderer at collection start.
    pub frame: u64,
    /// Root scale for the frame being collected; retained transforms are in
    /// device pixels while run entries are logical.
    pub root_scale: f32,
    /// Lifetime recolor-patch count, reported under the diag flag.
    pub stat_patches: u64,
    /// Lifetime count of bypassed spans that could neither draw retained
    /// nor rematerialize from their command's recording — the fail-closed
    /// terminal. Every hit revokes the span's confirmation so the next
    /// build materializes it again; a nonzero steady rate is a defect.
    pub stat_remat_miss: u64,
    /// Queues drained by the renderer once per frame.
    pub pending_color_patches: Vec<ColorPatch>,
    pub pending_releases: Vec<u32>,
    /// Identity-fed retained slots (see [`FeedSlot`]) and their capture
    /// queue, driven by [`CommandReplayFrame`](cranpose_ui_graphics::CommandReplayFrame)s
    /// the graph carries.
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

/// Kill switch for the identity feed: default ON since parity was proven
/// exact on the game scene (command_feed_parity test + desktop runs);
/// `CRANPOSE_COMMAND_FEED=0` disables retention entirely for A/B
/// comparison. Read per frame (not cached) so a comparison can flip it
/// mid-process; one environment lookup per frame is noise.
pub(crate) fn command_feed_enabled() -> bool {
    std::env::var("CRANPOSE_COMMAND_FEED").as_deref() != Ok("0")
}

/// Test/diagnostic view of the identity feed on this thread: live feed
/// slots, lifetime patch count, and lifetime remat-miss count (bypassed
/// spans that could neither draw retained nor rebuild from their recording
/// — the fail-closed terminal; see `stat_remat_miss`).
#[doc(hidden)]
pub fn feed_live_stats() -> (usize, u64, u64) {
    SHAPE_REPLAY.with(|state| {
        let state = state.borrow();
        (
            state.feed_slots.len(),
            state.stat_patches,
            state.stat_remat_miss,
        )
    })
}

/// Test hook: queues a feed capture stamped with the CURRENT frame ordinal.
/// Rendering the next frame advances the ordinal before the drain runs, so
/// an injected capture is exactly the stale-frame case the drain must drop
/// without capturing or confirming.
#[doc(hidden)]
pub fn inject_feed_capture_for_tests(
    command: cranpose_render_common::graph::DrawCommandId,
    slot: u32,
    shape_start: usize,
    shape_count: usize,
) {
    SHAPE_REPLAY.with(|state| {
        let mut state = state.borrow_mut();
        let frame = state.frame;
        state.pending_feed_captures.push(PendingFeedCapture {
            key: (command, slot),
            shape_start,
            shape_count,
            fingerprint: 0,
            capture_clip: None,
            frame,
        });
    });
}

/// Test hook: how many feed captures are queued on this thread.
#[doc(hidden)]
pub fn pending_feed_capture_count_for_tests() -> usize {
    SHAPE_REPLAY.with(|state| state.borrow().pending_feed_captures.len())
}

impl ShapeReplayState {
    /// Voids every pending per-scene request. Called when the collected
    /// scene will never render (rejected collection, renderer replacement):
    /// pending feed captures reference shape indices of the scene being
    /// retired, and a later frame's drain must never capture (and confirm)
    /// another scene's shapes under their identities; pending color patches
    /// were queued for that scene's retained draws.
    pub(crate) fn retire_all(&mut self) {
        self.pending_feed_captures.clear();
        self.pending_color_patches.clear();
    }

    /// Renderer-side frame handshake, called once when a scene collection
    /// begins. `supported` is false whenever this frame cannot host retained
    /// draws; a root-scale change retires the feed because the slots bake
    /// device pixels.
    pub(crate) fn begin_frame(&mut self, supported: bool, root_scale: f32) {
        self.frame = self.frame.wrapping_add(1);
        let feed_scale_changed = !self.feed_slots.is_empty() && self.root_scale != root_scale;
        self.root_scale = root_scale;
        self.supported = supported;
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

/// The per-frame transform of one retained span, relative to its capture.
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
}

pub(crate) fn rect_contains(outer: Rect, inner: Rect) -> bool {
    inner.x >= outer.x
        && inner.y >= outer.y
        && inner.x + inner.width <= outer.x + outer.width
        && inner.y + inner.height <= outer.y + outer.height
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

    #[test]
    fn retire_all_clears_pending_feed_captures() {
        let mut state = ShapeReplayState::default();
        state.pending_feed_captures.push(PendingFeedCapture {
            key: (
                cranpose_render_common::graph::DrawCommandId {
                    node_id: 1,
                    command_index: 0,
                    placement: cranpose_render_common::style_shared::DrawPlacement::Behind,
                },
                0,
            ),
            shape_start: 0,
            shape_count: 4,
            fingerprint: 0,
            capture_clip: None,
            frame: 3,
        });
        state.retire_all();
        assert!(
            state.pending_feed_captures.is_empty(),
            "retired scenes must void their capture requests: the shape \
             indices reference a scene that will never render"
        );
    }
}
