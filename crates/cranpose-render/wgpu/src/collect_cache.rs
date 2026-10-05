//! Reuse of a direct child's collected draws across frames.
//!
//! Collecting walks every layer of the scene each frame, while from one
//! frame to the next most subtrees are unchanged and land where they were.
//! A subtree that scene building left alone keeps its
//! [`LayerNode::content_revision`], and when the walk also hands it the same
//! offset, clip, anchor and scale, its draws are the ones it produced last
//! time: they are appended again instead of collected.

use std::hash::Hasher;

use cranpose_core::{
    NodeId,
    collections::map::{Entry as MapEntry, HashMap},
};
use cranpose_render_common::graph::LayerNode;
use cranpose_ui_graphics::FxHasher;

use crate::{collect::WalkContext, scene::SceneSegment};

/// How many frames a child unreached keeps its draws.
const FORGET_AFTER_FRAMES: u32 = 120;

/// The fewest render nodes a child's subtree holds for its draws to be
/// looked up. Collecting a smaller subtree costs about what the lookup does.
const MIN_REUSED_NODES: u32 = 4;

/// What one direct child's draws follow from: its subtree's revision, and a
/// hash of where it lands: its own transform, which a scroll moves without a
/// new revision, and what the walk hands it.
#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) struct SegmentKey {
    revision: u64,
    placement: u64,
}

impl SegmentKey {
    /// The key of `layer` collected under `context`, when the layer has an
    /// identity, a revision to reuse its draws by, and enough under it to be
    /// worth a lookup.
    pub(crate) fn of(layer: &LayerNode, context: &WalkContext) -> Option<(NodeId, Self)> {
        let node = layer.node_id?;
        if layer.content_revision == 0 || layer.subtree_nodes < MIN_REUSED_NODES {
            return None;
        }
        let mut placement = FxHasher::default();
        for value in layer.transform_to_parent.matrix().as_flattened() {
            placement.write_u32(value.to_bits());
        }
        context.hash_placement(&mut placement);
        Some((
            node,
            Self {
                revision: layer.content_revision,
                placement: placement.finish(),
            },
        ))
    }
}

/// What [`CollectCache::visit`] decided for a child.
pub(crate) enum Visit<'a> {
    /// Append these draws; the bool says whether the child holds text or an
    /// image.
    Reuse(&'a SceneSegment, bool),
    /// Collect the child, and keep a copy of its draws when `keep` is set.
    Collect { keep: bool },
    /// Collect the child, which is placed elsewhere than before: nothing
    /// under it lands where it did either, so nothing under it can reuse.
    Moved,
}

struct Segment {
    draws: SceneSegment,
    pixel_sensitive: bool,
}

/// Kept small, so the lookups of a frame stay in cache.
struct CachedChild {
    key: SegmentKey,
    /// How many frames in a row the key has stood.
    stood: u32,
    frame: u32,
    /// Kept once the key has stood [`STORE_AFTER_FRAMES`] frames, so a
    /// subtree that keeps changing is not copied each time.
    segment: Option<Box<Segment>>,
}

/// How many frames in a row a child's key stands before its draws are kept.
const STORE_AFTER_FRAMES: u32 = 3;

/// The draws of the direct children collected in recent frames, by node.
#[derive(Default)]
pub(crate) struct CollectCache {
    children: HashMap<NodeId, CachedChild>,
    frame: u32,
    /// Children whose draws this frame reused.
    reused: u32,
}

impl CollectCache {
    pub(crate) fn begin_frame(&mut self) {
        self.frame = self.frame.wrapping_add(1);
        self.reused = 0;
    }

    /// How many children's draws the last collection reused.
    pub(crate) fn reused(&self) -> u32 {
        self.reused
    }

    /// Forgets the children no frame has reached for a while. A child under
    /// a reused parent is not reached, yet its draws serve again once the
    /// parent changes.
    pub(crate) fn end_frame(&mut self) {
        let frame = self.frame;
        if frame.is_multiple_of(FORGET_AFTER_FRAMES) {
            self.children
                .retain(|_, child| frame.wrapping_sub(child.frame) < FORGET_AFTER_FRAMES);
        }
    }

    /// What to do with `node`, collected under `key` this frame: reuse the
    /// draws it produced under the same key before, or collect it, keeping a
    /// copy once the key has stood [`STORE_AFTER_FRAMES`] frames.
    pub(crate) fn visit(&mut self, node: NodeId, key: &SegmentKey) -> Visit<'_> {
        let frame = self.frame;
        let child = match self.children.entry(node) {
            MapEntry::Vacant(vacant) => {
                vacant.insert(CachedChild {
                    key: *key,
                    stood: 1,
                    segment: None,
                    frame,
                });
                return Visit::Collect { keep: false };
            }
            MapEntry::Occupied(occupied) => occupied.into_mut(),
        };
        child.frame = frame;
        if child.key != *key {
            let moved = child.key.placement != key.placement;
            child.key = *key;
            child.stood = 1;
            child.segment = None;
            return if moved {
                Visit::Moved
            } else {
                Visit::Collect { keep: false }
            };
        }
        child.stood = child.stood.saturating_add(1);
        match &child.segment {
            Some(segment) => {
                self.reused += 1;
                Visit::Reuse(&segment.draws, segment.pixel_sensitive)
            }
            None => Visit::Collect {
                keep: child.stood >= STORE_AFTER_FRAMES,
            },
        }
    }

    /// Keeps the draws `node` produced this frame, for frames its key stands.
    pub(crate) fn keep(&mut self, node: NodeId, draws: SceneSegment, pixel_sensitive: bool) {
        if let Some(child) = self.children.get_mut(&node) {
            child.segment = Some(Box::new(Segment {
                draws,
                pixel_sensitive,
            }));
        }
    }
}
