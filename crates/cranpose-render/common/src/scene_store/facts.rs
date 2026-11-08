// Facts about a record's subtree that its walks branch on, worked out at the
// end of each scene update for the records it changed.

use cranpose_core::RecordId;
use cranpose_ui::Rect;

use super::{ItemRef, LayerRef, RecordFlags, SceneStore};
use crate::graph::{CONTAINED_DRAW_SLACK, inflate_rect, rect_within};

// Works out the facts of `id` and of every record beneath it that the
// update in progress changed.
pub(super) fn refresh(store: &mut SceneStore, id: RecordId) {
    let update = store.update();
    let span = store.tree[id.index()].items;
    for position in 0..span.len() {
        let item = store.items.span(span)[position];
        if let Some(child) = item.as_child()
            && store.tree[child.index()].subtree_rev == update
        {
            refresh(store, child);
        }
    }
    let layer = LayerRef::new(store, id);
    let mut hit_targets = layer.hit_test().is_some();
    let mut space = layer.graphics_layer().space.is_some();
    for child in layer.children() {
        hit_targets |= child.has_hit_targets();
        space |= child.has_space();
    }
    let contained = layer.visual_clip_rect().is_some()
        || items_draw_within(
            layer,
            inflate_rect(layer.local_bounds(), CONTAINED_DRAW_SLACK),
        );
    let flags = &mut store.tree[id.index()].flags;
    *flags = flags
        .without(RecordFlags::HAS_HIT_TARGETS | RecordFlags::HAS_SPACE | RecordFlags::CONTAINED);
    if hit_targets {
        *flags = flags.with(RecordFlags::HAS_HIT_TARGETS);
    }
    if space {
        *flags = flags.with(RecordFlags::HAS_SPACE);
    }
    if contained {
        *flags = flags.with(RecordFlags::CONTAINED);
    }
}

// Whether the items of `layer` put nothing outside `bounds` of it.
fn items_draw_within(layer: LayerRef<'_>, bounds: Rect) -> bool {
    layer.items().all(|item| match item {
        ItemRef::Draw(_) => false,
        ItemRef::Text(text) => text.draws_within(bounds),
        ItemRef::Run(run) => run.draws_within(bounds),
        ItemRef::Layer(child) => {
            let graphics_layer = child.graphics_layer();
            child.draws_within_bounds()
                && graphics_layer.shadow_elevation <= 0.0
                && graphics_layer.render_effect.is_none()
                && graphics_layer.backdrop_effect.is_none()
                && rect_within(
                    child
                        .transform_to_parent()
                        .bounds_for_rect(inflate_rect(child.local_bounds(), CONTAINED_DRAW_SLACK)),
                    bounds,
                )
        }
        ItemRef::Leaf(leaf) => {
            let origin = leaf.origin();
            leaf.draws_within_bounds()
                && rect_within(
                    inflate_rect(leaf.node_rect(), CONTAINED_DRAW_SLACK)
                        .translate(origin.x, origin.y),
                    bounds,
                )
        }
    })
}
