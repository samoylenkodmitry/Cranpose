use cranpose_core::{MemoryApplier, NodeId, RecordId, collections::map::HashMap};
use cranpose_ui::{Point, Rect};
use cranpose_ui_graphics::GraphicsLayer;

use super::{
    AbsOrigin, LowerContext, TranslateAncestorContext, inline_leaf::retext_kept_inline,
    placed_transform, publish_retained_window_geometry, text_node, translate_layer::PlacedChild,
};
use crate::{
    NodeUpdate,
    graph::ProjectiveTransform,
    layer_transform::layer_transform_to_window,
    scene_store::{LayerRef, RecordKind, SceneStore},
};

#[derive(Clone, Copy)]
pub(super) struct TranslateGeometry {
    pub(super) content_offset: Point,
    pub(super) window_transform: ProjectiveTransform,
    pub(super) top_left: Point,
    child_origin: Point,
}

impl TranslateGeometry {
    pub(super) fn new(
        layout_state: &cranpose_ui::widgets::LayoutState,
        graphics_layer: &GraphicsLayer,
        local_bounds: Rect,
        parent_abs: AbsOrigin,
    ) -> Self {
        let content_offset = layout_state.content_offset();
        let top_left = Point {
            x: parent_abs.content_origin.x + layout_state.position().x,
            y: parent_abs.content_origin.y + layout_state.position().y,
        };
        let window_transform = layer_transform_to_window(
            parent_abs.window_transform,
            top_left,
            local_bounds,
            graphics_layer,
        );
        Self {
            content_offset,
            window_transform,
            top_left,
            child_origin: Point {
                x: top_left.x + content_offset.x,
                y: top_left.y + content_offset.y,
            },
        }
    }

    // What the children of `container`, the layer laid out here beneath
    // `ancestors`, take from it.
    pub(super) fn children_context(
        self,
        container: LayerRef<'_>,
        ancestors: TranslateAncestorContext,
    ) -> TranslateAncestorContext {
        TranslateAncestorContext {
            inherited_motion_context_animated: container.motion_context_animated(),
            inherited_translated_content_context: ancestors.inherited_translated_content_context
                || container.translated_content_context(),
            parent_content_offset: self.content_offset,
            parent_abs: self.child_abs(),
        }
    }

    pub(super) fn child_abs(self) -> AbsOrigin {
        AbsOrigin {
            content_origin: self.child_origin,
            window_transform: self.window_transform,
        }
    }
}

pub(super) fn translate_retained_child(
    store: &mut SceneStore,
    layer: RecordId,
    position: Point,
    content_offset: Point,
) {
    let transform = placed_transform(
        store.layer(layer).local_bounds(),
        position,
        store.graphics_layer(layer),
        content_offset,
    );
    store.set_placement(layer, position, transform);
    store.note_place_change(layer);
}

// What a translated container does with the layer of a previous child it
// places.
#[derive(Clone, Copy)]
pub(super) enum KeptChild {
    // The walk beneath the container brings the layer up to date.
    Left,
    // The layer follows the child's placement: the child was not dirty, it
    // only moved, which this step settles, or its layer properties or
    // draws changed, which the walk beneath settles.
    Moved,
    // The child, a label kept for new text or a new size, writes its text
    // into the layer where it now lies.
    Retexted,
}

// Brings a child a translated container keeps to its placement among the
// container's `children`, as `placed.kept` says. A subtree that publishes
// window origins publishes them again from where it now lies. Inline: most
// kept children only move.
#[inline]
pub(super) fn move_kept_child(
    applier: &MemoryApplier,
    store: &mut SceneStore,
    record: RecordId,
    placed: &PlacedChild,
    children: TranslateAncestorContext,
    dirty_nodes: &mut HashMap<NodeId, NodeUpdate>,
) {
    let position = placed.geometry.position;
    match (placed.kept, store.layer(record).kind()) {
        (KeptChild::Left, _) => {}
        (KeptChild::Moved, RecordKind::Leaf) => {
            let offset = children.parent_content_offset;
            store.set_placement(
                record,
                position,
                ProjectiveTransform::translation(position.x + offset.x, position.y + offset.y),
            );
            store.note_place_change(record);
        }
        (KeptChild::Moved, _) => {
            translate_retained_child(store, record, position, children.parent_content_offset);
            if store.layer(record).has_origin_sinks() {
                publish_retained_window_geometry(applier, store.layer(record), children.parent_abs);
            }
        }
        (KeptChild::Retexted, _) => {
            write_kept_label(applier, store, record, placed, children, dirty_nodes);
        }
    }
}

// Writes the text of a label the step keeps where it now lies. A label
// whose text cannot be written in place stays dirty for the walk beneath.
#[inline(never)]
fn write_kept_label(
    applier: &MemoryApplier,
    store: &mut SceneStore,
    record: RecordId,
    placed: &PlacedChild,
    children: TranslateAncestorContext,
    dirty_nodes: &mut HashMap<NodeId, NodeUpdate>,
) {
    let written = match store.layer(record).kind() {
        RecordKind::Leaf => {
            retext_kept_inline(applier, store, record, LowerContext::beneath(children))
        }
        RecordKind::Layer | RecordKind::Wrapper | RecordKind::DrawGroup => {
            text_node::retext_kept_leaf(applier, store, record, children) == Some(true)
        }
    };
    if written {
        dirty_nodes.remove(&placed.id);
    }
}
