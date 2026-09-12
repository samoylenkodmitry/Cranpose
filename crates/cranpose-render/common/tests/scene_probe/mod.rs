//! What a built render graph actually paints, for tests that need to compare
//! the scene against the tree it was built from.

use cranpose_render_common::graph::{LayerNode, PrimitiveNode, RenderNode};
use cranpose_ui_graphics::DrawPrimitive;

/// Every string the layer and its descendants paint, in draw order.
pub fn painted_text(layer: &LayerNode, out: &mut Vec<String>) {
    for child in &layer.children {
        match child {
            RenderNode::Primitive(primitive) => {
                if let PrimitiveNode::Text(text) = &primitive.node {
                    out.push(text.text.text.clone());
                }
            }
            RenderNode::Layer(child) => painted_text(child, out),
            RenderNode::DrawRun(run) => {
                for primitive in run.primitives() {
                    if let DrawPrimitive::Text(text) = primitive {
                        out.push(text.text.to_string());
                    }
                }
            }
        }
    }
}
