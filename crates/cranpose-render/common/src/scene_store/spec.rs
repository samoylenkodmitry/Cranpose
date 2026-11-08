// Records written from a layer tree built by hand: a renderer's tests and
// overlays describe what they draw as a tree, and the store holds it as
// records.

use cranpose_core::RecordId;
use cranpose_ui::Point;

use super::{Item, RecordFlags, RecordKind, RecordSeed, SceneStore};
use crate::graph::{
    InlineNode, LayerNode, PrimitiveNode, PrimitivePhase, ProjectiveTransform, RenderNode,
    for_each_in_draw_order,
};

pub(super) fn write_layer(
    store: &mut SceneStore,
    parent: Option<RecordId>,
    layer: &LayerNode,
) -> RecordId {
    let (kind, node) = match (layer.draw_phase, layer.wraps) {
        (Some(_), _) => (RecordKind::DrawGroup, layer.node_id),
        (None, Some(wrapped)) => (RecordKind::Wrapper, Some(wrapped)),
        (None, None) => (RecordKind::Layer, layer.node_id),
    };
    let record = store.insert(RecordSeed { kind, node, parent });
    store.set_content_offset(record, layer.content_offset);
    store.set_size(record, layer.size.width, layer.size.height);
    let origin = match kind {
        RecordKind::DrawGroup => layer
            .transform_to_parent
            .as_translation()
            .unwrap_or_default(),
        RecordKind::Layer | RecordKind::Wrapper | RecordKind::Leaf => layer.origin_in_parent,
    };
    store.set_placement(record, origin, layer.transform_to_parent);
    let node_rect = cranpose_ui::Rect::from_size(layer.size);
    store.set_graphics_layer(record, layer.graphics_layer.get().cloned());
    store.edit_props(record, |props| {
        props.layer_bounds = layer.layer_bounds.filter(|bounds| *bounds != node_rect);
        props.translated_content_offset = layer.translated_content_offset;
        props.hit_test.clone_from(&layer.hit_test);
        props.isolation = layer.isolation;
        props.cache_policy = layer.cache_policy;
    });
    let mut flags = RecordFlags::default();
    for (on, flag) in [
        (layer.clip_to_bounds, RecordFlags::CLIP_TO_BOUNDS),
        (layer.motion_context_animated, RecordFlags::MOTION_ANIMATED),
        (
            layer.translated_content_context,
            RecordFlags::TRANSLATED_CONTENT,
        ),
        (layer.has_origin_sinks, RecordFlags::HAS_ORIGIN_SINKS),
        (
            layer.draw_phase == Some(PrimitivePhase::AfterChildren),
            RecordFlags::AFTER_CHILDREN,
        ),
    ] {
        if on {
            flags = flags.with(flag);
        }
    }
    store.set_flags(record, flags);
    let mut items = Vec::with_capacity(layer.children.len());
    for_each_in_draw_order(&layer.children, |child| {
        items.push(write_node(store, record, child));
    });
    store.set_items(record, &items);
    record
}

fn write_node(store: &mut SceneStore, parent: RecordId, node: &RenderNode) -> Item {
    match node {
        RenderNode::Primitive(entry) => match &entry.node {
            PrimitiveNode::Draw(draw) => store.add_draw((**draw).clone()),
            PrimitiveNode::Text(text) => store.add_text((**text).clone()),
        },
        RenderNode::DrawRun(run) => store.add_run(run.clone()),
        RenderNode::Layer(layer) => Item::child(write_layer(store, Some(parent), layer)),
        RenderNode::Inline(inline) => Item::child(write_leaf(store, parent, inline)),
    }
}

fn write_leaf(store: &mut SceneStore, parent: RecordId, inline: &InlineNode) -> RecordId {
    let record = store.insert(RecordSeed {
        kind: RecordKind::Leaf,
        node: Some(inline.node_id),
        parent: Some(parent),
    });
    store.set_size(record, inline.size.width, inline.size.height);
    let offset = store.content_offset(parent);
    let origin = Point::new(inline.origin.x - offset.x, inline.origin.y - offset.y);
    store.set_placement(
        record,
        origin,
        ProjectiveTransform::translation(inline.origin.x, inline.origin.y),
    );
    if inline.motion_context_animated {
        store.set_flags(record, RecordFlags::MOTION_ANIMATED);
    }
    let mut items = Vec::with_capacity(inline.content.len());
    for_each_in_draw_order(&inline.content, |child| {
        items.push(write_node(store, record, child));
    });
    store.set_items(record, &items);
    record
}
