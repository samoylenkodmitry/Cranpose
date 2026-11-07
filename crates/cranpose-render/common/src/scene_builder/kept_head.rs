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

/// Whether a kept node checks the state its context passes down against the
/// state it lowered with.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Inherits {
    /// The context may pass down other state: the node checks it.
    Check,
    /// The node lies beneath a node that kept its layer and the state it
    /// lowered with, and nothing between them changed: the context passes
    /// down what the node lowered with.
    AsLowered,
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
    #[expect(clippy::inline_always, reason = "out of line: scrolls 2% slower")]
    #[inline(always)]
    pub fn follow(
        &mut self,
        applier: &MemoryApplier,
        node_id: NodeId,
        context: &LowerContext,
        (position, own_layer, inherits): (Option<Point>, OwnLayer, Inherits),
    ) -> Option<KeptNodeRef<'_>> {
        debug_assert!(
            inherits == Inherits::Check || Inherited::of(context) == self.lowered_with,
            "a node beneath a kept layer inherits what it lowered with"
        );
        if inherits == Inherits::Check && Inherited::of(context) != self.lowered_with {
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
        let (wrapped, own) = node_own_layer(layer, node_id, context)?;
        let outer = wrapped.as_ref().unwrap_or(context);
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

    /// The lowered node the head keeps.
    pub fn into_node(self) -> RenderNode {
        match self.node {
            KeptNode::Layer(layer) => RenderNode::Layer(layer),
            KeptNode::Inline(inline) => RenderNode::Inline(inline),
        }
    }
}

/// What the children of `node_id` lower with, the node lowered alone into
/// `layer` under `context`: the state beneath its own layer. `None` when
/// `layer` wraps the node's outer draws and holds no layer of the node.
pub fn lowered_children_context(
    layer: &LayerNode,
    node_id: NodeId,
    context: &LowerContext,
) -> Option<LowerContext> {
    let (wrapped, own) = node_own_layer(layer, node_id, context)?;
    Some(wrapped.as_ref().unwrap_or(context).beneath_layer(own))
}

// The own layer of `node_id` in `layer`, the node lowered alone under
// `context`, which holds its children: inside the wrapper of the node's
// outer draws when `layer` is that wrapper, with the state beneath the
// wrapper.
fn node_own_layer<'a>(
    layer: &'a LayerNode,
    node_id: NodeId,
    context: &LowerContext,
) -> Option<(Option<LowerContext>, &'a LayerNode)> {
    match layer.wraps {
        Some(_) => Some((
            Some(context.beneath_layer(layer)),
            wrapped_layer(layer, node_id)?,
        )),
        None => Some((None, layer)),
    }
}

// The layer of `node_id` inside `wrapper`, the wrapper of its outer draws.
fn wrapped_layer(wrapper: &LayerNode, node_id: NodeId) -> Option<&LayerNode> {
    wrapper.children.iter().find_map(|child| match child {
        RenderNode::Layer(layer) if layer.node_id == Some(node_id) => Some(&**layer),
        _ => None,
    })
}
