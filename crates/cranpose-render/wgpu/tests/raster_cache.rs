mod support;

use cranpose_core::NodeId;
use cranpose_render_common::graph::{
    CachePolicy, DrawPrimitiveNode, IsolationReasons, LayerNode, PrimitiveEntry, PrimitiveNode,
    PrimitivePhase, ProjectiveTransform, RenderGraph, RenderNode, TextPrimitiveNode,
};
use cranpose_render_common::raster_cache::LayerRasterCacheHashes;
use cranpose_render_common::Renderer;
use cranpose_ui::text::{AnnotatedString, SpanStyle};
use cranpose_ui::{TextLayoutOptions, TextStyle};
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
        content_offset: Point::default(),
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

fn text_scroll_like_graph(y: f32) -> RenderGraph {
    RenderGraph::new(test_layer(
        Some(20_000),
        CachePolicy::None,
        Rect {
            x: 0.0,
            y: 0.0,
            width: 320.0,
            height: 140.0,
        },
        ProjectiveTransform::identity(),
        vec![RenderNode::Layer(Box::new(text_layer(
            77,
            16.0,
            y,
            "Markdown text row cache reuse",
        )))],
    ))
}

fn repeated_text_graph() -> RenderGraph {
    RenderGraph::new(test_layer(
        Some(20_001),
        CachePolicy::None,
        Rect {
            x: 0.0,
            y: 0.0,
            width: 360.0,
            height: 160.0,
        },
        ProjectiveTransform::identity(),
        vec![
            RenderNode::Layer(Box::new(text_layer(
                77,
                16.0,
                20.0,
                "Repeated markdown heading",
            ))),
            RenderNode::Layer(Box::new(text_layer(
                78,
                16.0,
                64.0,
                "Repeated markdown heading",
            ))),
        ],
    ))
}

fn text_layer(node_id: NodeId, x: f32, y: f32, text_value: &str) -> LayerNode {
    let local_bounds = Rect {
        x: 0.0,
        y: 0.0,
        width: 260.0,
        height: 36.0,
    };
    let text = PrimitiveEntry {
        phase: PrimitivePhase::BeforeChildren,
        node: PrimitiveNode::Text(Box::new(TextPrimitiveNode {
            node_id,
            rect: local_bounds,
            text: AnnotatedString::from(text_value),
            text_style: TextStyle::from_span_style(SpanStyle {
                color: Some(Color(0.88, 0.90, 0.96, 1.0)),
                ..Default::default()
            }),
            font_size: 14.0,
            layout_options: TextLayoutOptions::default(),
            clip: None,
        })),
    };
    test_layer(
        Some(node_id),
        CachePolicy::None,
        local_bounds,
        ProjectiveTransform::translation(x, y),
        vec![RenderNode::Primitive(text)],
    )
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

#[test]
fn static_text_glyph_atlas_reuses_raster_under_scroll_translation() {
    let mut renderer = match support::headless_renderer() {
        Ok(renderer) => renderer,
        Err(err) => {
            eprintln!(
                "skipping static text glyph atlas assertion because headless WGPU init failed: {}",
                err
            );
            return;
        }
    };

    renderer.scene_mut().graph = Some(text_scroll_like_graph(20.0));
    renderer
        .capture_frame(320, 140)
        .expect("first text capture should succeed");
    let first_stats = renderer.last_frame_stats().expect("first frame stats");

    renderer.scene_mut().graph = Some(text_scroll_like_graph(54.0));
    renderer
        .capture_frame(320, 140)
        .expect("translated text capture should succeed");
    let second_stats = renderer.last_frame_stats().expect("second frame stats");

    assert_eq!(first_stats.text_image_cache_misses, 0);
    assert!(
        first_stats.text_glyph_atlas_misses > 0,
        "first frame must populate the text glyph atlas: {first_stats:?}"
    );
    assert!(
        second_stats.text_glyph_atlas_hits >= first_stats.text_glyph_atlas_misses,
        "translated static text should reuse first-frame atlas glyphs: {second_stats:?}"
    );
    assert_eq!(
        second_stats.text_glyph_atlas_misses, 0,
        "scroll translation alone must not upload new atlas glyphs: {second_stats:?}"
    );
    assert_eq!(
        second_stats.text_image_raster_bytes, 0,
        "atlas hit frame should not allocate CPU text image raster bytes: {second_stats:?}"
    );
}

#[test]
fn text_glyph_atlas_reuses_identical_content_across_node_ids() {
    let mut renderer = match support::headless_renderer() {
        Ok(renderer) => renderer,
        Err(err) => {
            eprintln!(
                "skipping repeated text glyph atlas assertion because headless WGPU init failed: {}",
                err
            );
            return;
        }
    };

    renderer.scene_mut().graph = Some(repeated_text_graph());
    renderer
        .capture_frame(360, 160)
        .expect("repeated text capture should succeed");
    let stats = renderer.last_frame_stats().expect("frame stats");

    assert_eq!(
        stats.text_image_cache_misses, 0,
        "atlas-safe text should not populate whole text image cache: {stats:?}"
    );
    assert!(
        stats.text_glyph_atlas_hits > 0,
        "second identical text node should hit shared atlas glyphs: {stats:?}"
    );
    assert!(
        stats.text_glyph_atlas_misses > 0,
        "first repeated text node should populate atlas glyphs: {stats:?}"
    );
}
