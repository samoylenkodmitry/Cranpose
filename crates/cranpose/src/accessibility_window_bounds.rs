// Where the scene shows each node, for the bounds a reader is given: a
// graphics layer that moves, scales or turns a node moves what a reader
// reaches with what the eye and a finger reach, as Compose's semantics
// bounds follow `graphicsLayer`.

use cranpose_core::{NodeId, collections::map::HashMap};
use cranpose_render_common::RenderScene;
use cranpose_ui::{Rect, SemanticsNode};

use super::AccessibilityRect;

// The window bounds of each node a scene draws, by node.
pub(crate) type WindowBounds = HashMap<NodeId, Rect>;

// Replaces `bounds` with the window bounds of each node `scene` draws,
// keeping its capacity.
pub(crate) fn place(scene: &impl RenderScene, bounds: &mut WindowBounds) {
    bounds.clear();
    scene.node_window_bounds(&mut |node_id, rect| {
        bounds.insert(node_id, rect);
    });
}

// `node`'s bounds in the window: where the scene draws it, else where layout
// put it.
pub(crate) fn rect_of(node: &SemanticsNode, bounds: &WindowBounds) -> AccessibilityRect {
    let rect = bounds.get(&node.node_id).unwrap_or(&node.bounds);
    AccessibilityRect::new(rect.x, rect.y, rect.width, rect.height)
}
