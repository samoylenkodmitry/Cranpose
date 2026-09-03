use cranpose_render_common::{
    graph::{CachePolicy, IsolationReasons, LayerNode, ProjectiveTransform, RenderNode},
    raster_cache::LayerRasterCacheHashes,
};
use cranpose_ui_graphics::{GraphicsLayer, Point, Rect};

pub fn layer_node(
    local_bounds: Rect,
    transform_to_parent: ProjectiveTransform,
    graphics_layer: GraphicsLayer,
    children: Vec<RenderNode>,
) -> LayerNode {
    LayerNode {
        node_id: None,
        wraps: None,
        local_bounds,
        transform_to_parent,
        motion_context_animated: false,
        translated_content_context: false,
        translated_content_offset: Point::default(),
        scene_children_origin: Point::default(),
        scene_children_layer_translation: Point::default(),
        content_offset: Point::default(),
        graphics_layer,
        clip_to_bounds: false,
        shadow_clip: None,
        hit_test: None,
        has_hit_targets: false,
        has_origin_sinks: false,
        isolation: IsolationReasons::default(),
        cache_policy: CachePolicy::None,
        cache_hashes: LayerRasterCacheHashes::default(),
        cache_hashes_valid: false,
        children,
    }
}
