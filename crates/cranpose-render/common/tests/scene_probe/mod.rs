//! What a built render graph actually paints, for tests that need to compare
//! the scene against the tree it was built from.

use cranpose_core::{MemoryApplier, NodeId};
use cranpose_render_common::{
    SceneUpdates,
    graph::{LayerNode, PrimitiveNode, RenderGraph, RenderNode},
    scene_builder::{build_graph_from_applier, update_graph_from_applier},
};
use cranpose_ui::{LayoutEngine, Size, TestComposition};
use cranpose_ui_graphics::{Brush, Color, DrawPrimitive};

/// Every string the layer and its descendants paint, in draw order.
pub fn painted_text(layer: &LayerNode, out: &mut Vec<String>) {
    out.extend(layer.painted_text());
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

pub fn with_applier<R>(
    composition: &mut TestComposition,
    read: impl FnOnce(&mut MemoryApplier) -> R,
) -> R {
    let runtime = composition.runtime_handle();
    let mut applier = composition.applier_mut();
    applier.set_runtime_handle(runtime);
    let result = read(&mut applier);
    applier.clear_runtime_handle();
    result
}

/// Recomposes, lays out and updates `graph` with the nodes the app shell
/// would route: content for reshaped, redrawn and restructured nodes, the
/// nodes whose layer properties changed and the moved ones apart. Returns
/// the moved nodes.
pub fn update_scene(
    composition: &mut TestComposition,
    root: NodeId,
    viewport: Size,
    graph: &mut RenderGraph,
) -> Vec<NodeId> {
    while composition.process_invalid_scopes().expect("recompose") {}
    with_applier(composition, |applier| {
        applier.compute_layout(root, viewport).expect("relayout");
        let geometry = cranpose_ui::take_geometry_scene_nodes();
        let mut content = geometry.reshaped;
        content.extend(cranpose_ui::take_draw_repass_nodes());
        content.extend(applier.take_structural_change_parents_attached_to(root));
        content.sort_unstable();
        content.dedup();
        let moved = geometry.moved;
        let mut layers = Vec::new();
        cranpose_ui::take_layer_property_repass_nodes_into(&mut layers);
        assert!(
            update_graph_from_applier(
                applier,
                graph,
                SceneUpdates {
                    content: &content,
                    layers: &layers,
                    moved: &moved,
                },
                1.0,
            ),
            "the scoped update applies"
        );
        moved
    })
}

/// The graph a scene built anew from the composition's current state.
pub fn fresh_graph(composition: &mut TestComposition, root: NodeId) -> RenderGraph {
    with_applier(composition, |applier| {
        build_graph_from_applier(applier, root, 1.0).expect("fresh graph")
    })
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
