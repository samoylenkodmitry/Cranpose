use cranpose_render_common::graph::{
    CachePolicy, IsolationReasons, LayerNode, ProjectiveTransform, RenderNode,
};
use cranpose_render_common::raster_cache::LayerRasterCacheHashes;
use cranpose_ui_graphics::{GraphicsLayer, Rect};

pub fn layer_node(
    local_bounds: Rect,
    transform_to_parent: ProjectiveTransform,
    graphics_layer: GraphicsLayer,
    children: Vec<RenderNode>,
) -> LayerNode {
    LayerNode {
        node_id: None,
        local_bounds,
        transform_to_parent,
        graphics_layer,
        clip_to_bounds: false,
        shadow_clip: None,
        hit_test: None,
        isolation: IsolationReasons::default(),
        cache_policy: CachePolicy::None,
        cache_hashes: LayerRasterCacheHashes::default(),
        children,
    }
}
