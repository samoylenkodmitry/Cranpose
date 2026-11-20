use cranpose_core::{MemoryApplier, NodeId, RecordId, collections::map::HashMap};
use cranpose_ui::{ModifierNodeSlices, Point, Rect, Size};
use cranpose_ui_graphics::GraphicsLayer;

use super::{
    AbsOrigin, Inherited, LowerContext, ParentOrigin, TranslateAncestorContext,
    inline_leaf::retext_kept_inline, placing_transform, publish_retained_window_geometry,
    text_node, translate_layer::PlacedChild,
};
use crate::{
    NodeUpdate,
    graph::ProjectiveTransform,
    layer_transform::laid_out_transform_to_window,
    scene_store::{LayerRef, RecordKind, SceneStore},
};

#[derive(Clone, Copy)]
pub(super) struct TranslateGeometry {
    pub(super) content_offset: Point,
    // Where the node lies in the window; `None` when no node of the update
    // reads it.
    window: Option<WindowPlace>,
}

#[derive(Clone, Copy)]
struct WindowPlace {
    top_left: Point,
    transform: Option<ProjectiveTransform>,
}

impl TranslateGeometry {
    // The geometry of a node laid out as `layout_state`, its place in the
    // window worked out from `parent_abs` when the update reads it.
    pub(super) fn new(
        layout_state: &cranpose_ui::widgets::LayoutState,
        graphics_layer: &GraphicsLayer,
        local_bounds: Rect,
        parent_abs: Option<AbsOrigin>,
    ) -> Self {
        let window = parent_abs.map(|parent_abs| {
            let top_left = Point {
                x: parent_abs.content_origin.x + layout_state.position().x,
                y: parent_abs.content_origin.y + layout_state.position().y,
            };
            WindowPlace {
                top_left,
                transform: laid_out_transform_to_window(
                    parent_abs.window_transform,
                    top_left,
                    local_bounds,
                    graphics_layer,
                ),
            }
        });
        Self {
            content_offset: layout_state.content_offset(),
            window,
        }
    }

    // Hands `slices`, of a node of `size`, where the node lies in the
    // window when the update worked it out, else its pointer input size
    // alone: a node whose slices hold a window origin sink always has it
    // worked out.
    pub(super) fn publish(self, slices: &ModifierNodeSlices, size: Size) {
        match self.window {
            Some(window) => slices.publish_window_geometry(window.top_left, window.transform, size),
            None => slices.publish_pointer_input_size(size),
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
            inherited: Inherited {
                motion_context_animated: container.motion_context_animated(),
                translated_content_context: ancestors.inherited.translated_content_context
                    || container.translated_content_context(),
                parent_content_offset: self.content_offset,
            },
            parent_abs: self
                .child_abs()
                .map_or(ParentOrigin::Unread, ParentOrigin::Known),
        }
    }

    pub(super) fn child_abs(self) -> Option<AbsOrigin> {
        self.window.map(|window| AbsOrigin {
            content_origin: Point {
                x: window.top_left.x + self.content_offset.x,
                y: window.top_left.y + self.content_offset.y,
            },
            window_transform: window.transform,
        })
    }
}

pub(super) fn translate_retained_child(
    store: &mut SceneStore,
    layer: RecordId,
    position: Point,
    content_offset: Point,
) {
    let transform = placing_transform(
        store.layer(layer).local_bounds(),
        position,
        store.graphics_layer(layer),
        content_offset,
    );
    store.set_placement(layer, position, transform);
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
    match placed.kept {
        KeptChild::Left => {}
        KeptChild::Moved if store.layer(record).kind() == RecordKind::Leaf => {
            store.move_leaf(record, position);
        }
        KeptChild::Moved => move_kept_layer(applier, store, record, position, children),
        KeptChild::Retexted => {
            write_kept_label(applier, store, record, placed, children, dirty_nodes);
        }
    }
}

// Moves `record`, a layer the step keeps, to `position`; a subtree that
// publishes window origins publishes them again from there.
#[inline(never)]
fn move_kept_layer(
    applier: &MemoryApplier,
    store: &mut SceneStore,
    record: RecordId,
    position: Point,
    children: TranslateAncestorContext,
) {
    translate_retained_child(
        store,
        record,
        position,
        children.inherited.parent_content_offset,
    );
    if store.layer(record).has_origin_sinks()
        && let Some(parent_abs) = children.parent_abs.resolve(store)
    {
        publish_retained_window_geometry(applier, store.layer(record), parent_abs);
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
            let context = LowerContext::beneath(children, store);
            retext_kept_inline(applier, store, record, context)
        }
        RecordKind::Layer | RecordKind::Wrapper | RecordKind::DrawGroup => {
            text_node::retext_kept_leaf(applier, store, record, children) == Some(true)
        }
    };
    if written {
        dirty_nodes.remove(&placed.id);
    }
}
