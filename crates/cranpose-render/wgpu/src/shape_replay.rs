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
//! Everything here is CPU state on the producer side: a pure PLANNER. GPU
//! resources live in the present-side store (the renderer's replay slots),
//! and the two sides talk ONLY through typed messages: the planner emits
//! one [`ReplayFrameOps`] batch into each [`FramePacket`](crate::frame_packet::FramePacket)
//! ([`ShapeReplayState::take_frame_ops`]) and applies the store's
//! [`ReplayAck`] before the next frame's planning
//! ([`ShapeReplayState::apply_ack`]). Both directions swap-recycle their
//! buffers, so the protocol adds no per-frame allocation.

use crate::frame_packet::{ReplayAck, ReplayConfirmation, ReplayFrameOps};
use crate::scene::{ColorPatch, PendingFeedCapture, SimilarityTransform};
use cranpose_render_common::graph::DrawCommandId;
use cranpose_ui_graphics::{FxHasher, GraphicsLayer, Point, Rect};
use std::cell::RefCell;
use std::hash::{Hash, Hasher};

/// Retained ops per frame are capped by the transform buffer's slot count;
/// spans past the cap emit dynamically for the frame. Mirrors the
/// renderer's `MAX_REPLAY_SLOTS`.
pub(crate) const MAX_RETAINED_OPS: usize = 128;

/// The planner's record of one confirmed identity-fed capture, keyed by
/// the (command, slot) pair the scene builder's verifier stamped on the
/// span. `gpu_slot` arrived via [`ReplayAck`]; liveness is driven by the
/// graph: spans stop referencing a key and the slot ages out.
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

