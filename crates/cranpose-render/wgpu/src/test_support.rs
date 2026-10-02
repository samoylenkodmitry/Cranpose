use cranpose_render_common::graph::{LayerNode, ProjectiveTransform, RenderNode};
use cranpose_ui_graphics::{GraphicsLayer, Rect};

pub fn layer_node(
    local_bounds: Rect,
    transform_to_parent: ProjectiveTransform,
    graphics_layer: GraphicsLayer,
    children: Vec<RenderNode>,
) -> LayerNode {
    LayerNode {
        local_bounds,
        transform_to_parent,
        graphics_layer: graphics_layer.into(),
        children,
        ..Default::default()
    }
}
