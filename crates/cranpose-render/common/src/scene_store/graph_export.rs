// The render graph of a store, for the readers that walk the graph until
// they read the store.

use std::{
    cell::Cell,
    hash::{Hash, Hasher},
};

use cranpose_core::SceneUpdate;
use cranpose_ui_graphics::FxHasher;

use super::{ItemRef, LayerRef, RecordKind, SceneStore};
use crate::graph::{
    ContentChanges, InlineNode, LayerChange, LayerNode, PrimitiveEntry, PrimitiveNode,
    PrimitivePhase, RenderNode,
};

pub(crate) fn export(store: &SceneStore) -> LayerNode {
    let update = u64::from(store.update().since(SceneUpdate::default()));
    store
        .root_layer()
        .map(|root| export_layer(root, update))
        .unwrap_or_default()
}

fn layer_change(change: super::RecordChange) -> LayerChange {
    let mut hasher = FxHasher::default();
    change.hash(&mut hasher);
    let [a, b, c, d, ..] = hasher.finish().to_le_bytes();
    LayerChange::numbered(u32::from_le_bytes([a, b, c, d]))
}

fn export_layer(layer: LayerRef<'_>, update: u64) -> LayerNode {
    let store = layer.store();
    let props = layer.props();
    let content_changes = props.map_or_else(ContentChanges::default, |props| {
        let (last, run) = props.content_run.numbered(store.update(), update);
        ContentChanges::numbered(last, run)
    });
    LayerNode {
        node_id: layer.node_id(),
        wraps: layer.wraps(),
        local_bounds: layer.local_bounds(),
        node_bounds: layer.node_bounds(),
        transform_to_parent: layer.transform_to_parent(),
        content_offset: layer.content_offset(),
        motion_context_animated: layer.motion_context_animated(),
        translated_content_context: layer.translated_content_context(),
        translated_content_offset: layer.translated_content_offset(),
        origin_in_parent: match layer.kind() {
            RecordKind::DrawGroup => cranpose_ui::Point::default(),
            RecordKind::Layer | RecordKind::Wrapper | RecordKind::Leaf => layer.origin_in_parent(),
        },
        graphics_layer: store.graphics_layer(layer.id()).cloned().into(),
        clip_to_bounds: layer.clip_to_bounds(),
        hit_test: layer.hit_test().cloned(),
        has_hit_targets: layer.has_hit_targets(),
        space: Cell::new(Some(layer.has_space())),
        has_origin_sinks: layer.has_origin_sinks(),
        contained: Cell::new(Some(layer.draws_within_bounds())),
        isolation: layer.isolation(),
        draw_phase: layer.draw_phase(),
        cache_policy: layer.cache_policy(),
        cache_hashes: Cell::new(None),
        content_changes,
        change: layer_change(layer.change()),
        own_change: layer_change(layer.own_change()),
        children: export_items(layer, update),
    }
}

fn export_items(layer: LayerRef<'_>, update: u64) -> Vec<RenderNode> {
    let mut after_children = false;
    let mut nodes = Vec::with_capacity(layer.item_count());
    for item in layer.items() {
        let phase = if after_children {
            PrimitivePhase::AfterChildren
        } else {
            PrimitivePhase::BeforeChildren
        };
        nodes.push(match item {
            ItemRef::Draw(draw) => RenderNode::Primitive(PrimitiveEntry {
                phase,
                node: PrimitiveNode::Draw(Box::new(draw.clone())),
            }),
            ItemRef::Text(text) => RenderNode::Primitive(PrimitiveEntry {
                phase,
                node: PrimitiveNode::Text(Box::new(text.clone())),
            }),
            ItemRef::Run(run) => RenderNode::DrawRun(run.clone()),
            ItemRef::Layer(child) => {
                after_children |= child.kind() != RecordKind::DrawGroup;
                RenderNode::Layer(Box::new(export_layer(child, update)))
            }
            ItemRef::Leaf(leaf) => {
                after_children = true;
                let mut inline = Box::<InlineNode>::default();
                inline.node_id = leaf.owner().expect("a leaf draws for a node");
                inline.origin = leaf.origin();
                inline.size = leaf.size();
                inline.motion_context_animated = leaf.motion_context_animated();
                inline.change = layer_change(leaf.change());
                inline.content = export_items(leaf, update);
                RenderNode::Inline(inline)
            }
        });
    }
    nodes
}
