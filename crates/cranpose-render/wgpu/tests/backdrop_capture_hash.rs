use cranpose_render_common::{
    Renderer,
    graph::{
        CachePolicy, DrawCommandId, DrawRunNode, LayerNode, PrimitivePhase, ProjectiveTransform,
        RenderGraph, RenderNode,
    },
    style_shared::DrawPlacement,
};
use cranpose_render_wgpu::CapturedFrame;
use cranpose_ui_graphics::{
    Brush, Color, DrawPrimitive, GraphicsLayer, LayerShape, Rect, RenderEffect, RoundedCornerShape,
};

use crate::support;

const WIDTH: u32 = 128;
const HEIGHT: u32 = 88;
const CONTENT: Rect = Rect {
    x: 0.0,
    y: 0.0,
    width: WIDTH as f32,
    height: HEIGHT as f32,
};

fn blur_backdrop(x: f32, y: f32, width: f32, height: f32) -> LayerNode {
    let bounds = Rect {
        x: 0.0,
        y: 0.0,
        width,
        height,
    };
    let mut backdrop = LayerNode {
        node_id: Some(20),
        local_bounds: bounds,
        transform_to_parent: ProjectiveTransform::translation(x, y),
        graphics_layer: GraphicsLayer {
            backdrop_effect: Some(RenderEffect::blur(2.0)),
            ..GraphicsLayer::default()
        }
        .into(),
        cache_policy: CachePolicy::Auto,
        children: vec![support::solid_rect(bounds, Color(0.10, 0.25, 0.95, 0.55))],
        ..LayerNode::default()
    };
    backdrop.recompute_raster_cache_hashes();
    backdrop
}

fn rounded_clip_scene(clip_x: f32) -> RenderGraph {
    let backdrop = blur_backdrop(16.0, 12.0, 12.0, 64.0);
    let clipped = LayerNode {
        node_id: Some(21),
        local_bounds: Rect {
            x: 0.0,
            y: 0.0,
            width: 32.0,
            height: 32.0,
        },
        transform_to_parent: ProjectiveTransform::translation(clip_x, 10.0),
        graphics_layer: GraphicsLayer {
            clip: true,
            shape: LayerShape::Rounded(RoundedCornerShape::uniform(8.0)),
            ..GraphicsLayer::default()
        }
        .into(),
        children: vec![RenderNode::Layer(Box::new(LayerNode {
            node_id: Some(22),
            local_bounds: CONTENT,
            transform_to_parent: ProjectiveTransform::translation(-clip_x, -10.0),
            children: vec![RenderNode::DrawRun(DrawRunNode::for_command(
                PrimitivePhase::BeforeChildren,
                Some(DrawCommandId {
                    node_id: 22,
                    command_index: 0,
                    placement: DrawPlacement::Behind,
                }),
                vec![DrawPrimitive::Rect {
                    rect: CONTENT,
                    brush: Brush::solid(Color(0.9, 0.3, 0.2, 1.0)),
                    stroke: None,
                }],
            ))],
            ..LayerNode::default()
        }))],
        ..LayerNode::default()
    };
    RenderGraph::new(LayerNode {
        local_bounds: CONTENT,
        children: vec![
            support::solid_rect(CONTENT, Color::BLACK),
            RenderNode::Layer(Box::new(clipped)),
            RenderNode::Layer(Box::new(backdrop)),
        ],
        ..LayerNode::default()
    })
}

fn subpixel_scene(edge_x: f32, alpha: f32) -> RenderGraph {
    let backdrop = blur_backdrop(28.0, 12.0, 72.0, 64.0);
    let moving_rect = Rect {
        x: 0.0,
        y: 0.0,
        width: 18.0,
        height: 24.0,
    };
    let moving_run = DrawRunNode::for_command(
        PrimitivePhase::BeforeChildren,
        Some(DrawCommandId {
            node_id: 21,
            command_index: 0,
            placement: DrawPlacement::Behind,
        }),
        vec![DrawPrimitive::Rect {
            rect: moving_rect,
            brush: Brush::solid(Color::WHITE.with_alpha(alpha)),
            stroke: None,
        }],
    );
    let moving_layer = LayerNode {
        node_id: Some(21),
        local_bounds: moving_rect,
        transform_to_parent: ProjectiveTransform::translation(edge_x, 32.0),
        children: vec![RenderNode::DrawRun(moving_run)],
        ..LayerNode::default()
    };

    RenderGraph::new(LayerNode {
        local_bounds: CONTENT,
        children: vec![
            support::solid_rect(CONTENT, Color::BLACK),
            RenderNode::Layer(Box::new(moving_layer)),
            RenderNode::Layer(Box::new(backdrop)),
        ],
        ..LayerNode::default()
    })
}

