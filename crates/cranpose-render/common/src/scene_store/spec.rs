// Records written from a layer tree built by hand: a renderer's tests and
// overlays describe what they draw as a tree, and the store holds it as
// records. A description written over the one before keeps each record of
// the same kind and node in place, so the journal logs only what changed.

use cranpose_core::RecordId;
use cranpose_ui::{Point, Rect, Size};

use super::{Item, ItemKind, RecordFlags, RecordKind, RecordSeed, SceneStore};
use crate::graph::{
    InlineNode, LayerNode, PrimitiveNode, PrimitivePhase, ProjectiveTransform, RenderNode,
    for_each_in_draw_order,
};

pub(super) fn write_root(store: &mut SceneStore, root: &LayerNode) {
    let old = store.root();
    let record = write_layer(store, None, old, root);
    if let Some(old) = old.filter(|&old| old != record) {
        store.remove_subtree(old);
    }
}

// Writes `layer` over `old` when it draws for the same kind and node, else
// as a new record, which the caller puts in place of `old`.
fn write_layer(
    store: &mut SceneStore,
    parent: Option<RecordId>,
    old: Option<RecordId>,
    layer: &LayerNode,
) -> RecordId {
    let (kind, node) = match (layer.draw_phase, layer.wraps) {
        (Some(_), _) => (RecordKind::DrawGroup, layer.node_id),
        (None, Some(wrapped)) => (RecordKind::Wrapper, Some(wrapped)),
        (None, None) => (RecordKind::Layer, layer.node_id),
    };
    let kept = old.filter(|&old| store.kind(old) == kind && store.node(old) == node);
    let record = kept.unwrap_or_else(|| store.insert(RecordSeed { kind, node, parent }));
    let node_rect = Rect::from_size(layer.size);
    let layer_bounds = layer.layer_bounds.filter(|bounds| *bounds != node_rect);
    let flags = layer_flags(layer);
    let restyled = kept.is_some() && restyled(store, record, layer, layer_bounds, flags);
    store.set_content_offset(record, layer.content_offset);
    store.write_size(record, layer.size.width, layer.size.height);
    let origin = match kind {
        RecordKind::DrawGroup => layer
            .transform_to_parent
            .as_translation()
            .unwrap_or_default(),
        RecordKind::Layer | RecordKind::Wrapper | RecordKind::Leaf => layer.origin_in_parent,
    };
    store.set_placement(record, origin, Some(layer.transform_to_parent));
    if store.graphics_layer(record) != layer.graphics_layer.get() {
        store.set_graphics_layer(record, layer.graphics_layer.get().cloned());
    }
    store.edit_props(record, |props| {
        props.layer_bounds = layer_bounds;
        props.translated_content_offset = layer.translated_content_offset;
        props.hit_test.clone_from(&layer.hit_test);
        props.isolation = layer.isolation;
        props.cache_policy = layer.cache_policy;
    });
    store.write_flags(record, flags);
    if restyled {
        store.note_property_change(record);
    }
    write_items(store, record, &layer.children);
    record
}

fn layer_flags(layer: &LayerNode) -> RecordFlags {
    let mut flags = RecordFlags::default();
    for (on, flag) in [
        (layer.clip_to_bounds, RecordFlags::CLIP_TO_BOUNDS),
        (layer.motion_context_animated, RecordFlags::MOTION_ANIMATED),
        (
            layer.translated_content_context,
            RecordFlags::TRANSLATED_CONTENT,
        ),
        (layer.has_origin_sinks, RecordFlags::ORIGIN_SINKS),
        (
            layer.draw_phase == Some(PrimitivePhase::AfterChildren),
            RecordFlags::AFTER_CHILDREN,
        ),
    ] {
        if on {
            flags = flags.with(flag);
        }
    }
    flags
}

// Whether `layer` draws `record`'s subtree in another way than the record
// does: in another size, clip, layer or pass.
fn restyled(
    store: &SceneStore,
    record: RecordId,
    layer: &LayerNode,
    layer_bounds: Option<Rect>,
    flags: RecordFlags,
) -> bool {
    let props = store.props(record);
    resized_or_flagged(store, record, layer.size, flags)
        || store.graphics_layer(record) != layer.graphics_layer.get()
        || props.and_then(|props| props.layer_bounds) != layer_bounds
        || props
            .map(|props| props.translated_content_offset)
            .unwrap_or_default()
            != layer.translated_content_offset
        || props.map(|props| props.isolation).unwrap_or_default() != layer.isolation
        || props.map(|props| props.cache_policy).unwrap_or_default() != layer.cache_policy
}

// Whether `record` takes another size or other flags than `size` and `flags`.
fn resized_or_flagged(
    store: &SceneStore,
    record: RecordId,
    size: Size,
    flags: RecordFlags,
) -> bool {
    let rect = store.node_rect(record);
    [rect.width, rect.height].map(f32::to_bits) != [size.width, size.height].map(f32::to_bits)
        || store.flags(record).without(RecordFlags::STORE_KEPT) != flags
}

