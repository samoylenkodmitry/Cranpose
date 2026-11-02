// A layout node lowered without the nodes of its children, for a renderer
// that walks the layout tree itself and keeps no render graph: it turns the
// node's draws into its own items, then visits each child, which it lowers
// the same way or keeps as it drew it before.

use cranpose_core::{MemoryApplier, NodeId};
use cranpose_ui::{Point, Rect};

use super::{
    Level, LowerContext, children_context, inline_leaf::lower_placed_child, layer_and_node_bounds,
    node_layer::Children, placed_transform, read_placed_node_data, shaped_graphics_layer,
    write_node_layer,
};
use crate::graph::{ProjectiveTransform, RenderNode};

/// Where a node a renderer lowers lies in its window.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum NodePlace {
    /// The window's root, drawn at the window's origin.
    Root,
    /// A child of another node.
    Child,
}

impl NodePlace {
    fn level(self) -> Level {
        match self {
            Self::Root => Level::Root,
            Self::Child => Level::Child,
        }
    }
}

/// A layout node lowered without the nodes of its children.
pub struct LoweredNode<'a> {
    /// The node as its parent's layer holds it: drawn in that layer, in a
    /// layer of its own, or in its own layer inside the wrapper of the draws
    /// it chains before its graphics layer.
    pub node: RenderNode,
    /// Its children, in the order they draw, whose nodes go between its
    /// draws before them and after them in its own layer, the layer whose
    /// `node_id` is the node's.
    pub children: &'a [NodeId],
    /// What its children lower with.
    pub child_context: &'a LowerContext,
}

/// Lowers the placed node `node_id` at `place` under `context` without its
/// children's nodes, publishes its window geometry, and hands it to
/// `visit`. `None` when the node is gone or not placed, or is a window root
/// below the root.
pub fn lower_node_alone<R>(
    applier: &MemoryApplier,
    (node_id, place): (NodeId, NodePlace),
    context: &LowerContext,
    visit: impl FnOnce(LoweredNode<'_>) -> R,
) -> Option<R> {
    read_placed_node_data(applier, node_id, place.level(), |data| {
        let children = data.children;
        if place == NodePlace::Child && children.is_empty() {
            let node = lower_placed_child(applier, node_id, data, context);
            return visit(LoweredNode {
                node,
                children,
                child_context: context,
            });
        }
        let mut layer = crate::layer_recycling::layer_box();
        let child_context = write_node_layer(
            applier,
            node_id,
            data,
            (context, Children::Leave),
            &mut layer,
        );
        visit(LoweredNode {
            node: RenderNode::Layer(layer),
            children,
            child_context: &child_context,
        })
    })
}

/// Hands `visit` the children of the placed node `node_id` at `place`, in
/// the order they draw, and what they lower with under `context`, for a
/// node a renderer keeps as it drew it before. `None` as for
/// [`lower_node_alone`].
pub fn visit_kept_node<R>(
    applier: &MemoryApplier,
    (node_id, place): (NodeId, NodePlace),
    context: &LowerContext,
    visit: impl FnOnce(&[NodeId], &LowerContext) -> R,
) -> Option<R> {
    read_placed_node_data(applier, node_id, place.level(), |data| {
        let child_context = children_context(&data.modifier_slices, &data.layout_state, context);
        visit(data.children, &child_context)
    })
}

/// Where a placed child lies in its parent's layer: the identity of the
/// outermost node it lowers to, as the node's `node_id` and `wraps` read,
/// and that node's transform to its parent.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ChildPlacement {
    /// The node's `node_id` and `wraps`.
    pub identity: (Option<NodeId>, Option<NodeId>),
    /// The node's transform to its parent.
    pub transform: ProjectiveTransform,
    /// The child's top left in its parent's content, as its layout placed it.
    pub position: Point,
}

/// Where the placed child `node_id` lies under `context`, read without
/// lowering it: what a renderer compares with the child it kept, whose own
/// state did not change since, and places the subtree it kept at. `None` as
/// for [`lower_node_alone`].
pub fn kept_child_placement(
    applier: &MemoryApplier,
    node_id: NodeId,
    context: &LowerContext,
) -> Option<ChildPlacement> {
    read_placed_node_data(applier, node_id, Level::Child, |data| {
        let slices = &data.modifier_slices;
        let size = data.layout_state.size();
        let position = data.layout_state.position();
        let offset = context.parent_content_offset;
        if slices.outer_draw_command_count() > 0 {
            return ChildPlacement {
                identity: (None, Some(node_id)),
                transform: placed_transform(Rect::from_size(size), position, None, offset),
                position,
            };
        }
        let (local_bounds, _) = layer_and_node_bounds(slices, size);
        let graphics_layer = shaped_graphics_layer(slices, local_bounds);
        ChildPlacement {
            identity: (Some(node_id), None),
            transform: placed_transform(local_bounds, position, graphics_layer.as_deref(), offset),
            position,
        }
    })
}

/// The bounds of the layer of the window's root `root`, in its own space:
/// what the window's shadow light is worked out from. `None` when the root
/// is not placed.
pub fn root_layer_bounds(applier: &MemoryApplier, root: NodeId) -> Option<Rect> {
    read_placed_node_data(applier, root, Level::Root, |data| {
        layer_and_node_bounds(&data.modifier_slices, data.layout_state.size()).0
    })
}

/// Frees the boxes the lowered nodes of a walk left in the pool past those
/// the next walk takes, and the recordings of the commands it did not draw
/// again, when the walk ends.
pub fn release_lowered() {
    crate::layer_recycling::keep_for_next_walk();
    let tight =
        cranpose_ui::current_memory_headroom() == cranpose_ui_graphics::MemoryHeadroom::Tight;
    crate::thread_state::with(|state| state.redrawn.borrow_mut().end_update(tight));
}

/// Lets go what walks keep for the next: the boxes of lowered nodes and the
/// recordings of the commands they draw again.
pub fn release_walk_storage() {
    crate::layer_recycling::release();
    crate::thread_state::with(|state| state.redrawn.borrow_mut().clear());
}
