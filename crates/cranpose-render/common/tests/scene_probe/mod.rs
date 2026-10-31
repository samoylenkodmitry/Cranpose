//! What a built render graph actually paints, for tests that need to compare
//! the scene against the tree it was built from.

use cranpose_core::{MemoryApplier, NodeId};
use cranpose_render_common::{
    graph::{LayerNode, PrimitiveNode, RenderGraph, RenderNode},
    scene_builder::{build_graph_from_applier, update_graph_from_applier},
};
use cranpose_ui::{LayoutEngine, Size, TestComposition};
use cranpose_ui_graphics::{Brush, Color, DrawPrimitive};

pub(crate) fn painted_text(layer: &LayerNode, out: &mut Vec<String>) {
    out.extend(layer.painted_text());
}

pub(crate) fn initial_graph(
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

pub(crate) fn with_applier<R>(
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

pub(crate) fn update_scene(
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
        let moved: Vec<NodeId> = geometry.moved.iter().map(|moved| moved.node).collect();
        let mut layers = Vec::new();
        cranpose_ui::take_layer_property_repass_nodes_into(&mut layers);
        assert!(
            update_graph_from_applier(
                applier,
                graph,
                &content
                    .iter()
                    .map(|&id| (id, cranpose_render_common::NodeUpdate::Content))
                    .chain(
                        layers
                            .iter()
                            .map(|&id| (id, cranpose_render_common::NodeUpdate::Layer))
                    )
                    .chain(
                        moved
                            .iter()
                            .map(|&id| (id, cranpose_render_common::NodeUpdate::Moved))
                    )
                    .collect::<Vec<_>>(),
                1.0,
            ),
            "the scoped update applies"
        );
        moved
    })
}

pub(crate) fn fresh_graph(composition: &mut TestComposition, root: NodeId) -> RenderGraph {
    with_applier(composition, |applier| {
        build_graph_from_applier(applier, root, 1.0).expect("fresh graph")
    })
}

pub(crate) fn painted_solid_rect_colors(layer: &LayerNode, colors: &mut Vec<Color>) {
    nodes_solid_rect_colors(&layer.children, colors);
}

fn nodes_solid_rect_colors(nodes: &[RenderNode], colors: &mut Vec<Color>) {
    for child in nodes {
        match child {
            RenderNode::Layer(child) => painted_solid_rect_colors(child, colors),
            RenderNode::Inline(inline) => nodes_solid_rect_colors(&inline.content, colors),
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