/// The planner half of a capture in flight: what [`ShapeReplayState::apply_ack`]
/// copies into the [`FeedSlot`] when the store confirms the capture. Keyed
/// like `feed_slots`; entries live exactly one plan→ack cycle.
pub(crate) struct RequestedFeedSlot {
    pub fingerprint: u64,
    pub capture_clip: Option<Rect>,
    /// The (generation, frame) of the ops batch that carried the capture,
    /// stamped by [`ShapeReplayState::take_frame_ops`] so a cancelled
    /// batch's entries — which can never be confirmed — are removable
    /// precisely ([`ShapeReplayState::reclaim_cancelled_ops`]).
    pub generation: u64,
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
    /// Queues accumulated during collection and moved whole into the
    /// frame's [`ReplayFrameOps`] by [`Self::take_frame_ops`].
    pub pending_color_patches: Vec<ColorPatch>,
    pub pending_releases: Vec<u32>,
    /// Identity-fed retained slots (see [`FeedSlot`]) and their capture
    /// queue, driven by [`CommandReplayFrame`](cranpose_ui_graphics::CommandReplayFrame)s
    /// the graph carries.
    pub feed_slots: std::collections::HashMap<
        (DrawCommandId, u32),
        FeedSlot,
        cranpose_ui_graphics::FxBuildHasher,
    >,
    pub pending_feed_captures: Vec<PendingFeedCapture>,
    /// Captures emitted into this frame's ops, awaiting the store's
    /// confirmation. [`Self::apply_ack`] promotes confirmed entries into
    /// `feed_slots` and drops the rest silently (the capture failed; the
    /// planner just won't serve them). Cleared every ack, so entries never
    /// outlive their plan→ack cycle.
    pub awaiting_confirmation: std::collections::HashMap<
        (DrawCommandId, u32),
        RequestedFeedSlot,
        cranpose_ui_graphics::FxBuildHasher,
    >,
    /// The previous batch's emptied buffers, returned by the store with
    /// its ack; [`Self::take_frame_ops`] swaps the pending queues against
    /// them so every vec keeps its high-water capacity (P4b: the queue,
    /// in-flight, and store-held vecs ping-pong, none is reallocated).
    pub recycled_ops: ReplayFrameOps,
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

/// Test hook: (queued release ids, awaiting-confirmation entries) on this
/// thread's planner — the cancellation contract's no-leak instruments.
#[doc(hidden)]
pub fn planner_replay_queue_stats_for_tests() -> (usize, usize) {
    SHAPE_REPLAY.with(|state| {
        let state = state.borrow();
        (
            state.pending_releases.len(),
            state.awaiting_confirmation.len(),
        )
    })
}

/// Test hook: the recycled-ops buffer capacities (captures, color patches,
/// releases) on this thread's planner — proof a cancelled batch's buffers
/// came back for reuse instead of being dropped.
#[doc(hidden)]
pub fn recycled_ops_capacities_for_tests() -> (usize, usize, usize) {
    SHAPE_REPLAY.with(|state| {
        let state = state.borrow();
        (
            state.recycled_ops.captures.capacity(),
            state.recycled_ops.color_patches.capacity(),
            state.recycled_ops.releases.capacity(),
        )
    })
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

    /// The GPU renderer was replaced (surface recreation, device loss):
    /// every retained slot died with it, so the feed retires wholesale
    /// (confirmations revoked, generation bumped — fail-closed BEFORE the
    /// new renderer exists), pending per-scene requests are voided, and
    /// queued release ids are cleared rather than drained into the new
    /// store — the new store starts fully free, so old ids are at best a
    /// guarded no-op there; clearing guarantees ids never cross renderers.
    /// In-flight captures can never be confirmed against the new store's
    /// universe, so the awaiting map empties too.
    pub(crate) fn renderer_replaced(&mut self) {
        self.retire_feed();
        self.retire_all();
        self.pending_releases.clear();
        self.awaiting_confirmation.clear();
    }

    /// Closes this frame's replay window and emits the frame's plan: the
    /// pending capture/patch/release queues move whole into a
    /// [`ReplayFrameOps`] for the frame's packet, swapped against the
    /// previous batch's recycled buffers so no side allocates (P4b).
    /// Called at packet build — collection for the frame is done, so
    /// `supported` drops here (later collections this frame, e.g.
    /// overlays, build windowed scenes that cannot carry retained ops);
    /// this replaces the store-side write the old drain did.
    ///
    /// Feed slots whose spans stopped arriving age out first (planner-side
    /// eviction): their releases join this frame's ops, so the store frees
    /// the buffers before this frame's captures ask, and the revocation
    /// makes the next build materialize the span again.
    pub(crate) fn take_frame_ops(&mut self, generation: u64) -> ReplayFrameOps {
        let frame = self.frame;
        let releases = &mut self.pending_releases;
        self.feed_slots.retain(|key, slot| {
            if frame.wrapping_sub(slot.last_referenced) > FEED_SLOT_IDLE_FRAMES {
                releases.push(slot.gpu_slot);
                cranpose_render_common::scene_builder::revoke_retained_slot(key.0, key.1);
                false
            } else {
                true
            }
        });
        // `take` leaves fresh empty vecs in `recycled_ops` (no allocation);
        // the capacity travels out with the batch and comes back with the
        // ack.
        let mut ops = std::mem::take(&mut self.recycled_ops);
        std::mem::swap(&mut ops.captures, &mut self.pending_feed_captures);
        std::mem::swap(&mut ops.color_patches, &mut self.pending_color_patches);
        std::mem::swap(&mut ops.releases, &mut self.pending_releases);
        for capture in &ops.captures {
            self.awaiting_confirmation.insert(
                capture.key,
                RequestedFeedSlot {
                    fingerprint: capture.fingerprint,
                    capture_clip: capture.capture_clip,
                    generation,
                    frame,
                },
            );
        }
        ops.generation = generation;
        ops.frame = frame;
        self.supported = false;
        ops
    }

    /// Applies the store's answer to this frame's ops, before the next
    /// frame's planning (and, today, before this frame renders — the same
    /// point the old in-store drain confirmed at, so observable timing is
    /// unchanged: confirmations only gate the NEXT build's bypass and
    /// `feed_slots` is only served at collect time). Each confirmation
    /// promotes its awaiting entry into `feed_slots`; a displaced slot
    /// (recapture over a live identity) is released through the NEXT
    /// frame's ops — one frame later than the old same-frame release,
    /// safe because the capture frame never draws the old slot (its span
    /// emitted ordinarily) and the pool has headroom for one lingering
    /// slot (128 slots, the MEGA scene uses 22). Unconfirmed entries drop
    /// silently. The ack's recycled buffers become the next batch's, and
    /// the drained confirmations vec returns to the caller for the store
    /// to refill (P4b: no side allocates per frame).
    pub(crate) fn apply_ack(
        &mut self,
        mut ack: ReplayAck,
        recycled: ReplayFrameOps,
    ) -> Vec<ReplayConfirmation> {
        let frame = self.frame;
        for (key, gpu_slot) in ack.confirmations.drain(..) {
            let Some(requested) = self.awaiting_confirmation.remove(&key) else {
                // Defensive: a confirmation this planner never asked for.
                // Without the requested fingerprint/clip no valid feed slot
                // can exist, but the store DID retain buffers — queue the
                // release rather than leak the slot.
                self.pending_releases.push(gpu_slot);
                continue;
            };
            if let Some(old) = self.feed_slots.insert(
                key,
                FeedSlot {
                    gpu_slot,
                    fingerprint: requested.fingerprint,
                    capture_clip: requested.capture_clip,
                    last_referenced: frame,
                },
            ) {
                // A recaptured identity replaces its old buffers whole.
                if old.gpu_slot != gpu_slot {
                    self.pending_releases.push(old.gpu_slot);
                }
            }
            // Only now may scene building skip materializing this span:
            // the retained buffer verifiably exists — in the ack's feed
            // generation's slot universe, which the confirmation is
            // stamped with so a later universe never trusts it.
            cranpose_render_common::scene_builder::confirm_retained_slot(
                key.0,
                key.1,
                ack.generation,
            );
        }
        self.awaiting_confirmation.clear();
        self.recycled_ops = recycled;
        ack.confirmations
    }

    /// Takes back a cancelled packet's [`ReplayFrameOps`] — a plan the store
    /// never consumed. The releases re-queue whole (the slot ids are still
    /// live store-side; dropping them would leak pool slots forever — the
    /// pool is 128 ids). The captures drop (no store slot was ever created),
    /// but the `awaiting_confirmation` entries [`Self::take_frame_ops`]
    /// recorded for exactly this batch can never be confirmed and must not
    /// wait for an ack that may never come (every later frame could be
    /// Surface), so they purge precisely by the batch's (generation, frame)
    /// stamp. Color patches drop (regenerated every frame). The emptied
    /// buffers recycle exactly like [`Self::apply_ack`]'s, capacity intact.
    pub(crate) fn reclaim_cancelled_ops(&mut self, mut ops: ReplayFrameOps) {
        self.pending_releases.extend_from_slice(&ops.releases);
        ops.releases.clear();
        self.awaiting_confirmation.retain(|_, requested| {
            (requested.generation, requested.frame) != (ops.generation, ops.frame)
        });
        ops.captures.clear();
        ops.color_patches.clear();
        self.recycled_ops = ops;
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

    fn test_key(node_id: usize, slot: u32) -> (DrawCommandId, u32) {
        (
            DrawCommandId {
                node_id,
                command_index: 0,
                placement: cranpose_render_common::style_shared::DrawPlacement::Behind,
            },
            slot,
        )
    }

    fn capture_for(key: (DrawCommandId, u32), fingerprint: u64, frame: u64) -> PendingFeedCapture {
        PendingFeedCapture {
            key,
            shape_start: 0,
            shape_count: 4,
            fingerprint,
            capture_clip: None,
            frame,
        }
    }

    #[test]
    fn retire_all_clears_pending_feed_captures() {
        let mut state = ShapeReplayState::default();
        state
            .pending_feed_captures
            .push(capture_for(test_key(1, 0), 0, 3));
        state.retire_all();
        assert!(
            state.pending_feed_captures.is_empty(),
            "retired scenes must void their capture requests: the shape \
             indices reference a scene that will never render"
        );
    }

    #[test]
    fn take_frame_ops_evicts_idle_feed_slots() {
        let mut state = ShapeReplayState::default();
        let idle = test_key(1, 0);
        let live = test_key(1, 1);
        state.frame = FEED_SLOT_IDLE_FRAMES + 5;
        state.feed_slots.insert(
            idle,
            FeedSlot {
                gpu_slot: 7,
                fingerprint: 0,
                capture_clip: None,
                last_referenced: 1,
            },
        );
        state.feed_slots.insert(
            live,
            FeedSlot {
                gpu_slot: 8,
                fingerprint: 0,
                capture_clip: None,
                last_referenced: state.frame,
            },
        );
        let generation = crate::pipeline::retained_feed_generation();
        cranpose_render_common::scene_builder::set_retained_feed_epoch(Some(generation));
        cranpose_render_common::scene_builder::confirm_retained_slot(idle.0, idle.1, generation);
        assert!(cranpose_render_common::scene_builder::retained_slot_confirmed(idle.0, idle.1));

        let ops = state.take_frame_ops(generation);

        assert_eq!(
            ops.releases,
            vec![7],
            "the idle slot's release must ride this frame's ops"
        );
        assert!(
            !state.feed_slots.contains_key(&idle),
            "an idle slot must age out planner-side"
        );
        assert!(
            state.feed_slots.contains_key(&live),
            "a recently served slot must survive"
        );
        assert!(
            !cranpose_render_common::scene_builder::retained_slot_confirmed(idle.0, idle.1),
            "eviction must revoke the span's confirmation"
        );
    }

    #[test]
    fn apply_ack_releases_displaced_slot_and_promotes_recapture() {
        let mut state = ShapeReplayState::default();
        let key = test_key(2, 0);
        state.frame = 10;
        state.feed_slots.insert(
            key,
            FeedSlot {
                gpu_slot: 3,
                fingerprint: 1,
                capture_clip: None,
                last_referenced: 9,
            },
        );
        state.pending_feed_captures.push(capture_for(key, 42, 10));
        let generation = crate::pipeline::retained_feed_generation();
        let ops = state.take_frame_ops(generation);
        assert_eq!(ops.captures.len(), 1);

        let ack = ReplayAck {
            generation,
            confirmations: vec![(key, 9)],
        };
        let returned = state.apply_ack(ack, ReplayFrameOps::default());
        assert!(
            returned.is_empty(),
            "the confirmations buffer returns drained for the store to refill"
        );
        let slot = state.feed_slots.get(&key).expect("recapture must be live");
        assert_eq!(
            (slot.gpu_slot, slot.fingerprint, slot.last_referenced),
            (9, 42, 10),
            "the confirmed capture replaces the identity's slot whole"
        );
        assert_eq!(
            state.pending_releases,
            vec![3],
            "the displaced slot's release must queue for the next frame's ops"
        );
        assert!(
            state.awaiting_confirmation.is_empty(),
            "no capture may stay awaiting past its ack"
        );
    }

    #[test]
    fn ack_roundtrip_promotes_capture_into_served_feed_slot() {
        let mut state = ShapeReplayState {
            supported: true,
            frame: 4,
            ..Default::default()
        };
        let key = test_key(3, 2);
        let clip = Rect {
            x: 1.0,
            y: 2.0,
            width: 30.0,
            height: 40.0,
        };
        state.pending_feed_captures.push(PendingFeedCapture {
            key,
            shape_start: 5,
            shape_count: 7,
            fingerprint: 77,
            capture_clip: Some(clip),
            frame: 4,
        });
        let generation = crate::pipeline::retained_feed_generation();

        let ops = state.take_frame_ops(generation);
        assert_eq!((ops.generation, ops.frame), (generation, 4));
        assert_eq!(ops.captures.len(), 1);
        assert!(state.pending_feed_captures.is_empty());
        assert!(!state.supported, "packet build closes the replay window");

        cranpose_render_common::scene_builder::set_retained_feed_epoch(Some(generation));
        let ack = ReplayAck {
            generation,
            confirmations: vec![(key, 11)],
        };
        state.apply_ack(ack, ReplayFrameOps::default());

        // Everything span serving checks at collect time is in place: the
        // gpu slot, the capture-context fingerprint, the ambient clip, and
        // the reference stamp.
        let slot = state
            .feed_slots
            .get(&key)
            .expect("a confirmed capture must be servable");
        assert_eq!(
            (slot.gpu_slot, slot.fingerprint, slot.last_referenced),
            (11, 77, 4)
        );
        let got = slot
            .capture_clip
            .expect("capture clip must survive the roundtrip");
        assert_eq!(
            (got.x, got.y, got.width, got.height),
            (clip.x, clip.y, clip.width, clip.height)
        );
        assert!(
            cranpose_render_common::scene_builder::retained_slot_confirmed(key.0, key.1),
            "the next build may bypass the confirmed span"
        );
    }

    #[test]
    fn reclaim_cancelled_ops_requeues_releases_and_purges_awaiting() {
        let mut state = ShapeReplayState::default();
        let cancelled_key = test_key(5, 0);
        let later_key = test_key(5, 1);
        state.frame = 6;
        state.pending_releases.push(21);
        state.pending_releases.push(22);
        state
            .pending_feed_captures
            .push(capture_for(cancelled_key, 0, 6));
        let generation = crate::pipeline::retained_feed_generation();
        let cancelled_ops = state.take_frame_ops(generation);
        assert_eq!(cancelled_ops.releases, vec![21, 22]);
        assert_eq!(state.awaiting_confirmation.len(), 1);
        let captures_capacity = cancelled_ops.captures.capacity();
        let releases_capacity = cancelled_ops.releases.capacity();

        // A later frame's plan with its own capture: its awaiting entry
        // must SURVIVE the earlier frame's reclaim (the purge is precise
        // to the cancelled batch's (generation, frame)).
        state.begin_frame(true, 1.0);
        state
            .pending_feed_captures
            .push(capture_for(later_key, 0, 7));
        let _later_ops = state.take_frame_ops(generation);
        assert_eq!(state.awaiting_confirmation.len(), 2);

        state.reclaim_cancelled_ops(cancelled_ops);

        assert_eq!(
            state.pending_releases,
            vec![21, 22],
            "cancelled releases must re-queue whole — the pool is 128 ids \
             and a dropped batch would leak them forever"
        );
        assert!(
            !state.awaiting_confirmation.contains_key(&cancelled_key),
            "the cancelled batch's capture can never confirm"
        );
        assert!(
            state.awaiting_confirmation.contains_key(&later_key),
            "a later frame's awaiting entry must survive the purge"
        );
        assert!(
            state.recycled_ops.captures.capacity() >= captures_capacity
                && state.recycled_ops.releases.capacity() >= releases_capacity,
            "the cancelled batch's buffers must recycle with capacity intact"
        );
        assert!(
            state.recycled_ops.captures.is_empty() && state.recycled_ops.releases.is_empty(),
            "recycled buffers must come back empty"
        );
    }

    #[test]
    fn renderer_replaced_clears_awaiting_and_release_queue() {
        let mut state = ShapeReplayState::default();
        let key = test_key(4, 0);
        state.frame = 2;
        state.feed_slots.insert(
            key,
            FeedSlot {
                gpu_slot: 5,
                fingerprint: 0,
                capture_clip: None,
                last_referenced: 2,
            },
        );
        state.pending_feed_captures.push(capture_for(key, 0, 2));
        let generation_before = crate::pipeline::retained_feed_generation();
        let _ops = state.take_frame_ops(generation_before);
        assert!(!state.awaiting_confirmation.is_empty());

        state.renderer_replaced();

        assert!(
            state.awaiting_confirmation.is_empty(),
            "in-flight captures can never confirm against the new store"
        );
        assert!(
            state.pending_releases.is_empty(),
            "old slot ids must never cross renderers"
        );
        assert!(state.feed_slots.is_empty());
        assert_eq!(
            crate::pipeline::retained_feed_generation(),
            generation_before.wrapping_add(1),
            "the replacement must move the feed to a fresh generation"
        );
    }
}
