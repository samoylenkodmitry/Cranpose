// Where a render graph draws each layout node in the window.

use cranpose_core::{NodeId, collections::map::HashMap};
use cranpose_ui_graphics::Rect;

use crate::graph::{LayerNode, ProjectiveTransform, RenderNode};

// Visits each layout node that `layer` and the layers beneath it draw, with
// the bounds of its rect in the window: through its placement, the graphics
// layers above it and its own, given `to_window` for `layer`'s parent. A mesh
// surface's node in `surfaces` maps its own space to the window as a scene's
// eye shows it, and the nodes on it follow that map.
pub(crate) fn for_each_node_window_bounds(
    layer: &LayerNode,
    to_window: ProjectiveTransform,
    surfaces: &HashMap<NodeId, ProjectiveTransform>,
    visit: &mut dyn FnMut(NodeId, Rect),
) {
    let shown = layer.node_id.and_then(|node_id| surfaces.get(&node_id));
    let to_window = shown
        .copied()
        .unwrap_or_else(|| layer.transform_to_parent.then(to_window));
    if let Some(node_id) = layer.node_id {
        visit(node_id, to_window.bounds_for_rect(layer.node_rect()));
    }
    for child in &layer.children {
        match child {
            RenderNode::Layer(child) => {
                for_each_node_window_bounds(child, to_window, surfaces, visit);
            }
            RenderNode::Inline(inline) => {
                let inline_to_window = inline.transform_to_parent().then(to_window);
                visit(
                    inline.node_id,
                    inline_to_window.bounds_for_rect(inline.node_rect()),
                );
            }
            RenderNode::Primitive(_) | RenderNode::DrawRun(_) => {}
        }
    }
}