fn inspector_glass(cache_policy: CachePolicy) -> RenderGraph {
    let bounds = Rect {
        x: 16.0,
        y: 12.0,
        width: 56.0,
        height: 48.0,
    };
    RenderGraph::new(LayerNode {
        local_bounds: CONTENT,
        children: vec![RenderNode::Layer(Box::new(LayerNode {
            node_id: Some(71),
            local_bounds: bounds,
            graphics_layer: GraphicsLayer {
                backdrop_effect: Some(RenderEffect::blur(6.0)),
                ..GraphicsLayer::default()
            }
            .into(),
            cache_policy,
            children: vec![support::solid_rect(bounds, Color(1.0, 1.0, 1.0, 0.35))],
            ..LayerNode::default()
        }))],
        ..LayerNode::default()
    })
}

fn painted_page(color: Color) -> RenderGraph {
    RenderGraph::new(LayerNode {
        local_bounds: CONTENT,
        children: vec![support::solid_rect(CONTENT, color)],
        ..LayerNode::default()
    })
}

fn nested_backdrop_scene(
    page_color: Color,
    composite_color: Color,
    root_color: Color,
    composite_cache: CachePolicy,
    backdrop_cache: CachePolicy,
) -> RenderGraph {
    let backdrop_bounds = Rect {
        x: 0.0,
        y: 0.0,
        width: 16.0,
        height: 16.0,
    };
    let backdrop = LayerNode {
        node_id: Some(42),
        local_bounds: backdrop_bounds,
        transform_to_parent: ProjectiveTransform::translation(4.0, 4.0),
        graphics_layer: GraphicsLayer {
            backdrop_effect: Some(RenderEffect::blur(6.0)),
            ..GraphicsLayer::default()
        }
        .into(),
        cache_policy: backdrop_cache,
        children: vec![support::solid_rect(
            backdrop_bounds,
            Color(1.0, 1.0, 1.0, 0.35),
        )],
        ..LayerNode::default()
    };
    let nested = LayerNode {
        node_id: Some(43),
        local_bounds: Rect {
            x: 0.0,
            y: 0.0,
            width: 40.0,
            height: 28.0,
        },
        transform_to_parent: ProjectiveTransform::translation(4.0, 4.0),
        graphics_layer: GraphicsLayer {
            alpha: 0.5,
            ..GraphicsLayer::default()
        }
        .into(),
        children: vec![RenderNode::Layer(Box::new(backdrop))],
        ..LayerNode::default()
    };
    let composite_bounds = Rect {
        x: 0.0,
        y: 0.0,
        width: 20.0,
        height: 16.0,
    };
    let composite = LayerNode {
        node_id: Some(44),
        local_bounds: composite_bounds,
        transform_to_parent: ProjectiveTransform::translation(7.0, 6.0),
        graphics_layer: GraphicsLayer {
            alpha: 0.5,
            ..GraphicsLayer::default()
        }
        .into(),
        cache_policy: composite_cache,
        children: vec![support::solid_rect(composite_bounds, composite_color)],
        ..LayerNode::default()
    };
    let parent_bounds = Rect {
        x: 0.0,
        y: 0.0,
        width: 48.0,
        height: 36.0,
    };
    let parent = LayerNode {
        node_id: Some(41),
        local_bounds: parent_bounds,
        transform_to_parent: ProjectiveTransform::uniform_scale(2.0)
            .then(ProjectiveTransform::translation(20.0, 8.0)),
        children: vec![
            support::solid_rect(parent_bounds, page_color),
            RenderNode::Layer(Box::new(composite)),
            RenderNode::Layer(Box::new(nested)),
        ],
        ..LayerNode::default()
    };
    RenderGraph::new(LayerNode {
        node_id: Some(40),
        local_bounds: CONTENT,
        children: vec![
            support::solid_rect(CONTENT, root_color),
            RenderNode::Layer(Box::new(parent)),
        ],
        ..LayerNode::default()
    })
}

