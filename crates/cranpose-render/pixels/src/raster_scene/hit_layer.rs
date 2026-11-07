use cranpose_render_common::graph::{HitTestNode, LayerNode};
use cranpose_ui_graphics::Rect;

// What the hit targets beneath a layer take from it: its bounds, whether
// it cuts them, and its node's own pointer handling.
#[derive(Clone)]
pub(crate) struct HitLayer {
    pub local_bounds: Rect,
    pub node_bounds: Option<Rect>,
    pub clips: bool,
    pub hit_test: Option<HitTestNode>,
}

impl HitLayer {
    // What the hit targets beneath `layer` take from it.
    pub(crate) fn of(layer: &LayerNode) -> Self {
        Self {
            local_bounds: layer.local_bounds,
            node_bounds: layer.node_bounds,
            clips: layer.clip_rect().is_some(),
            hit_test: layer.hit_test.clone(),
        }
    }

    // Takes what the hit targets beneath `layer` take from it, in place.
    pub(crate) fn take_from(&mut self, layer: &LayerNode) {
        self.local_bounds = layer.local_bounds;
        self.node_bounds = layer.node_bounds;
        self.clips = layer.clip_rect().is_some();
        self.hit_test.clone_from(&layer.hit_test);
    }
}
