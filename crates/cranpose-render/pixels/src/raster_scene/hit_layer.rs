use cranpose_render_common::graph::{HitTestNode, LayerNode};
use cranpose_ui_graphics::Rect;

use super::SubtreeItems;

// What the readers of a scene take from a layer beyond its record: the
// bounds the hit targets beneath it take, whether it cuts them, its node's
// own pointer handling, and the records of a subtree it drew into a
// surface of its own.
pub(crate) struct HitLayer {
    pub local_bounds: Rect,
    pub node_bounds: Option<Rect>,
    pub clips: bool,
    pub hit_test: Option<HitTestNode>,
    // The records of the layer's subtree, its own first, for a layer that
    // drew its children into a surface of its own: the scene holds none of
    // its children's records.
    pub surface: Box<[SubtreeItems]>,
}

impl HitLayer {
    // What the hit targets beneath `layer` take from it.
    pub(crate) fn of(layer: &LayerNode) -> Self {
        Self {
            local_bounds: layer.local_bounds,
            node_bounds: layer.node_bounds,
            clips: layer.clip_rect().is_some(),
            hit_test: layer.hit_test.clone(),
            surface: Box::default(),
        }
    }

    // Takes what the hit targets beneath `layer` take from it, in place.
    pub(crate) fn take_from(&mut self, layer: &LayerNode) {
        self.local_bounds = layer.local_bounds;
        self.node_bounds = layer.node_bounds;
        self.clips = layer.clip_rect().is_some();
        self.hit_test.clone_from(&layer.hit_test);
        self.surface = Box::default();
    }
}

impl Clone for HitLayer {
    fn clone(&self) -> Self {
        Self {
            local_bounds: self.local_bounds,
            node_bounds: self.node_bounds,
            clips: self.clips,
            hit_test: self.hit_test.clone(),
            surface: self.surface.iter().map(SubtreeItems::copied).collect(),
        }
    }
}
