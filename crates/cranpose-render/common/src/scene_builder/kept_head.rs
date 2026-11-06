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
use crate::graph::{InlineNode, LayerNode, RenderNode};

/// A placed node lowered without its children: its own layer, or the node
/// drawn in its parent's layer, which follows the node where it moves while
/// the node's own state stays as it was.
pub struct KeptHead {
    node: KeptNode,
    lowered_with: Inherited,
}

// The node a head keeps, boxed: a record that keeps one holds a pointer.
enum KeptNode {
    Layer(Box<LayerNode>),
    Inline(Box<InlineNode>),
}

/// The node a [`KeptHead`] keeps, as its parent's layer holds it.
#[derive(Clone, Copy)]
pub enum KeptNodeRef<'a> {
    /// The node's own layer, or the wrapper of its outer draws.
    Layer(&'a LayerNode),
    /// The node drawn in its parent's layer.
    Inline(&'a InlineNode),
}

impl KeptHead {
    /// Keeps `node`, which a node lowered alone under `context`. Hands back
    /// a draw, which no node lowers to alone.
    pub fn new(node: RenderNode, context: &LowerContext) -> Result<Self, RenderNode> {
        let node = match node {
            RenderNode::Layer(layer) => KeptNode::Layer(layer),
            RenderNode::Inline(inline) => KeptNode::Inline(inline),
            draw @ (RenderNode::Primitive(_) | RenderNode::DrawRun(_)) => return Err(draw),
        };
        Ok(Self {
            node,
            lowered_with: Inherited::of(context),
        })
    }

    /// The node as its parent's layer holds it.
    pub fn node(&self) -> KeptNodeRef<'_> {
        match &self.node {
            KeptNode::Layer(layer) => KeptNodeRef::Layer(layer),
            KeptNode::Inline(inline) => KeptNodeRef::Inline(inline),
        }
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
            KeptNode::Layer(layer) => layer,
            KeptNode::Inline(inline) => {
                if let Some(position) = position {
                    inline.origin = Point::new(position.x + offset.x, position.y + offset.y);
                }
                return Some(*context);
            }
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
        recycle_lowered(match self.node {
            KeptNode::Layer(layer) => RenderNode::Layer(layer),
            KeptNode::Inline(inline) => RenderNode::Inline(inline),
        });
    }
}

// The layer of `node_id` inside `wrapper`, the wrapper of its outer draws.
fn own_layer(wrapper: &LayerNode, node_id: NodeId) -> Option<&LayerNode> {
    wrapper.children.iter().find_map(|child| match child {
        RenderNode::Layer(layer) if layer.node_id == Some(node_id) => Some(&**layer),
        _ => None,
    })
}
