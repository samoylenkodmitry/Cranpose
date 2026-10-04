//! What a built render graph actually paints, for tests that need to compare
//! the scene against the tree it was built from.

use cranpose_core::NodeId;
use cranpose_render_common::{
    graph::{LayerNode, PrimitiveNode, RenderGraph, RenderNode},
    scene_builder::build_graph_from_applier,
};
use cranpose_ui::{LayoutEngine, Size, TestComposition};
use cranpose_ui_graphics::{Brush, Color, DrawPrimitive};

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

pub fn initial_graph(
    composition: &mut TestComposition,
    root: NodeId,
    viewport: Size,
) -> RenderGraph {
    let runtime = composition.runtime_handle();
    let mut applier = composition.applier_mut();
    applier.set_runtime_handle(runtime);
    applier
        .compute_layout(root, viewport)
        .expect("composition lays out");
    let graph = build_graph_from_applier(&applier, root, 1.0).expect("initial scene graph");
    applier.clear_runtime_handle();
    graph
}

pub fn painted_solid_rect_colors(layer: &LayerNode, colors: &mut Vec<Color>) {
    for child in &layer.children {
        match child {
            RenderNode::Layer(child) => painted_solid_rect_colors(child, colors),
            RenderNode::DrawRun(run) => {
                for primitive in run.primitives() {
                    if let DrawPrimitive::Rect {
                        brush: Brush::Solid(color),
                        ..
                    } = primitive
                    {
                        colors.push(color);
                    }
                }
            }
            RenderNode::Primitive(entry) => {
                if let PrimitiveNode::Draw(draw) = &entry.node
                    && let DrawPrimitive::Rect {
                        brush: Brush::Solid(color),
                        ..
                    } = &draw.primitive
                {
                    colors.push(*color);
                }
            }
        }
    }
}
