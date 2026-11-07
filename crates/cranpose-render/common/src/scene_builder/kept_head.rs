// A placed node lowered without its children, which a renderer that keeps
// no render graph keeps with the record of the node: while the node does
// not change, the renderer pushes it again where the node now lies and
// takes its children from their own records, lowering none of them again.
// While nothing in the node's subtree changes, the renderer may link the
// heads of its children into it, and push the subtree as a retained graph
// pushes its nodes.

use cranpose_core::{MemoryApplier, NodeId};
use cranpose_ui::Point;

use super::{
    Inherited, Level, LowerContext, Position, publish_layer_window_geometry, read_placed_node_data,
    recycle_lowered, refresh_layer_properties, translate_retained_child,
};
use crate::graph::{InlineNode, LayerNode, PrimitivePhase, RenderNode};

/// A placed node lowered without its children: its own layer, or the node
/// drawn in its parent's layer, which follows the node where it moves while
/// the node's own state stays as it was.
pub struct KeptHead {
    node: KeptNode,
    lowered_with: Inherited,
    // Whether the heads of the node's children are linked into it.
    subtree: bool,
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
    /// The node's layer with its children linked into it, for a head that
    /// keeps its subtree.
    Subtree(&'a LayerNode),
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
            subtree: false,
        })
    }

    /// Whether the head keeps its node's subtree, as
    /// [`KeptHead::link_children`] linked it.
    pub fn holds_subtree(&self) -> bool {
        self.subtree
    }

    /// Puts the `count` nodes `children` yields, the nodes of the node's
    /// children in the order they draw, each with its own children linked,
    /// into the node's own layer between its draws before and after them, as
    /// the node lowers with its children: the head then keeps its node's
    /// subtree.
    pub fn link_children(
        &mut self,
        node_id: NodeId,
        (count, children): (usize, impl Iterator<Item = RenderNode>),
    ) {
        self.subtree = true;
        let KeptNode::Layer(layer) = &mut self.node else {
            return;
        };
        let wraps = layer.wraps.is_some();
        let Some(own) = own_layer_mut(layer, node_id) else {
            return;
        };
        let after = own
            .children
            .iter()
            .position(|child| child.draw_phase() == Some(PrimitivePhase::AfterChildren))
            .unwrap_or(own.children.len());
        let draws_after = own.children.len() - after;
        own.children.reserve_exact(count);
        own.children.extend(children);
        own.children[after..].rotate_left(draws_after);
        own.refresh_child_facts();
        if wraps {
            layer.refresh_child_facts();
        }
    }

    /// Takes the nodes of the node's children out of its own layer, in the
    /// order they draw, and hands each to `child` as a head that keeps its
    /// own children linked: the head keeps its node alone again.
    pub fn unlink_children(&mut self, node_id: NodeId, mut child: impl FnMut(Self)) {
        if !std::mem::take(&mut self.subtree) {
            return;
        }
        let context = self.lowered_with.lean_context();
        let KeptNode::Layer(layer) = &mut self.node else {
            return;
        };
        let wrapper = layer.wraps.map(|_| context.beneath_layer(layer));
        let wraps = wrapper.is_some();
        let Some(own) = own_layer_mut(layer, node_id) else {
            return;
        };
        let inner = wrapper.as_ref().unwrap_or(&context).beneath_layer(own);
        if let (Some(first), Some(last)) = (
            own.children.iter().position(is_child_node),
            own.children.iter().rposition(is_child_node),
        ) {
            for node in own.children.drain(first..=last) {
                match Self::new(node, &inner) {
                    Ok(head) => child(Self {
                        subtree: true,
                        ..head
                    }),
                    Err(draw) => recycle_lowered(draw),
                }
            }
        }
        own.refresh_child_facts();
        if wraps {
            layer.refresh_child_facts();
        }
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
    #[inline(always)]
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
        // A kept subtree pushes only under a context with no place in the
        // window: none of its nodes publishes one.
        if self.subtree {
            return Some(KeptNodeRef::Subtree(layer));
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
// wrapper. In line: `KeptHead::follow` runs it for each node it places.
#[inline(always)]
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

// The own layer of `node_id` in `layer`, the node lowered alone, which holds
// its children: inside the wrapper of the node's outer draws when `layer` is
// that wrapper.
fn own_layer_mut(layer: &mut LayerNode, node_id: NodeId) -> Option<&mut LayerNode> {
    if layer.wraps.is_none() {
        return Some(layer);
    }
    layer.children.iter_mut().find_map(|child| match child {
        RenderNode::Layer(own) if own.node_id == Some(node_id) => Some(&mut **own),
        _ => None,
    })
}

// Whether `node`, in a node's own layer, is the node of one of its children:
// a layer of a node or the wrapper of one, or a node drawn inline. The
// layers of the node's own draw groups belong to no node.
fn is_child_node(node: &RenderNode) -> bool {
    match node {
        RenderNode::Layer(layer) => layer.node_id.is_some() || layer.wraps.is_some(),
        RenderNode::Inline(_) => true,
        RenderNode::Primitive(_) | RenderNode::DrawRun(_) => false,
    }
}

// The layer of `node_id` inside `wrapper`, the wrapper of its outer draws.
fn wrapped_layer(wrapper: &LayerNode, node_id: NodeId) -> Option<&LayerNode> {
    wrapper.children.iter().find_map(|child| match child {
        RenderNode::Layer(layer) if layer.node_id == Some(node_id) => Some(&**layer),
        _ => None,
    })
}
