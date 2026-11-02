// The subtree of a node whose layer holds a 3D space, lowered whole, for a
// renderer that keeps no render graph: a scene's surfaces draw out of the
// tree's order and take input through its eye, so the renderer keeps the
// subtree's layers to draw it again and to answer where its surfaces show.

use cranpose_core::{MemoryApplier, NodeId, collections::map::HashMap};
use cranpose_ui::Rect;

use super::{
    Level, LowerContext, NodePlace, inline_leaf::lower_placed_child, node_layer::Children,
    publish_content_geometry, read_placed_node_data, write_node_layer,
};
use crate::graph::{LayerNode, ProjectiveTransform, RenderNode};

/// The subtree of a node whose layer holds a 3D space, lowered whole.
pub struct SpaceFragment {
    node: NodeId,
    root: RenderNode,
}

impl SpaceFragment {
    /// The node whose subtree it is.
    pub fn node(&self) -> NodeId {
        self.node
    }

    /// The subtree as its parent's layer holds it.
    pub fn root(&self) -> &RenderNode {
        &self.root
    }

    /// The subtree's outermost layer.
    pub fn layer(&self) -> Option<&LayerNode> {
        match &self.root {
            RenderNode::Layer(layer) => Some(layer),
            _ => None,
        }
    }

    /// Publishes where the scenes in the subtree show the layers on their
    /// surfaces that publish window origins, and leaves each scene's eye the
    /// world it walks in, given `to_window`, the map from the subtree's
    /// parent to the window.
    pub fn publish(&self, applier: &MemoryApplier, to_window: ProjectiveTransform) {
        let Some(layer) = self.layer() else {
            return;
        };
        let to_window = layer.transform_to_parent.then(to_window);
        crate::space::for_each_window_eye(layer, to_window, &mut |tree, view, to_window| {
            if let Some(eye) = &tree.cells[0].scene.eye {
                eye.world.rebuild(|world| view.build_world(tree, world));
            }
            for seen in view
                .surfaces
                .iter()
                .filter(|seen| seen.layer.has_origin_sinks)
            {
                if let Some(content) = view.content_to_window(seen, to_window) {
                    publish_content_geometry(applier, seen.layer, content);
                }
            }
        });
    }

    /// Visits the node of each mesh surface in the subtree with the map from
    /// its own space to the window, as its scene's eye shows it, given
    /// `to_window` for the subtree's parent.
    pub fn surface_windows(
        &self,
        to_window: ProjectiveTransform,
        visit: &mut dyn FnMut(NodeId, ProjectiveTransform),
    ) {
        let Some(layer) = self.layer() else {
            return;
        };
        let to_window = layer.transform_to_parent.then(to_window);
        crate::space::for_each_window_eye(layer, to_window, &mut |_, view, to_window| {
            for seen in &view.surfaces {
                if let (Some(node_id), Some(content)) =
                    (seen.layer.node_id, view.content_to_window(seen, to_window))
                {
                    visit(node_id, content);
                }
            }
        });
    }

    /// Visits each layout node the subtree draws with the bounds of its rect
    /// in the window, given `to_window` for the subtree's parent and the
    /// maps of its mesh surfaces' nodes in `surfaces`.
    pub fn node_window_bounds(
        &self,
        to_window: ProjectiveTransform,
        surfaces: &HashMap<NodeId, ProjectiveTransform>,
        visit: &mut dyn FnMut(NodeId, Rect),
    ) {
        if let Some(layer) = self.layer() {
            crate::node_window_bounds::for_each_node_window_bounds(
                layer, to_window, surfaces, visit,
            );
        }
    }
}

/// Lowers the subtree of the placed node `node_id` at `place` under
/// `context` whole, for a node whose layer holds a 3D space, publishing its
/// window geometry as [`super::lower_node_alone`] does. `None` when the
/// node is gone or not placed, or is a window root below the root.
pub fn lower_space_fragment(
    applier: &MemoryApplier,
    (node_id, place): (NodeId, NodePlace),
    context: LowerContext,
) -> Option<SpaceFragment> {
    let level = match place {
        NodePlace::Root => Level::Root,
        NodePlace::Child => Level::Child,
    };
    let root = read_placed_node_data(applier, node_id, level, |data| {
        if place == NodePlace::Child {
            return lower_placed_child(applier, node_id, data, context);
        }
        let mut layer = crate::layer_recycling::layer_box();
        write_node_layer(
            applier,
            node_id,
            data,
            (context, Children::Lower),
            &mut layer,
        );
        RenderNode::Layer(layer)
    })?;
    Some(SpaceFragment {
        node: node_id,
        root,
    })
}
