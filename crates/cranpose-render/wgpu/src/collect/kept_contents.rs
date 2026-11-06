// The contents of isolated layers that returned packets gave back, kept for
// the next collects. A layer whose subtree took no change number since, and
// that its parent passes down to as it did, takes its content from here with
// no walk of its subtree: a still subtree in a window that animates around
// it costs one lookup a frame.

use cranpose_core::{NodeId, collections::map::HashMap};
use cranpose_render_common::{graph::LayerChange, layer_shadow::ShadowLight};

use super::{
    ChildLayer, LayerScene, LayerSceneRecycler, RasterScale, WalkContext, WalkSettings,
    layer_motion::Motion,
};
use crate::frame_packet::MAX_FRAMES_IN_FLIGHT;

// What an isolated layer's content was collected from: the number its
// subtree took when it last changed, and what the walk passed down to it.
#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) struct ContentKey {
    change: LayerChange,
    raster_scale: (bool, u32),
    light: [u32; 5],
    // Whether the walk wants pixel-sensitive content and whether its layers
    // translate their content.
    flags: (bool, bool),
}

impl ContentKey {
    // Every field of the context, its settings and the light is named here,
    // so that a new one has to be keyed or left out on purpose.
    pub(super) fn new(change: LayerChange, context: &WalkContext<'_>) -> Self {
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
            // not; the change number already holds that for a kept subtree.
            update: _,
            // A layer in a scene's space keeps no content.
            in_space: _,
        } = *settings;
        let ShadowLight {
            x,
            y,
            z,
            radius,
            pixels_per_unit,
        } = light;
        Self {
            change,
            raster_scale: match raster_scale {
                RasterScale::Exact(scale) => (false, scale.to_bits()),
                RasterScale::Minimum(scale) => (true, scale.to_bits()),
            },
            light: [x, y, z, radius, pixels_per_unit].map(f32::to_bits),
            flags: (wants_pixel_sensitive, translated),
        }
    }
}

// What a collected content carries besides its scene for a later collect
// to take it: its key, whether it holds pixel-sensitive content, and the
// raster scales its layers recorded.
pub(crate) struct KeptWith {
    pub(super) key: ContentKey,
    pub(super) pixel_sensitive: bool,
    pub(super) scales: Vec<(NodeId, Motion)>,
}

struct Kept {
    with: KeptWith,
    content: Box<LayerScene>,
}

#[derive(Default)]
pub(crate) struct KeptContents {
    // By node, the contents returned packets gave back, one for each packet
    // in flight at most.
    kept: HashMap<NodeId, Vec<Kept>>,
    // The key of each node's content in the collect that ran last, and in
    // the one that runs now.
    keys: HashMap<NodeId, ContentKey>,
    next_keys: HashMap<NodeId, ContentKey>,
}

impl KeptContents {
    // The content of `node` kept with `key`, which the collect that runs
    // takes, and notes the key for the packets that come back.
    pub(super) fn take(
        &mut self,
        node: NodeId,
        key: ContentKey,
    ) -> Option<(Box<LayerScene>, KeptWith)> {
        self.next_keys.insert(node, key);
        let copies = self.kept.get_mut(&node)?;
        let at = copies.iter().position(|kept| kept.with.key == key)?;
        let Kept { with, content } = copies.swap_remove(at);
        Some((content, with))
    }

    // After a collect: gives `recycler` each content no layer of it may
    // take.
    pub(crate) fn end_collect(&mut self, recycler: &mut LayerSceneRecycler) {
        std::mem::swap(&mut self.keys, &mut self.next_keys);
        self.next_keys.clear();
        let keys = &self.keys;
        self.kept.retain(|node, copies| {
            let key = keys.get(node);
            let mut at = 0;
            while at < copies.len() {
                if key == Some(&copies[at].with.key) {
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
                (Some(node), Some(with)) if self.keys.get(&node) == Some(&with.key) => {
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
