// Facts about a record's subtree that its walks branch on, worked out when a
// walk first reads them after a change and kept until the next change.
//
// A record keeps each fact only while the facts it follows from stand: its
// child facts follow from all its children's, and a containment that holds
// follows from every child's containment. A change forgets the fact it
// touches in its record and climbs while the records it reaches kept that
// fact, so a record that kept none ends the climb. Draws, sizes and places
// touch containment; the children a record holds, a pointer handler, a
// space and an origin sink touch the child facts.

use std::{cell::Cell, mem::size_of};

use cranpose_core::RecordId;
use cranpose_ui::{GraphicsLayer, Rect};

use super::{Item, ItemKind, ItemRef, LayerProps, LayerRef, RecordFlags, SceneStore};
use crate::graph::{CONTAINED_DRAW_SLACK, inflate_rect, rect_within};

#[derive(Clone, Copy, Default)]
struct RowFacts {
    children: Option<ChildFacts>,
    contained: Option<bool>,
}

// Whether the record or a record beneath it handles pointers, whether one
// publishes live window origins, and whether one is a scene, a mesh surface
// or a 3D transform.
#[derive(Clone, Copy)]
struct ChildFacts {
    hit_targets: bool,
    origin_sinks: bool,
    space: bool,
}

#[derive(Clone, Default)]
pub(crate) struct Facts {
    rows: Vec<Cell<RowFacts>>,
}

impl Facts {
    // Makes the row at `index` hold no facts: a new record's, or a freed row
    // a new record takes.
    pub(super) fn open_row(&mut self, index: usize) {
        match self.rows.get(index) {
            Some(row) => row.set(RowFacts::default()),
            None => self.rows.resize_with(index + 1, Cell::default),
        }
    }

    pub(super) fn clear(&mut self) {
        self.rows.clear();
    }

    pub(super) fn heap_bytes(&self) -> usize {
        self.rows.capacity() * size_of::<Cell<RowFacts>>()
    }
}

// Forgets the containment of the record at `index` and of each ancestor
// whose containment followed from it.
pub(super) fn forget_contained(store: &SceneStore, index: usize) {
    forget(store, index, |row| row.contained.take().is_some());
}

// Forgets the child facts of the record at `index` and of each ancestor
// whose child facts followed from them.
pub(super) fn forget_children(store: &SceneStore, index: usize) {
    forget(store, index, |row| row.children.take().is_some());
}

// Climbs from the record at `index` while `take` finds a fact the record
// kept, which it forgets.
fn forget(store: &SceneStore, index: usize, take: impl Fn(&mut RowFacts) -> bool) {
    let mut current = Some(index);
    while let Some(index) = current {
        let row = &store.facts.rows[index];
        let mut facts = row.get();
        if !take(&mut facts) {
            break;
        }
        row.set(facts);
        current = store.tree[index].parent.map(RecordId::index);
    }
}

pub(super) fn has_hit_targets(layer: LayerRef<'_>) -> bool {
    child_facts(layer).hit_targets
}

pub(super) fn has_origin_sinks(layer: LayerRef<'_>) -> bool {
    child_facts(layer).origin_sinks
}

pub(super) fn has_space(layer: LayerRef<'_>) -> bool {
    child_facts(layer).space
}

// Whether everything `layer` and its subtree draw lies within its local
// bounds, give or take the slack of antialiasing.
pub(super) fn contained(layer: LayerRef<'_>) -> bool {
    let row = &layer.store.facts.rows[layer.id.index()];
    if let Some(contained) = row.get().contained {
        return contained;
    }
    let contained = layer.visual_clip_rect().is_some()
        || items_draw_within(
            layer,
            inflate_rect(layer.local_bounds(), CONTAINED_DRAW_SLACK),
        );
    row.set(RowFacts {
        contained: Some(contained),
        ..row.get()
    });
    contained
}

fn child_facts(layer: LayerRef<'_>) -> ChildFacts {
    let row = &layer.store.facts.rows[layer.id.index()];
    if let Some(facts) = row.get().children {
        return facts;
    }
    let mut facts = ChildFacts {
        hit_targets: layer.hit_test().is_some(),
        origin_sinks: layer.flags().contains(RecordFlags::ORIGIN_SINKS),
        space: layer.graphics_layer().space.is_some(),
    };
    for child in layer.children() {
        let child = child_facts(child);
        facts.hit_targets |= child.hit_targets;
        facts.origin_sinks |= child.origin_sinks;
        facts.space |= child.space;
    }
    row.set(RowFacts {
        children: Some(facts),
        ..row.get()
    });
    facts
}

// Whether the items of `layer` put nothing outside `bounds` of it.
fn items_draw_within(layer: LayerRef<'_>, bounds: Rect) -> bool {
    layer.items().all(|item| match item {
        ItemRef::Draw(_) => false,
        ItemRef::Text(text) => text.draws_within(bounds),
        ItemRef::Run(run) => run.draws_within(bounds),
        ItemRef::Layer(child) => {
            let graphics_layer = child.graphics_layer();
            graphics_layer.shadow_elevation <= 0.0
                && graphics_layer.render_effect.is_none()
                && graphics_layer.backdrop_effect.is_none()
                && rect_within(
                    child
                        .transform_to_parent()
                        .bounds_for_rect(inflate_rect(child.local_bounds(), CONTAINED_DRAW_SLACK)),
                    bounds,
                )
                && contained(child)
        }
        ItemRef::Leaf(leaf) => {
            let origin = leaf.origin();
            rect_within(
                inflate_rect(leaf.node_rect(), CONTAINED_DRAW_SLACK).translate(origin.x, origin.y),
                bounds,
            ) && contained(leaf)
        }
    })
}

// What of a record's properties its containment and reach and its parent's
// follow from: whether it draws offscreen and where its layer bounds lie.
pub(super) fn props_inputs(props: &LayerProps) -> (bool, Option<[u32; 4]>) {
    (
        props.isolation.group_opacity || props.isolation.explicit_offscreen,
        props
            .layer_bounds
            .map(|bounds| [bounds.x, bounds.y, bounds.width, bounds.height].map(f32::to_bits)),
    )
}

// What of a graphics layer its record's containment and reach and its
// parent's follow from: a clip, a shadow, and how far an effect or a backdrop
// reaches past the layer. A layer that only moves leaves them alone.
pub(super) fn layer_inputs(layer: &GraphicsLayer) -> (bool, bool, Option<u32>, Option<u32>) {
    (
        layer.clip,
        layer.shadow_elevation > 0.0,
        layer
            .render_effect
            .as_ref()
            .map(|effect| (effect.input_padding() + effect.output_padding()).to_bits()),
        layer
            .backdrop_effect
            .as_ref()
            .map(|backdrop| backdrop.output_padding().to_bits()),
    )
}

// Whether `old` and `new` hold the same children in the same order.
pub(super) fn same_children(old: &[Item], new: &[Item]) -> bool {
    fn children(items: &[Item]) -> impl Iterator<Item = Item> + '_ {
        items
            .iter()
            .copied()
            .filter(|item| item.kind() == ItemKind::Child)
    }
    children(old).eq(children(new))
}
