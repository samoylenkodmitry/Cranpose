use std::{
    cell::RefCell,
    hash::{Hash, Hasher},
};

use cranpose_render_common::graph::DrawCommandId;
use cranpose_ui_graphics::{FxHasher, GraphicsLayer, Point, Rect};

use crate::{
    frame_packet::{ReplayAck, ReplayConfirmation, ReplayFrameOps},
    scene::{ColorPatch, PendingFeedCapture, SimilarityTransform},
};

pub(crate) const MAX_RETAINED_OPS: usize = 128;

pub(crate) struct FeedSlot {
    pub gpu_slot: u32,
    pub fingerprint: u64,
    pub capture_clip: Option<Rect>,
    pub last_referenced: u64,
}

pub(crate) struct RequestedFeedSlot {
    pub fingerprint: u64,
    pub capture_clip: Option<Rect>,
    pub generation: u64,
    pub frame: u64,
}

pub(crate) const FEED_SLOT_IDLE_FRAMES: u64 = 120;

#[derive(Default)]
pub(crate) struct ShapeReplayState {
    pub supported: bool,
    pub frame: u64,
    pub root_scale: f32,
    pub stat_patches: u64,
    pub stat_remat_miss: u64,
    pub pending_color_patches: Vec<ColorPatch>,
    pub pending_releases: Vec<u32>,
    pub feed_slots: std::collections::HashMap<
        (DrawCommandId, u32),
        FeedSlot,
        cranpose_ui_graphics::FxBuildHasher,
    >,
    pub pending_feed_captures: Vec<PendingFeedCapture>,
    pub awaiting_confirmation: std::collections::HashMap<
        (DrawCommandId, u32),
        RequestedFeedSlot,
        cranpose_ui_graphics::FxBuildHasher,
    >,
    pub recycled_ops: ReplayFrameOps,
}

thread_local! {
    pub(crate) static SHAPE_REPLAY: RefCell<ShapeReplayState> =
        RefCell::new(ShapeReplayState::default());
}

pub(crate) fn command_feed_enabled() -> bool {
    crate::debug_toggles::debug_toggle("CRANPOSE_COMMAND_FEED").as_deref() != Some("0")
}

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

#[doc(hidden)]
pub fn pending_feed_capture_count_for_tests() -> usize {
    SHAPE_REPLAY.with(|state| state.borrow().pending_feed_captures.len())
}

#[doc(hidden)]
pub fn clear_pending_feed_captures_for_tests() {
    SHAPE_REPLAY.with(|state| state.borrow_mut().pending_feed_captures.clear());
}

pub(crate) fn any_pending_feed_captures() -> bool {
    SHAPE_REPLAY.with(|state| !state.borrow().pending_feed_captures.is_empty())
}

pub(crate) fn shape_index_pending_feed_capture(shape_index: usize) -> bool {
    SHAPE_REPLAY.with(|state| {
        state.borrow().pending_feed_captures.iter().any(|capture| {
            shape_index >= capture.shape_start
                && shape_index < capture.shape_start.saturating_add(capture.shape_count)
        })
    })
}

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
    pub(crate) fn retire_all(&mut self) {
        self.pending_feed_captures.clear();
        self.pending_color_patches.clear();
    }

    pub(crate) fn begin_frame(&mut self, supported: bool, root_scale: f32) {
        self.frame = self.frame.wrapping_add(1);
        let feed_scale_changed = !self.feed_slots.is_empty() && self.root_scale != root_scale;
        self.root_scale = root_scale;
        self.supported = supported;
        if (!self.supported && !self.feed_slots.is_empty()) || feed_scale_changed {
            self.retire_feed();
        }
    }

    pub(crate) fn retire_feed(&mut self) {
        for (_, slot) in self.feed_slots.drain() {
            self.pending_releases.push(slot.gpu_slot);
        }
        self.pending_feed_captures.clear();
        cranpose_render_common::scene_builder::clear_retained_slot_confirmations();
        crate::pipeline::bump_retained_feed_generation();
    }

    pub(crate) fn renderer_replaced(&mut self) {
        self.retire_feed();
        self.retire_all();
        self.pending_releases.clear();
        self.awaiting_confirmation.clear();
    }

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

    pub(crate) fn apply_ack(
        &mut self,
        mut ack: ReplayAck,
        recycled: ReplayFrameOps,
    ) -> Vec<ReplayConfirmation> {
        let frame = self.frame;
        for (key, gpu_slot) in ack.confirmations.drain(..) {
            let Some(requested) = self.awaiting_confirmation.remove(&key) else {
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
            ) && old.gpu_slot != gpu_slot
            {
                self.pending_releases.push(old.gpu_slot);
            }
            cranpose_render_common::scene_builder::confirm_retained_slot(
                key.0,
                key.1,
                ack.generation,
            );
        }
        self.awaiting_confirmation
            .retain(|_, requested| requested.frame != ack.frame);
        self.recycled_ops = recycled;
        ack.confirmations
    }

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
            frame: ops.frame,
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
            frame: ops.frame,
            confirmations: vec![(key, 11)],
        };
        state.apply_ack(ack, ReplayFrameOps::default());

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