fn verify_nested_backdrop_page_provenance(composite_cache: CachePolicy) {
    let Some(mut cached) = renderer() else {
        return;
    };
    let mut fresh = support::LockedRenderer::beside_locked().expect("reference renderer");
    let backdrop_region = Rect {
        x: 36.0,
        y: 24.0,
        width: 32.0,
        height: 32.0,
    };
    let scene = |page_color, composite_color, root_color, backdrop_cache| {
        nested_backdrop_scene(
            page_color,
            composite_color,
            root_color,
            composite_cache,
            backdrop_cache,
        )
    };
    let mut warm = capture(
        &mut cached,
        scene(Color::BLACK, Color::RED, Color::RED, CachePolicy::Auto),
    );
    for _ in 1..3 {
        warm = capture(
            &mut cached,
            scene(Color::BLACK, Color::RED, Color::RED, CachePolicy::Auto),
        );
    }
    let ancestor_changed = capture(
        &mut cached,
        scene(Color::BLACK, Color::RED, Color::BLUE, CachePolicy::Auto),
    );
    support::assert_same_bytes(
        "unrelated ancestor pixels do not change the nested backdrop",
        32,
        &support::region_pixels(&warm, backdrop_region),
        &support::region_pixels(&ancestor_changed, backdrop_region),
    );

    let page_changed = capture(
        &mut cached,
        scene(Color::BLUE, Color::RED, Color::BLUE, CachePolicy::Auto),
    );
    let page_reference = capture(
        &mut fresh,
        scene(Color::BLUE, Color::RED, Color::BLUE, CachePolicy::None),
    );
    let page_pixels = support::region_pixels(&page_changed, backdrop_region);
    let page_reference_pixels = support::region_pixels(&page_reference, backdrop_region);
    assert!(
        support::region_pixels(&ancestor_changed, backdrop_region) != page_pixels,
        "changing the immediate source page must change the nested backdrop"
    );
    support::assert_same_bytes(
        "nested backdrop after immediate source page changes",
        32,
        &page_pixels,
        &page_reference_pixels,
    );

    let composite_changed = capture(
        &mut cached,
        scene(Color::BLUE, Color::GREEN, Color::BLUE, CachePolicy::Auto),
    );
    let composite_reference = capture(
        &mut fresh,
        scene(Color::BLUE, Color::GREEN, Color::BLUE, CachePolicy::None),
    );
    let composite_pixels = support::region_pixels(&composite_changed, backdrop_region);
    let composite_reference_pixels = support::region_pixels(&composite_reference, backdrop_region);
    assert!(
        page_pixels != composite_pixels,
        "changing a retained ancestor composite must change the nested backdrop"
    );
    support::assert_same_bytes(
        "nested backdrop after retained ancestor composite changes",
        32,
        &composite_pixels,
        &composite_reference_pixels,
    );
}

fn capture(renderer: &mut support::LockedRenderer, graph: RenderGraph) -> CapturedFrame {
    renderer.scene_mut().graph = Some(graph);
    renderer
        .capture_frame(WIDTH, HEIGHT)
        .expect("backdrop fixture capture should succeed")
}

fn renderer() -> Option<support::LockedRenderer> {
    match support::headless_renderer() {
        Ok(renderer) => Some(renderer),
        Err(error) => {
            eprintln!("skipping backdrop cache regression because WGPU init failed: {error}");
            None
        }
    }
}

fn warm_cached_backdrop(
    renderer: &mut support::LockedRenderer,
    scene: impl Fn() -> RenderGraph,
) -> CapturedFrame {
    let warm = capture(renderer, scene());
    let replay = capture(renderer, scene());
    let replay_stats = renderer.last_frame_stats().expect("replayed frame stats");
    assert!(
        replay_stats.layer_cache_hits > 0,
        "the stable backdrop should be replayed after warmup: {replay_stats:?}"
    );
    assert_eq!(warm.pixels, replay.pixels);
    warm
}

