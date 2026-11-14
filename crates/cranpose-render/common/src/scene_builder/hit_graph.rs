use cranpose_core::RecordId;
use cranpose_ui::Rect;
use cranpose_ui_graphics::LayerShape;

use super::graph_changes::GraphChanges;
use crate::{
    graph::ProjectiveTransform,
    scene_store::{LayerRef, SceneStore},
};

pub(super) fn rebuild_tracking_hits(
    store: &mut SceneStore,
    layer: RecordId,
    rebuild: impl FnOnce(&mut SceneStore, RecordId) -> bool,
) -> Option<bool> {
    let previous = HitGraphState::of(store.layer(layer));
    let mut descendants = crate::thread_state::with(|state| state.hit_geometry_scratch.take());
    descendants.clear();
    visit_hit_layers(store.layer(layer), &mut |child| {
        descendants.push(HitGraphState::of(child));
        true
    });
    let dirty = rebuild(store, layer).then(|| {
        let mut expected = descendants.iter();
        let layer = store.layer(layer);
        previous.dirty_against(&HitGraphState::of(layer))
            || !visit_hit_layers(layer, &mut |child| {
                expected.next() == Some(&HitGraphState::of(child))
            })
            || expected.next().is_some()
    });
    crate::thread_state::with(|state| state.hit_geometry_scratch.set(descendants));
    dirty
}

fn visit_hit_layers<'a>(layer: LayerRef<'a>, visit: &mut impl FnMut(LayerRef<'a>) -> bool) -> bool {
    for child in layer.children() {
        if child.hit_test().is_none() && !child.has_hit_targets() {
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
    space: bool,
}

impl HitGraphState {
    pub(super) fn of(layer: LayerRef<'_>) -> Self {
        Self {
            hit_test: layer.hit_test().is_some(),
            has_hit_targets: layer.has_hit_targets(),
            local_bounds: layer.local_bounds(),
            node_bounds: layer.node_bounds(),
            transform_to_parent: layer.transform_to_parent(),
            clip_rect: layer.clip_rect(),
            shape: layer.graphics_layer().shape,
            space: layer.graphics_layer().space.is_some(),
        }
    }

    #[inline]
    pub(super) fn changes_against(&self, replacement: &Self) -> GraphChanges {
        GraphChanges::hits(self.dirty_against(replacement))
            | GraphChanges::moves(self.transform_to_parent != replacement.transform_to_parent)
    }

    pub(super) fn dirty_against(&self, replacement: &Self) -> bool {
        self.hit_test
            || replacement.hit_test
            || self.space
            || replacement.space
            || ((self.has_hit_targets || replacement.has_hit_targets) && self != replacement)
    }
}
