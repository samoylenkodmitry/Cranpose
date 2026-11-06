// A placed node lowered without its children, which a renderer that keeps
// no render graph keeps with the record of the node: while the node does
// not change, the renderer pushes it again where the node now lies and
// takes its children from their own records, lowering none of them again.

use cranpose_core::{MemoryApplier, NodeId};
use cranpose_ui::Point;

use super::{
    Inherited, Level, LowerContext, publish_layer_window_geometry, read_placed_node_data,
    recycle_lowered, translate_retained_child,
};
use crate::graph::{LayerNode, RenderNode};

/// A placed node lowered without its children: its own layer, or the node
/// drawn in its parent's layer, which follows the node where it moves while
/// the node's own state stays as it was.
pub struct KeptHead {
    node: RenderNode,
    lowered_with: Inherited,
}

impl KeptHead {
    /// Keeps `node`, which a node lowered alone under `context`.
    pub fn new(node: RenderNode, context: &LowerContext) -> Self {
        Self {
            node,
            lowered_with: Inherited::of(context),
        }
    }

    /// The node as its parent's layer holds it.
    pub fn node(&self) -> &RenderNode {
        &self.node
    }

    /// Places the head of `node_id`, whose own state did not change since
    /// it lowered, where it now lies under `context`: at `position` in its
    /// parent's content when the parent placed it anew, else where it lay.
    /// Publishes where its layer lies in its window when the node reads it.
    /// Returns what the node's children lower with. `None` when `context`
    /// passes down other state than the head lowered with, or the node is
    /// gone.
    pub fn follow(
        &mut self,
        applier: &MemoryApplier,
        node_id: NodeId,
        context: &LowerContext,
        position: Option<Point>,
    ) -> Option<LowerContext> {
        if Inherited::of(context) != self.lowered_with {
            return None;
        }
        let offset = context.parent_content_offset;
        let layer = match &mut self.node {
            RenderNode::Layer(layer) => layer,
            RenderNode::Inline(inline) => {
                if let Some(position) = position {
                    inline.origin = Point::new(position.x + offset.x, position.y + offset.y);
                }
                return Some(*context);
            }
            RenderNode::Primitive(_) | RenderNode::DrawRun(_) => return None,
        };
        if let Some(position) = position {
            translate_retained_child(layer, position, offset);
        }
        // The wrapper of a node's outer draws holds the node's own layer,
        // which holds its children.
        let wrapper = layer.wraps.is_some().then(|| context.beneath_layer(layer));
        let (outer, own) = match wrapper {
            Some(outer) => (outer, own_layer(layer, node_id)?),
            None => (*context, &**layer),
        };
        if own.has_origin_sinks
            && let Some(parent_abs) = &outer.parent_abs
        {
            read_placed_node_data(applier, node_id, Level::Child, |data| {
                let child_abs = parent_abs.children_of(own);
                publish_layer_window_geometry(
                    &data.modifier_slices,
                    (own, parent_abs, &child_abs),
                    data.layout_state.size(),
                );
            })?;
        }
        Some(outer.beneath_layer(own))
    }

    /// Lets the node go, as [`recycle_lowered`] lets a lowered node go.
    pub fn recycle(self) {
        recycle_lowered(self.node);
    }
}

// The layer of `node_id` inside `wrapper`, the wrapper of its outer draws.
fn own_layer(wrapper: &LayerNode, node_id: NodeId) -> Option<&LayerNode> {
    wrapper.children.iter().find_map(|child| match child {
        RenderNode::Layer(layer) if layer.node_id == Some(node_id) => Some(&**layer),
        _ => None,
    })
}
