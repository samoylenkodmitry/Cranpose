// The contents of isolated layers that returned packets gave back, kept for
// the next collects. A layer whose content hash is as it was, and that its
// parent passes down to as it did, takes its content from here with no walk
// of its subtree: a still subtree in a window that animates around it, or
// one that only moves, turns or fades, costs one lookup a frame.

use std::hash::Hasher;

use cranpose_core::{NodeId, collections::map::HashMap};
use cranpose_render_common::layer_shadow::ShadowLight;
use cranpose_ui_graphics::FxHasher;

use super::{
    ChildLayer, LayerScene, LayerSceneRecycler, RasterScale, WalkContext, WalkSettings,
    layer_motion::Motion,
};
use crate::frame_packet::MAX_FRAMES_IN_FLIGHT;

// What an isolated layer's content was collected from: the hash of its
// content, which its raster cache keys on too, and what the walk passed down
// to it, save the light.
#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) struct ContentKey {
    content_hash: u64,
    raster_scale: (bool, u32),
    // Whether the walk wants pixel-sensitive content and whether its layers
    // translate their content.
    flags: (bool, bool),
}

// The shadow light a walk passes down, by its bits. It moves with the
// layer, so a content keeps it only when its layers cast shadows.
#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) struct LightBits([u32; 5]);

impl LightBits {
    // Every field of the light is named here, so that a new one has to be
    // keyed on purpose.
    pub(super) fn of(light: ShadowLight) -> Self {
        let ShadowLight {
            x,
            y,
            z,
            radius,
            pixels_per_unit,
        } = light;
        Self([x, y, z, radius, pixels_per_unit].map(f32::to_bits))
    }

    // `content_hash` of a content that casts shadows, with this light, which
    // places them: a raster of the content holds only under the same light.
    pub(super) fn lit_hash(self, content_hash: u64) -> u64 {
        let mut hasher = FxHasher::default();
        hasher.write_u64(content_hash);
        for bits in self.0 {
            hasher.write_u32(bits);
        }
        hasher.finish()
    }
}

impl ContentKey {
    // The key of a content with `content_hash` that `context` walks, and the
    // light it passes down. Every field of the context and its settings is
    // named here, so that a new one has to be keyed or left out on purpose.
    pub(super) fn new(content_hash: u64, context: &WalkContext<'_>) -> (Self, LightBits) {
        let WalkContext {
            // An isolated layer's content starts at its origin, unclipped
            // and with no anchor.
            offset: _,
            visual_clip: _,
            rounding: _,
            snap_anchor: _,
            translated,
            settings,
        } = *context;
        let WalkSettings {
            raster_scale,
            light,
            wants_pixel_sensitive,
            // The update tells content that changes from content that does
            // not; the content hash already holds that for a kept subtree.
            update: _,
            // A layer in a scene's space keeps no content.
            in_space: _,
        } = *settings;
        let key = Self {
            content_hash,
            raster_scale: match raster_scale {
                RasterScale::Exact(scale) => (false, scale.to_bits()),
                RasterScale::Minimum(scale) => (true, scale.to_bits()),
            },
            flags: (wants_pixel_sensitive, translated),
        };
        (key, LightBits::of(light))
    }
}

// What a collected content carries besides its scene for a later collect
// to take it: its key, the light when its layers cast shadows, whether it
// holds pixel-sensitive content, and the raster scales its layers recorded.
pub(crate) struct KeptWith {
    pub(super) key: ContentKey,
    pub(super) light: Option<LightBits>,
    pub(super) pixel_sensitive: bool,
    pub(super) scales: Vec<(NodeId, Motion)>,
}

impl KeptWith {
    // Whether a walk under `(key, light)` collects the content kept with
    // this.
    fn fits(&self, (key, light): (ContentKey, LightBits)) -> bool {
        self.key == key && self.light.is_none_or(|kept| kept == light)
    }
}

struct Kept {
    with: Box<KeptWith>,
    content: Box<LayerScene>,
}

#[derive(Default)]
pub(crate) struct KeptContents {
    // By node, the contents returned packets gave back, one for each packet
    // in flight at most.
    kept: HashMap<NodeId, Vec<Kept>>,
    // The key and light of each node's content in the collect that ran
    // last, and in the one that runs now.
    walks: HashMap<NodeId, (ContentKey, LightBits)>,
    next_walks: HashMap<NodeId, (ContentKey, LightBits)>,
}

impl KeptContents {
    // The content of `node` kept for `walk`, which the collect that runs
    // takes, and notes the walk for the packets that come back.
    pub(super) fn take(
        &mut self,
        node: NodeId,
        walk: (ContentKey, LightBits),
    ) -> Option<(Box<LayerScene>, Box<KeptWith>)> {
        self.next_walks.insert(node, walk);
        let copies = self.kept.get_mut(&node)?;
        let at = copies.iter().position(|kept| kept.with.fits(walk))?;
        let Kept { with, content } = copies.swap_remove(at);
        Some((content, with))
    }

    // After a collect: gives `recycler` each content no layer of it may
    // take.
    pub(crate) fn end_collect(&mut self, recycler: &mut LayerSceneRecycler) {
        std::mem::swap(&mut self.walks, &mut self.next_walks);
        self.next_walks.clear();
        let walks = &self.walks;
        self.kept.retain(|node, copies| {
            let walk = walks.get(node).copied();
            let mut at = 0;
            while at < copies.len() {
                if walk.is_some_and(|walk| copies[at].with.fits(walk)) {
                    at += 1;
                } else {
                    recycler.recycle(copies.swap_remove(at).content);
                }
            }
            !copies.is_empty()
        });
    }

    // Keeps, of the isolated children in `scene`, a scene a packet gave back,
    // the contents the last collect's layers may take again, and gives the
    // rest to `recycler`.
    pub(crate) fn keep_from(
        &mut self,
        mut scene: Box<LayerScene>,
        recycler: &mut LayerSceneRecycler,
    ) {
        while let Some(child) = scene.children.pop() {
            let ChildLayer {
                node_id,
                kept,
                content,
                ..
            } = child;
            match (node_id, kept) {
                (Some(node), Some(with))
                    if self.walks.get(&node).is_some_and(|&walk| with.fits(walk)) =>
                {
                    let copies = self.kept.entry(node).or_default();
                    if copies.len() < MAX_FRAMES_IN_FLIGHT {
                        copies.push(Kept { with, content });
                    } else {
                        recycler.recycle(content);
                    }
                }
                _ => self.keep_from(content, recycler),
            }
        }
        recycler.recycle(scene);
    }
}
