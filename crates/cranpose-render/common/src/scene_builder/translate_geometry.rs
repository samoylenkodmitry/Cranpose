use cranpose_core::{MemoryApplier, NodeId, collections::map::HashMap};
use cranpose_ui::{Point, Rect};
use cranpose_ui_graphics::GraphicsLayer;

use super::{
    AbsOrigin, Level, LowerContext, PlacedChild, TranslateAncestorContext, placed_transform,
    publish_retained_window_geometry, read_placed_node_data, release_for_rebuild, text_node,
    write_node_layer,
};
use crate::{
    NodeUpdate,
    graph::{LayerNode, ProjectiveTransform},
    layer_transform::layer_transform_to_window,
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
        container: &LayerNode,
        ancestors: TranslateAncestorContext,
    ) -> TranslateAncestorContext {
        TranslateAncestorContext {
            inherited_motion_context_animated: container.motion_context_animated,
            update: ancestors.update,
            inherited_translated_content_context: ancestors.inherited_translated_content_context
                || container.translated_content_context,
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
    layer: &mut LayerNode,
    position: Point,
    content_offset: Point,
) {
    layer.transform_to_parent = placed_transform(
        layer.local_bounds,
        position,
        layer.graphics_layer.get(),
        content_offset,
    );
    layer.origin_in_parent = position;
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
    // The child, a label no record names that only changed size, writes its
    // text into the layer where it now lies, or is built there again.
    Resized,
}

// Brings a child a translated container keeps to its placement among the
// container's `children`, as `placed.kept` says. A subtree that publishes
// window origins publishes them again from where it now lies. Inline: most
// kept children only move.
#[inline]
pub(super) fn move_kept_child(
    applier: &MemoryApplier,
    layer: &mut LayerNode,
    placed: &PlacedChild,
    children: TranslateAncestorContext,
    dirty_nodes: &mut HashMap<NodeId, NodeUpdate>,
) {
    match placed.kept {
        KeptChild::Left => {}
        KeptChild::Moved => {
            let position = placed.geometry.position;
            translate_retained_child(layer, position, children.parent_content_offset);
            if layer.has_origin_sinks {
                publish_retained_window_geometry(applier, layer, children.parent_abs);
            }
        }
        KeptChild::Retexted | KeptChild::Resized => {
            write_kept_label(applier, layer, placed, children, dirty_nodes);
        }
    }
}

// Writes the text of a label the step keeps where it now lies. A label
// whose text cannot be written in place stays dirty for the walk beneath,
// or is built again when no record names it.
#[inline(never)]
fn write_kept_label(
    applier: &MemoryApplier,
    layer: &mut LayerNode,
    placed: &PlacedChild,
    children: TranslateAncestorContext,
    dirty_nodes: &mut HashMap<NodeId, NodeUpdate>,
) {
    let written = text_node::retext_kept_leaf(applier, layer, children) == Some(true);
    match placed.kept {
        KeptChild::Retexted if written => {
            dirty_nodes.remove(&placed.id);
        }
        KeptChild::Resized if !written => rebuild_kept_child(applier, layer, placed.id, children),
        _ => {}
    }
}

// Builds the layer of `node_id`, a child the step keeps, again from the node
// where it now lies.
fn rebuild_kept_child(
    applier: &MemoryApplier,
    layer: &mut LayerNode,
    node_id: NodeId,
    children: TranslateAncestorContext,
) {
    release_for_rebuild(layer);
    read_placed_node_data(applier, node_id, Level::Child, |data| {
        write_node_layer(
            applier,
            node_id,
            data,
            LowerContext::beneath(children),
            layer,
        );
    });
    layer.note_content_change(children.update);
}
