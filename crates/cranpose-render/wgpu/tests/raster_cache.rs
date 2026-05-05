mod support;

use cranpose_core::NodeId;
use cranpose_render_common::graph::{
    CachePolicy, DrawPrimitiveNode, IsolationReasons, LayerNode, PrimitiveEntry, PrimitiveNode,
    PrimitivePhase, ProjectiveTransform, RenderGraph, RenderNode,
};
use cranpose_render_common::raster_cache::LayerRasterCacheHashes;
use cranpose_render_common::Renderer;
use cranpose_ui_graphics::{Brush, Color, GraphicsLayer, Point, Rect};

fn test_layer(
    node_id: Option<NodeId>,
    cache_policy: CachePolicy,
    local_bounds: Rect,
    transform_to_parent: ProjectiveTransform,
    children: Vec<RenderNode>,
) -> LayerNode {
    LayerNode {
        node_id,
        local_bounds,
        transform_to_parent,
        motion_context_animated: false,
        translated_content_context: false,
        translated_content_offset: Point::default(),
        graphics_layer: GraphicsLayer::default(),
        clip_to_bounds: false,
        shadow_clip: None,
        hit_test: None,
        has_hit_targets: false,
        isolation: IsolationReasons::default(),
        cache_policy,
        cache_hashes: LayerRasterCacheHashes::default(),
        cache_hashes_valid: false,
        children,
    }
}

fn card_layer(node_id: NodeId, y: f32) -> LayerNode {
    let local_bounds = Rect {
        x: 0.0,
        y: 0.0,
        width: 96.0,
        height: 28.0,
    };
    let primitive = PrimitiveEntry {
        phase: PrimitivePhase::BeforeChildren,
        node: PrimitiveNode::Draw(DrawPrimitiveNode {
            primitive: cranpose_ui_graphics::DrawPrimitive::Rect {
                rect: local_bounds,
                brush: Brush::solid(Color(0.15, 0.35, 0.85, 1.0)),
            },
            clip: None,
        }),
    };
    let mut layer = test_layer(
        Some(node_id),
        CachePolicy::Auto,
        local_bounds,
        ProjectiveTransform::translation(12.0, y),
        vec![RenderNode::Primitive(primitive)],
    );
    layer.graphics_layer.alpha = 0.85;
    layer
}

fn scroll_like_graph(offsets: &[f32]) -> RenderGraph {
    let children = offsets
        .iter()
        .enumerate()
        .map(|(index, y)| RenderNode::Layer(Box::new(card_layer(index + 1, *y))))
        .collect();
    RenderGraph::new(test_layer(
        Some(10_000),
        CachePolicy::None,
        Rect {
            x: 0.0,
            y: 0.0,
            width: 128.0,
            height: 160.0,
        },
        ProjectiveTransform::identity(),
        children,
    ))
}

#[test]
fn capture_frame_reuses_cached_child_layers_during_rigid_scroll() {
    let mut renderer = match support::headless_renderer() {
        Ok(renderer) => renderer,
        Err(err) => {
            eprintln!(
                "skipping rigid scroll cache reuse assertion because headless WGPU init failed: {}",
                err
            );
            return;
        }
    };

    renderer.scene_mut().graph = Some(scroll_like_graph(&[8.0, 44.0, 80.0, 116.0]));
    renderer
        .capture_frame(160, 180)
        .expect("first capture should succeed");
    let first_stats = renderer.last_frame_stats().expect("first frame stats");

    renderer.scene_mut().graph = Some(scroll_like_graph(&[15.25, 51.25, 87.25, 123.25]));
    renderer
        .capture_frame(160, 180)
        .expect("second capture should succeed");
    let second_stats = renderer.last_frame_stats().expect("second frame stats");

    assert_eq!(first_stats.layer_cache_hits, 0);
    assert_eq!(first_stats.layer_cache_misses, 4);
    assert_eq!(second_stats.layer_cache_hits, 4);
    assert_eq!(second_stats.layer_cache_misses, 0);
    assert_eq!(second_stats.layer_cache_evictions, 0);
    assert!(
        second_stats.isolated_layer_renders < first_stats.isolated_layer_renders,
        "cached child layers should avoid repainting isolated child surfaces"
    );
    assert!(
        second_stats.isolated_layer_pixels < first_stats.isolated_layer_pixels,
        "rigid scroll should reduce isolated layer repaint area after cache warmup"
    );
    assert!(
        second_stats.offscreen_acquires < first_stats.offscreen_acquires,
        "cache reuse should reduce offscreen acquisitions on the second frame"
    );
}
