use cranpose_ui::Rect;
use cranpose_ui_graphics::LayerShape;

use crate::graph::{LayerNode, ProjectiveTransform, RenderNode};

/// Rebuilds `layer` with `rebuild`, which reports whether it succeeded, and
/// returns whether the hit geometry of the layer or of a layer under it
/// changed; `None` when the rebuild failed. A rebuild takes in the moves and
/// layer changes of the nodes under the layer, which its own state does not
/// show: a sheet that slides inside a container that redraws keeps the
/// container's state.
pub(super) fn rebuild_tracking_hits(
    layer: &mut LayerNode,
    rebuild: impl FnOnce(&mut LayerNode) -> bool,
) -> Option<bool> {
    let previous = HitGraphState::of(layer);
    let mut descendants = crate::thread_state::with(|state| state.hit_geometry_scratch.take());
    descendants.clear();
    visit_hit_layers(layer, &mut |child| {
        descendants.push(HitGraphState::of(child));
        true
    });
    let dirty = rebuild(layer).then(|| {
        let mut expected = descendants.iter();
        previous.dirty_against(&HitGraphState::of(layer))
            || !visit_hit_layers(layer, &mut |child| {
                expected.next() == Some(&HitGraphState::of(child))
            })
            || expected.next().is_some()
    });
    crate::thread_state::with(|state| state.hit_geometry_scratch.set(descendants));
    dirty
}

/// Visits the layers under `layer` that are hit targets or hold one, in draw
/// order, while `visit` returns true; returns whether every visit did.
fn visit_hit_layers(layer: &LayerNode, visit: &mut impl FnMut(&LayerNode) -> bool) -> bool {
    for child in &layer.children {
        let RenderNode::Layer(child) = child else {
            continue;
        };
        if child.hit_test.is_none() && !child.has_hit_targets {
            continue;
        }
        if !visit(child) || !visit_hit_layers(child, visit) {
            return false;
        }
    }
    true
}

#[derive(PartialEq)]
pub(crate) struct HitGraphState {
    hit_test: bool,
    has_hit_targets: bool,
    local_bounds: Rect,
    node_bounds: Option<Rect>,
    transform_to_parent: ProjectiveTransform,
    clip_rect: Option<Rect>,
    shape: LayerShape,
    /// Whether the layer is a scene, a mesh surface or a 3D transform. Each
    /// update of such a layer runs its closure again, which may move where
    /// the surfaces of its scene take input or hide one another.
    space: bool,
}

impl HitGraphState {
    pub(super) fn of(layer: &LayerNode) -> Self {
        Self {
            hit_test: layer.hit_test.is_some(),
            has_hit_targets: layer.has_hit_targets,
            local_bounds: layer.local_bounds,
            node_bounds: layer.node_bounds,
            transform_to_parent: layer.transform_to_parent,
            clip_rect: layer.clip_rect(),
            shape: layer.graphics_layer.shape,
            space: layer.graphics_layer.space.is_some(),
        }
    }

    pub(super) fn dirty_against(&self, replacement: &Self) -> bool {
        self.hit_test
            || replacement.hit_test
            || self.space
            || replacement.space
            || ((self.has_hit_targets || replacement.has_hit_targets) && self != replacement)
    }
}