#[test]
fn rounded_clip_motion_past_the_capture_edge_updates_its_backdrop() {
    let Some(mut renderer) = renderer() else {
        return;
    };
    let warm = warm_cached_backdrop(&mut renderer, || rounded_clip_scene(12.0));
    let moved = capture(&mut renderer, rounded_clip_scene(11.0));
    cranpose_render_wgpu::set_debug_toggle("CRANPOSE_NO_BACKDROP_CACHE", Some("1"));
    let uncached = capture(&mut renderer, rounded_clip_scene(11.0));
    cranpose_render_wgpu::set_debug_toggle("CRANPOSE_NO_BACKDROP_CACHE", None);

    let backdrop = Rect {
        x: 16.0,
        y: 12.0,
        width: 12.0,
        height: 64.0,
    };
    assert_ne!(
        support::region_pixels(&warm, backdrop),
        support::region_pixels(&uncached, backdrop),
        "moving the rounded clip changes pixels inside the backdrop"
    );
    support::assert_same_bytes(
        "rounded-clip moved cached output versus forced uncached output",
        WIDTH,
        &moved.pixels,
        &uncached.pixels,
    );
}

fn assert_unanchored_motion_updates_backdrop(alpha: f32) {
    let Some(mut renderer) = renderer() else {
        return;
    };
    let warm = warm_cached_backdrop(&mut renderer, || subpixel_scene(52.001, alpha));
    let moved = capture(&mut renderer, subpixel_scene(52.030, alpha));

    cranpose_render_wgpu::set_debug_toggle("CRANPOSE_NO_BACKDROP_CACHE", Some("1"));
    let uncached = capture(&mut renderer, subpixel_scene(52.030, alpha));
    cranpose_render_wgpu::set_debug_toggle("CRANPOSE_NO_BACKDROP_CACHE", None);
    assert_ne!(
        warm.pixels, uncached.pixels,
        "the sub-bin motion must change the rendered picture"
    );
    support::assert_same_bytes(
        "sub-bin moved cached output versus forced uncached output",
        WIDTH,
        &moved.pixels,
        &uncached.pixels,
    );
}

#[test]
fn opaque_rectangle_subpixel_motion_updates_its_backdrop() {
    assert_unanchored_motion_updates_backdrop(1.0);
}

#[test]
fn translucent_rectangle_subpixel_motion_updates_its_backdrop() {
    assert_unanchored_motion_updates_backdrop(0.55);
}

#[test]
fn overlay_backdrop_tracks_the_root_page_after_cache_warmup() {
    let Some(mut cached) = renderer() else {
        return;
    };
    cached.set_inspector_overlay(Some(inspector_glass(CachePolicy::Auto)));
    let warm = capture(&mut cached, painted_page(Color::RED));
    let _ = capture(&mut cached, painted_page(Color::RED));
    let updated = capture(&mut cached, painted_page(Color::BLUE));

    cached.set_inspector_overlay(None);
    let plain_blue = capture(&mut cached, painted_page(Color::BLUE));
    let mut fresh = support::LockedRenderer::beside_locked().expect("reference renderer");
    fresh.set_inspector_overlay(Some(inspector_glass(CachePolicy::None)));
    let reference = capture(&mut fresh, painted_page(Color::BLUE));
    let overlay_bounds = Rect {
        x: 16.0,
        y: 12.0,
        width: 56.0,
        height: 48.0,
    };
    assert_ne!(
        support::region_pixels(&reference, overlay_bounds),
        support::region_pixels(&plain_blue, overlay_bounds),
        "the overlay must visibly composite its backdrop over the page"
    );
    assert_ne!(
        support::region_pixels(&warm, overlay_bounds),
        support::region_pixels(&reference, overlay_bounds),
        "the changed root page must alter the overlay's visible backdrop"
    );
    support::assert_same_bytes(
        "overlay backdrop after root page changes",
        WIDTH,
        &updated.pixels,
        &reference.pixels,
    );
}

#[test]
fn nested_backdrop_tracks_immediate_page_and_retained_composite_inputs() {
    verify_nested_backdrop_page_provenance(CachePolicy::Auto);
}

#[test]
fn nested_backdrop_tracks_transient_composite_inputs() {
    verify_nested_backdrop_page_provenance(CachePolicy::None);
}
