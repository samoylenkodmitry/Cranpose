// The subtree of a placed child that did not change, lowered whole, for a
// renderer that keeps no render graph: while the child moves from update to
// update, the renderer pushes the subtree again from where the child now
// lies and lowers none of it again.

use cranpose_core::{MemoryApplier, NodeId};
use cranpose_ui::Point;

use super::{
    ChangedNodes, Level, LowerContext, RecordingStore, dirty_walk::patch_layer,
    inline_leaf::lower_placed_child, publish_retained_window_geometry, read_placed_node_data,
    recycle_lowered, translate_retained_child,
};
use crate::graph::RenderNode;

/// The subtree of a placed child lowered whole, which follows the child
/// where it moves while nothing in it changes.
pub struct MovedSubtree {
    node: NodeId,
    root: RenderNode,
    lowered_with: Inherited,
}

// What a subtree lowers with from above it that a move leaves as it was.
#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) struct Inherited {
    motion_context_animated: bool,
    translated_content_context: bool,
    store: RecordingStore,
}

impl Inherited {
    pub(super) fn of(context: &LowerContext) -> Self {
        Self {
            motion_context_animated: context.inherited_motion_context_animated,
            translated_content_context: context.inherited_translated_content_context,
            store: context.store,
        }
    }
}

impl MovedSubtree {
    /// The child whose subtree it is.
    pub fn node(&self) -> NodeId {
        self.node
    }

    /// The subtree as its parent's layer holds it.
    pub fn root(&self) -> &RenderNode {
        &self.root
    }

    /// Places the subtree where its child, unchanged since it was lowered,
    /// now lies under `context`: at `position` in its parent's content.
    /// Publishes where its layers that publish window origins now lie.
    /// False when `context` passes down other state than the subtree was
    /// lowered with.
    pub fn follow(
        &mut self,
        applier: &MemoryApplier,
        position: Point,
        context: &LowerContext,
    ) -> bool {
        if Inherited::of(context) != self.lowered_with {
            return false;
        }
        let offset = context.parent_content_offset;
        match &mut self.root {
            RenderNode::Layer(layer) => {
                translate_retained_child(layer, position, offset);
                if let Some(parent_abs) = &context.parent_abs {
                    publish_retained_window_geometry(applier, layer, parent_abs);
                }
            }
            RenderNode::Inline(inline) => {
                inline.origin = Point::new(position.x + offset.x, position.y + offset.y);
            }
            RenderNode::Primitive(_) | RenderNode::DrawRun(_) => return false,
        }
        true
    }

    /// Brings the subtree, placed under `context` since its child did not
    /// change, up to the nodes beneath the child that `changed` names, as a
    /// render graph brings its layers. False when it cannot: the subtree is
    /// then of no use.
    pub fn patch(
        &mut self,
        applier: &MemoryApplier,
        changed: &mut ChangedNodes,
        context: &LowerContext,
    ) -> bool {
        let RenderNode::Layer(layer) = &mut self.root else {
            return false;
        };
        let Some(parent_abs) = &context.parent_abs else {
            return false;
        };
        let children = parent_abs.children_of(layer);
        let inherited = context.inherited_translated_content_context;
        patch_layer(applier, layer, (children, inherited), changed)
    }

    /// Lets the subtree go, as [`recycle_lowered`] lets a lowered node go.
    pub fn recycle(self) {
        recycle_lowered(self.root);
    }
}

/// Where the placed child `node_id` lies in its parent's content, as its
/// layout placed it. `None` when the child is gone or not placed, or is a
/// window root.
pub fn placed_child_position(applier: &MemoryApplier, node_id: NodeId) -> Option<Point> {
    read_placed_node_data(applier, node_id, Level::Child, |data| {
        data.layout_state.position()
    })
}

/// Lowers the placed child `node_id` under `context` with its whole subtree,
/// publishing window geometry as [`super::lower_node_alone`] does. `None`
/// when the child is gone or not placed, or is a window root.
pub fn lower_moved_subtree(
    applier: &MemoryApplier,
    node_id: NodeId,
    context: &LowerContext,
) -> Option<MovedSubtree> {
    let root = read_placed_node_data(applier, node_id, Level::Child, |data| {
        lower_placed_child(applier, node_id, data, context)
    })?;
    Some(MovedSubtree {
        node: node_id,
        root,
        lowered_with: Inherited::of(context),
    })
}
