// Facts about a record's subtree that its walks branch on, worked out at the
// end of each scene update for the records it changed.

use cranpose_ui::Rect;

use super::{ItemRef, LayerRef, RecordFlags, SceneStore};
use crate::graph::{CONTAINED_DRAW_SLACK, inflate_rect, rect_within};

// Works out the facts of the records whose inputs the update changed, and
// of each ancestor whose facts follow from a changed one.
pub(super) fn refresh(store: &mut SceneStore) {
    while let Some(id) = store.facts_stale.pop() {
        let index = id.index();
        let flags = store.tree[index].flags;
        if !flags.contains(RecordFlags::FACTS_STALE) {
            continue;
        }
        let facts = work_out(LayerRef::new(store, id));
        store.tree[index].flags = flags
            .without(RecordFlags::FACTS | RecordFlags::FACTS_STALE)
            .with(facts);
        if facts != flags.only(RecordFlags::FACTS)
            && let Some(parent) = store.tree[index].parent
        {
            store.stale(parent.index());
        }
    }
}

// The facts of `layer`, from its own properties and items and its
// children's facts.
fn work_out(layer: LayerRef<'_>) -> RecordFlags {
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
    [
        (hit_targets, RecordFlags::HAS_HIT_TARGETS),
        (space, RecordFlags::HAS_SPACE),
        (contained, RecordFlags::CONTAINED),
    ]
    .into_iter()
    .filter(|(on, _)| *on)
    .fold(RecordFlags::default(), |facts, (_, flag)| facts.with(flag))
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
