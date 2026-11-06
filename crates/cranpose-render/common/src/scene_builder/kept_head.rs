// A placed node lowered without its children, which a renderer that keeps
// no render graph keeps with the record of the node: while the node does
// not change, the renderer pushes it again where the node now lies and
// takes its children from their own records, lowering none of them again.

use cranpose_core::{MemoryApplier, NodeId};
use cranpose_ui::Point;

use super::{
    Inherited, Level, LowerContext, Position, publish_layer_window_geometry, read_placed_node_data,
    recycle_lowered, refresh_layer_properties, translate_retained_child,
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

/// What changed of a kept node's own layer since the node lowered.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum OwnLayer {
    /// Nothing: the node only moved, or lies beneath a node that changed.
    AsLowered,
    /// Its graphics-layer properties, while it drew the same and lay where
    /// it lay.
    Changed,
}

/// The node a [`KeptHead`] keeps, placed where it now lies, as its parent's
/// layer holds it.
pub enum KeptNodeRef<'a> {
    /// The node's own layer, or the wrapper of its outer draws, and what the
    /// node's children lower with.
    Layer(&'a LayerNode, LowerContext),
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

    /// Places the head of `node_id`, which draws as it drew when it
    /// lowered, where it now lies under `context`: at `position` in its
    /// parent's content when the parent placed it anew, else where it lay.
    /// Takes the graphics-layer properties the node has now when `own_layer`
    /// changed. Publishes where its layer lies in its window when the node
    /// reads it. Returns the node, with what its children lower with for a
    /// layer. `None` when `context` passes down other state than the head
    /// lowered with, the node is gone, or a node with a new layer no longer
    /// fits its kept one or draws in its parent's layer.
    pub fn follow(
        &mut self,
        applier: &MemoryApplier,
        node_id: NodeId,
        context: &LowerContext,
        (position, own_layer): (Option<Point>, OwnLayer),
    ) -> Option<KeptNodeRef<'_>> {
        if Inherited::of(context) != self.lowered_with {
            return None;
        }
        if own_layer == OwnLayer::Changed {
            let KeptNode::Layer(layer) = &mut self.node else {
                return None;
            };
            refresh_layer_properties(applier, layer, (Level::Child, Position::Kept), context)?;
        }
        let offset = context.parent_content_offset;
        let layer = match &mut self.node {
            KeptNode::Layer(layer) => layer,
            KeptNode::Inline(inline) => {
                if let Some(position) = position {
                    inline.origin = Point::new(position.x + offset.x, position.y + offset.y);
                }
                return Some(KeptNodeRef::Inline(inline));
            }
        };
        if let Some(position) = position {
            translate_retained_child(layer, position, offset);
        }
        // The wrapper of a node's outer draws holds the node's own layer,
        // which holds its children.
        let wrapped;
        let (outer, own) = match layer.wraps {
            Some(_) => {
                wrapped = context.beneath_layer(layer);
                (&wrapped, wrapped_layer(layer, node_id)?)
            }
            None => (context, &**layer),
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
        let inner = outer.beneath_layer(own);
        Some(KeptNodeRef::Layer(layer, inner))
    }

    /// Lets the node go, as [`recycle_lowered`] lets a lowered node go.
    pub fn recycle(self) {
        recycle_lowered(self.into_node());
    }

    /// Lets the node go before it lowers again in this update: its new
    /// draws record into the recordings it held.
    pub fn recycle_to_redraw(self) {
        crate::layer_recycling::recycle_to_redraw(self.into_node());
    }

    fn into_node(self) -> RenderNode {
        match self.node {
            KeptNode::Layer(layer) => RenderNode::Layer(layer),
            KeptNode::Inline(inline) => RenderNode::Inline(inline),
        }
    }
}

// The layer of `node_id` inside `wrapper`, the wrapper of its outer draws.
fn wrapped_layer(wrapper: &LayerNode, node_id: NodeId) -> Option<&LayerNode> {
    wrapper.children.iter().find_map(|child| match child {
        RenderNode::Layer(layer) if layer.node_id == Some(node_id) => Some(&**layer),
        _ => None,
    })
}