// Writes `children` as the items of `record`: each child over the record at
// its place among the record's children, each draw over the equal draw at
// its place among the record's draws.
fn write_items(store: &mut SceneStore, record: RecordId, children: &[RenderNode]) {
    let mut old_child = 0;
    let mut old_draw = 0;
    let mut redrawn = false;
    let mut items = Vec::with_capacity(children.len());
    for_each_in_draw_order(children, |child| {
        let item = match child {
            RenderNode::Layer(layer) => {
                let old = next_item(store, record, &mut old_child, Lane::Children);
                let old = old.and_then(Item::as_child);
                let new = write_layer(store, Some(record), old, layer);
                replace(store, old, new)
            }
            RenderNode::Inline(inline) => {
                let old = next_item(store, record, &mut old_child, Lane::Children);
                let old = old.and_then(Item::as_child);
                let new = write_leaf(store, record, old, inline);
                replace(store, old, new)
            }
            RenderNode::Primitive(_) | RenderNode::DrawRun(_) => {
                match next_item(store, record, &mut old_draw, Lane::Draws) {
                    Some(old) if same_draw(store, old, child) => old,
                    old => {
                        if let Some(old) = old {
                            store.remove_item(old);
                        }
                        redrawn = true;
                        add_draw(store, child)
                    }
                }
            }
        };
        items.push(item);
    });
    while let Some(old) = next_item(store, record, &mut old_child, Lane::Children) {
        store.remove_item(old);
    }
    while let Some(old) = next_item(store, record, &mut old_draw, Lane::Draws) {
        store.remove_item(old);
        redrawn = true;
    }
    if redrawn || store.items(record) != items.as_slice() {
        store.note_content_change(record);
    }
    store.write_items(record, &items);
}

// Which of a record's items a walk over them reads.
#[derive(Clone, Copy, PartialEq)]
enum Lane {
    Children,
    Draws,
}

// The next item of `record` in `lane` from `cursor` on.
fn next_item(store: &SceneStore, record: RecordId, cursor: &mut usize, lane: Lane) -> Option<Item> {
    let items = store.items(record);
    let found = items[*cursor..]
        .iter()
        .position(|item| (item.kind() == ItemKind::Child) == (lane == Lane::Children))?;
    let item = items[*cursor + found];
    *cursor += found + 1;
    Some(item)
}

// Removes `old` when a new record took its place.
fn replace(store: &mut SceneStore, old: Option<RecordId>, new: RecordId) -> Item {
    if let Some(old) = old.filter(|&old| old != new) {
        store.remove_subtree(old);
    }
    Item::child(new)
}

fn same_draw(store: &SceneStore, item: Item, node: &RenderNode) -> bool {
    match (item.kind(), node) {
        (ItemKind::Draw, RenderNode::Primitive(entry)) => {
            matches!(&entry.node, PrimitiveNode::Draw(draw) if **draw == *store.draw(item))
        }
        (ItemKind::Text, RenderNode::Primitive(entry)) => {
            matches!(&entry.node, PrimitiveNode::Text(text) if **text == *store.text(item))
        }
        (ItemKind::Run, RenderNode::DrawRun(run)) => run == store.run(item),
        _ => false,
    }
}

fn add_draw(store: &mut SceneStore, node: &RenderNode) -> Item {
    match node {
        RenderNode::Primitive(entry) => match &entry.node {
            PrimitiveNode::Draw(draw) => store.add_draw((**draw).clone()),
            PrimitiveNode::Text(text) => store.add_text((**text).clone()),
        },
        RenderNode::DrawRun(run) => store.add_run(run.clone()),
        RenderNode::Layer(_) | RenderNode::Inline(_) => {
            unreachable!("a child record is no draw")
        }
    }
}

fn write_leaf(
    store: &mut SceneStore,
    parent: RecordId,
    old: Option<RecordId>,
    inline: &InlineNode,
) -> RecordId {
    let node = Some(inline.node_id);
    let kept = old.filter(|&old| store.kind(old) == RecordKind::Leaf && store.node(old) == node);
    let record = kept.unwrap_or_else(|| {
        store.insert(RecordSeed {
            kind: RecordKind::Leaf,
            node,
            parent: Some(parent),
        })
    });
    let flags = if inline.motion_context_animated {
        RecordFlags::MOTION_ANIMATED
    } else {
        RecordFlags::default()
    };
    let restyled = kept.is_some() && resized_or_flagged(store, record, inline.size, flags);
    store.write_size(record, inline.size.width, inline.size.height);
    let offset = store.content_offset(parent);
    let origin = Point::new(inline.origin.x - offset.x, inline.origin.y - offset.y);
    store.set_placement(
        record,
        origin,
        Some(ProjectiveTransform::translation(
            inline.origin.x,
            inline.origin.y,
        )),
    );
    store.write_flags(record, flags);
    if restyled {
        store.note_property_change(record);
    }
    write_items(store, record, &inline.content);
    record
}
