mod support;

use cranpose_render_common::{
    Renderer,
    graph::{
        DrawPrimitiveNode, PrimitiveEntry, PrimitiveNode, PrimitivePhase, RenderGraph, RenderNode,
    },
};
use cranpose_render_wgpu::{CancelReason, PresentOutcome};
use cranpose_ui_graphics::{Brush, Color, Rect};

const WIDTH: u32 = 128;
const HEIGHT: u32 = 96;

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
    RenderGraph::new(support::layer_node(
        Some(9_400),
        WIDTH as f32,
        HEIGHT as f32,
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
fn uncaptured_device_error_poisons_one_frame_then_recovers() {
    let mut renderer = match support::headless_renderer() {
        Ok(renderer) => renderer,
        Err(err) => {
            eprintln!("skipping device-error survival: headless WGPU init failed: {err}");
            return;
        }
    };
    renderer.scene_mut().graph = Some(direct_graph());
    let (texture, view) = target_view(&renderer, WIDTH, HEIGHT);

    let packet = renderer
        .build_frame_packet_for_tests(WIDTH, HEIGHT)
        .expect("direct graph must lower into a packet");
    let outcome = renderer
        .render_held_packet_for_tests(&texture, &view, WIDTH, HEIGHT, packet)
        .expect("the pre-error packet must draw");
    assert_eq!(outcome, PresentOutcome::Presented);
    assert_eq!(
        renderer.device_error_count_for_tests(),
        0,
        "a clean frame must record no device errors"
    );

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

    let packet = renderer
        .build_frame_packet_for_tests(WIDTH, HEIGHT)
        .expect("the post-error build must lower normally");
    let outcome = renderer
        .render_held_packet_for_tests(&texture, &view, WIDTH, HEIGHT, packet)
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

    let packet = renderer
        .build_frame_packet_for_tests(WIDTH, HEIGHT)
        .expect("the recovery build must lower normally");
    let outcome = renderer
        .render_held_packet_for_tests(&texture, &view, WIDTH, HEIGHT, packet)
        .expect("the recovery packet must draw");
    assert_eq!(
        outcome,
        PresentOutcome::Presented,
        "one skipped frame per poisoning — the next packet must present"
    );
}
