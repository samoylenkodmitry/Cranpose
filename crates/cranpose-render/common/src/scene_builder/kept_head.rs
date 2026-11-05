// The layer of a placed node with children, lowered without them, for a
// renderer that keeps no render graph: while the node's own draws do not
// change, the renderer pushes the layer again from where the node now lies,
// and only the node's children lower again.

use cranpose_core::{MemoryApplier, NodeId};

use super::{
    Inherited, Level, LowerContext, children_context, publish_layer_window_geometry,
    read_placed_node_data, recycle_lowered, translate_retained_child,
};
use crate::graph::RenderNode;

/// The layer of a placed node lowered without its children, which follows
/// the node where it moves while the node's own draws stay as they were.
pub struct KeptHead {
    node: NodeId,
    layer: RenderNode,
    lowered_with: Inherited,
}

impl KeptHead {
    /// Keeps `layer`, the layer the node `node` lowered alone under
    /// `context`.
    pub fn new(node: NodeId, layer: RenderNode, context: &LowerContext) -> Self {
        Self {
            node,
            layer,
            lowered_with: Inherited::of(context),
        }
    }

    /// The node whose layer it is.
    pub fn node(&self) -> NodeId {
        self.node
    }

    /// Places the layer where its node, whose own draws did not change since
    /// it lowered, now lies under `context`, publishes where the layer lies
    /// in its window when the node reads it, and hands the layer to `visit`
    /// with the node's children and what they lower with. `None`, with
    /// nothing handed, when the node is gone or not placed, or `context`
    /// passes down other state than the layer lowered with.
    pub fn follow<R>(
        &mut self,
        applier: &MemoryApplier,
        context: &LowerContext,
        visit: impl FnOnce(&RenderNode, &[NodeId], &LowerContext) -> R,
    ) -> Option<R> {
        if Inherited::of(context) != self.lowered_with {
            return None;
        }
        let layer_node = &mut self.layer;
        read_placed_node_data(applier, self.node, Level::Child, |data| {
            let RenderNode::Layer(layer) = layer_node else {
                return None;
            };
            translate_retained_child(
                layer,
                data.layout_state.position(),
                context.parent_content_offset,
            );
            if layer.has_origin_sinks
                && let Some(parent_abs) = &context.parent_abs
            {
                let child_abs = parent_abs.children_of(layer);
                publish_layer_window_geometry(
                    &data.modifier_slices,
                    (layer, parent_abs, &child_abs),
                    data.layout_state.size(),
                );
            }
            let inner = children_context(&data.modifier_slices, &data.layout_state, context);
            Some(visit(layer_node, data.children, &inner))
        })?
    }

    /// Lets the layer go, as [`recycle_lowered`] lets a lowered node go.
    pub fn recycle(self) {
        recycle_lowered(self.layer);
    }
}
