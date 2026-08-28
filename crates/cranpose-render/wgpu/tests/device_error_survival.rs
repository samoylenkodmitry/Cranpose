//! GPU robustness: an uncaptured wgpu device error must be survived —
//! recorded, logged, and answered with one skipped frame — never a panic.
//! wgpu's default uncaptured handler panics on the reporting thread;
//! mid-encode that unwind runs the drop glue of live pass/encoder
//! objects, whose follow-up error reports re-enter the same panicking
//! handler, and the second panic inside the first's unwind aborts the
//! process with the original validation message unlogged. The framework
//! therefore installs a logging handler at `GpuRenderer` construction
//! (`CRANPOSE_SURVIVE_GPU_ERRORS` kill switch), poisons the renderer,
//! cancels exactly one packet (`CancelReason::DeviceError`, buffers
//! returned whole), and renders on.
//!
//! Red without the fix: the deliberate validation error below reaches the
//! default handler and this suite dies at `create_texture` with
//! `wgpu error: Validation Error`.

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
        Some(9_400),
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

fn target_view(renderer: &support::LockedRenderer, width: u32, height: u32) -> wgpu::TextureView {
    let device = renderer
        .try_device()
        .expect("renderer GPU device was not initialized");
    let texture = device.create_texture(&wgpu::TextureDescriptor {
        label: Some("Device Error Survival Render Target"),
        size: wgpu::Extent3d {
            width,
            height,
            depth_or_array_layers: 1,
        },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: wgpu::TextureFormat::Bgra8UnormSrgb,
        usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
        view_formats: &[],
    });
    texture.create_view(&wgpu::TextureViewDescriptor::default())
}

/// A genuine wgpu validation error — recorded, one packet cancelled with
/// `CancelReason::DeviceError` (its scene returned to the producer pool),
/// and the packet after that presents: one skipped frame per poisoning,
/// no panic anywhere.
#[test]
fn uncaptured_device_error_poisons_one_frame_then_recovers() {
    let mut renderer = match support::headless_renderer() {
        Ok(renderer) => renderer,
        Err(err) => {
            eprintln!("skipping device-error survival: headless WGPU init failed: {err}");
            return;
        }
    };
    renderer.scene_mut().graph = Some(direct_graph());
    let view = target_view(&renderer, WIDTH, HEIGHT);

    // Healthy first: a clean device presents and has recorded nothing.
    let packet = renderer
        .build_frame_packet_for_tests(WIDTH, HEIGHT)
        .expect("direct graph must lower into a packet");
    let outcome = renderer
        .render_held_packet_for_tests(&view, WIDTH, HEIGHT, packet)
        .expect("the pre-error packet must draw");
    assert_eq!(outcome, PresentOutcome::Presented);
    assert_eq!(
        renderer.device_error_count_for_tests(),
        0,
        "a clean frame must record no device errors"
    );

    // A zero-width texture is a wgpu validation error, reported through
    // the device's uncaptured-error path. Without the installed handler
    // this call panics in wgpu's fatal default handler — the red run.
    let device = renderer
        .try_device()
        .expect("renderer GPU device was not initialized");
    let _ = device.create_texture(&wgpu::TextureDescriptor {
        label: Some("Deliberate Validation Error (zero width)"),
        size: wgpu::Extent3d {
            width: 0,
            height: 1,
            depth_or_array_layers: 1,
        },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: wgpu::TextureFormat::Bgra8UnormSrgb,
        usage: wgpu::TextureUsages::TEXTURE_BINDING,
        view_formats: &[],
    });
    assert!(
        renderer.device_error_count_for_tests() >= 1,
        "the uncaptured-error handler must record the validation error"
    );

    // The frame after the error skips whole — cancelled before any
    // encoding, buffers returned — and clears the poison on the way out.
    let packet = renderer
        .build_frame_packet_for_tests(WIDTH, HEIGHT)
        .expect("the post-error build must lower normally");
    let outcome = renderer
        .render_held_packet_for_tests(&view, WIDTH, HEIGHT, packet)
        .expect("a device-error skip is a protocol outcome, not a draw error");
    assert_eq!(
        outcome,
        PresentOutcome::Cancelled(CancelReason::DeviceError),
        "the frame after an uncaptured device error must cancel, not encode"
    );
    assert!(
        renderer.has_retained_direct_scene_for_tests(),
        "the cancelled packet's scene must return to the producer pool"
    );

    // One skip per poisoning: the next packet renders.
    let packet = renderer
        .build_frame_packet_for_tests(WIDTH, HEIGHT)
        .expect("the recovery build must lower normally");
    let outcome = renderer
        .render_held_packet_for_tests(&view, WIDTH, HEIGHT, packet)
        .expect("the recovery packet must draw");
    assert_eq!(
        outcome,
        PresentOutcome::Presented,
        "one skipped frame per poisoning — the next packet must present"
    );
}
