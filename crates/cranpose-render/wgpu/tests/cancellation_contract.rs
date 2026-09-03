mod support;

use cranpose_core::NodeId;
use cranpose_render_common::{
    Renderer,
    graph::{
        CachePolicy, DrawPrimitiveNode, IsolationReasons, LayerNode, PrimitiveEntry, PrimitiveNode,
        PrimitivePhase, ProjectiveTransform, RenderGraph, RenderNode,
    },
    raster_cache::LayerRasterCacheHashes,
};
use cranpose_render_wgpu::{CancelReason, PresentOutcome};
use cranpose_ui_graphics::{Brush, Color, GraphicsLayer, Point, Rect};

const WIDTH: u32 = 128;
const HEIGHT: u32 = 96;

fn test_layer(node_id: Option<NodeId>, children: Vec<RenderNode>) -> LayerNode {
    LayerNode {
        node_id,
        wraps: None,
        local_bounds: Rect {
            x: 0.0,
            y: 0.0,
            width: WIDTH as f32,
            height: HEIGHT as f32,
        },
        transform_to_parent: ProjectiveTransform::identity(),
        motion_context_animated: false,
        translated_content_context: false,
        translated_content_offset: Point::default(),
        content_offset: Point::default(),
        scene_children_origin: Point::default(),
        scene_children_layer_translation: Point::default(),
        graphics_layer: GraphicsLayer::default(),
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

fn rect_primitive(rect: Rect, color: Color) -> RenderNode {
    RenderNode::Primitive(PrimitiveEntry {
        phase: PrimitivePhase::BeforeChildren,
        node: PrimitiveNode::Draw(DrawPrimitiveNode {
            primitive: cranpose_ui_graphics::DrawPrimitive::Rect {
                rect,
                brush: Brush::solid(color),
                stroke: None,
            },
            clip: None,
        }),
    })
}

fn direct_graph() -> RenderGraph {
    RenderGraph::new(test_layer(
        Some(7_100),
        vec![rect_primitive(
            Rect {
                x: 16.0,
                y: 12.0,
                width: 64.0,
                height: 48.0,
            },
            Color(0.2, 0.7, 0.3, 1.0),
        )],
    ))
}

fn target_view(
    renderer: &support::LockedRenderer,
    width: u32,
    height: u32,
) -> (wgpu::Texture, wgpu::TextureView) {
    let device = renderer
        .try_device()
        .expect("renderer GPU device was not initialized");
    support::render_target(
        device,
        width,
        height,
        wgpu::TextureFormat::Bgra8UnormSrgb,
        wgpu::TextureUsages::RENDER_ATTACHMENT,
    )
}

#[test]
fn renderer_replacement_cancels_in_flight_packet() {
    let mut renderer = match support::headless_renderer() {
        Ok(renderer) => renderer,
        Err(err) => {
            eprintln!("skipping renderer-epoch cancel: headless WGPU init failed: {err}");
            return;
        }
    };
    renderer.scene_mut().graph = Some(direct_graph());

    let packet = renderer
        .build_frame_packet_for_tests(WIDTH, HEIGHT)
        .expect("direct graph must lower into a packet");
    support::reinit_gpu(&mut renderer).expect("GPU reinit failed");

    let (texture, view) = target_view(&renderer, WIDTH, HEIGHT);
    let outcome = renderer
        .render_held_packet_for_tests(&texture, &view, WIDTH, HEIGHT, packet)
        .expect("a cancel is a protocol outcome, not a draw error");
    assert_eq!(
        outcome,
        PresentOutcome::Cancelled(CancelReason::RendererEpoch),
        "a packet built against the dead renderer must cancel, not draw"
    );

    let packet = renderer
        .build_frame_packet_for_tests(WIDTH, HEIGHT)
        .expect("the next build must lower normally");
    let outcome = renderer
        .render_held_packet_for_tests(&texture, &view, WIDTH, HEIGHT, packet)
        .expect("the post-replacement packet must draw");
    assert_eq!(
        outcome,
        PresentOutcome::Presented,
        "a packet built against the NEW renderer must present"
    );
}

#[test]
fn surface_reconfigure_cancels_waiting_packet() {
    let mut renderer = match support::headless_renderer() {
        Ok(renderer) => renderer,
        Err(err) => {
            eprintln!("skipping surface-epoch cancel: headless WGPU init failed: {err}");
            return;
        }
    };
    renderer.scene_mut().graph = Some(direct_graph());

    let packet = renderer
        .build_frame_packet_for_tests(WIDTH, HEIGHT)
        .expect("direct graph must lower into a packet");
    renderer.note_surface_reconfigured();

    let (texture, view) = target_view(&renderer, WIDTH, HEIGHT);
    let outcome = renderer
        .render_held_packet_for_tests(&texture, &view, WIDTH, HEIGHT, packet)
        .expect("a cancel is a protocol outcome, not a draw error");
    assert_eq!(
        outcome,
        PresentOutcome::Cancelled(CancelReason::SurfaceEpoch),
        "a packet straddling a surface reconfigure must cancel, not draw"
    );

    let packet = renderer
        .build_frame_packet_for_tests(WIDTH, HEIGHT)
        .expect("the next build must lower normally");
    let outcome = renderer
        .render_held_packet_for_tests(&texture, &view, WIDTH, HEIGHT, packet)
        .expect("the post-reconfigure packet must draw");
    assert_eq!(outcome, PresentOutcome::Presented);
}

#[test]
fn viewport_mismatch_cancels_packet() {
    let mut renderer = match support::headless_renderer() {
        Ok(renderer) => renderer,
        Err(err) => {
            eprintln!("skipping viewport cancel: headless WGPU init failed: {err}");
            return;
        }
    };
    renderer.scene_mut().graph = Some(direct_graph());

    let packet = renderer
        .build_frame_packet_for_tests(WIDTH, HEIGHT)
        .expect("direct graph must lower into a packet");
    let (texture, view) = target_view(&renderer, WIDTH / 2, HEIGHT / 2);
    let outcome = renderer
        .render_held_packet_for_tests(&texture, &view, WIDTH / 2, HEIGHT / 2, packet)
        .expect("a cancel is a protocol outcome, not a draw error");
    assert_eq!(
        outcome,
        PresentOutcome::Cancelled(CancelReason::Viewport),
        "a packet lowered for another viewport must cancel, not draw"
    );
}
