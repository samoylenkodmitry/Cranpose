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
