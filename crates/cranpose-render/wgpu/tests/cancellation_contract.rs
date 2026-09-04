mod support;

use cranpose_core::NodeId;
use cranpose_render_common::{
    Renderer,
    graph::{
        CachePolicy, DrawCommandId, DrawRunNode, LayerNode, PrimitivePhase, ProjectiveTransform,
        RenderGraph, RenderNode,
    },
    style_shared::DrawPlacement,
};
use cranpose_render_wgpu::{CancelReason, PresentOutcome};
use cranpose_ui_graphics::{Brush, Color, DrawPrimitive, Rect};

const WIDTH: u32 = 128;
const HEIGHT: u32 = 96;

fn test_layer(node_id: Option<NodeId>, children: Vec<RenderNode>) -> LayerNode {
    support::contract_layer(
        node_id,
        CachePolicy::None,
        Rect {
            x: 0.0,
            y: 0.0,
            width: WIDTH as f32,
            height: HEIGHT as f32,
        },
        ProjectiveTransform::identity(),
        children,
    )
}

fn direct_graph() -> RenderGraph {
    RenderGraph::new(test_layer(
        Some(7_100),
        vec![support::rect_primitive(
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

/// A graph whose one command records `count` rects of `color`: enough
/// records for the run store to retain the command's tables.
fn stored_run_graph(count: usize, color: Color) -> RenderGraph {
    let primitives = (0..count)
        .map(|index| DrawPrimitive::Rect {
            rect: Rect {
                x: (index % 8) as f32 * 14.0 + 4.0,
                y: (index / 8) as f32 * 9.0 + 4.0,
                width: 10.0,
                height: 6.0,
            },
            brush: Brush::solid(color),
            stroke: None,
        })
        .collect();
    RenderGraph::new(test_layer(
        Some(7_200),
        vec![RenderNode::DrawRun(DrawRunNode::for_command(
            PrimitivePhase::BeforeChildren,
            Some(DrawCommandId {
                node_id: 7_200,
                command_index: 0,
                placement: DrawPlacement::Behind,
            }),
            primitives,
        ))],
    ))
}

fn present_and_read(renderer: &mut support::LockedRenderer, graph: RenderGraph) -> Vec<u8> {
    renderer.scene_mut().graph = Some(graph);
    let packet = renderer
        .build_frame_packet_for_tests(WIDTH, HEIGHT)
        .expect("the graph must lower into a packet");
    let (texture, view) = support::render_target(
        renderer.try_device().expect("device"),
        WIDTH,
        HEIGHT,
        wgpu::TextureFormat::Bgra8UnormSrgb,
        wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
    );
    let outcome = renderer
        .render_held_packet_for_tests(&texture, &view, WIDTH, HEIGHT, packet)
        .expect("the packet must draw");
    assert_eq!(outcome, PresentOutcome::Presented);
    let device = renderer.try_device().expect("device");
    let queue = renderer.try_queue_for_tests().expect("queue");
    support::read_texture_rgba8(device, queue, &texture)
}

/// The run store learns a command's tables only from a packet it draws: a
/// cancelled packet carrying new tables for a retained command leaves the
/// store at its last upload, and the next presented packet draws the new
/// tables, not what the store held.
#[test]
fn a_cancelled_packet_leaves_the_run_store_at_its_last_upload() {
    let mut renderer = match support::headless_renderer() {
        Ok(renderer) => renderer,
        Err(err) => {
            eprintln!("skipping run store cancel: headless WGPU init failed: {err}");
            return;
        }
    };
    let green = Color(0.2, 0.7, 0.3, 1.0);
    let red = Color(0.8, 0.2, 0.2, 1.0);
    let first = present_and_read(&mut renderer, stored_run_graph(96, green));

    renderer.scene_mut().graph = Some(stored_run_graph(96, red));
    let packet = renderer
        .build_frame_packet_for_tests(WIDTH, HEIGHT)
        .expect("the red graph must lower into a packet");
    let (texture, view) = target_view(&renderer, WIDTH / 2, HEIGHT / 2);
    let outcome = renderer
        .render_held_packet_for_tests(&texture, &view, WIDTH / 2, HEIGHT / 2, packet)
        .expect("a cancel is a protocol outcome, not a draw error");
    assert_eq!(outcome, PresentOutcome::Cancelled(CancelReason::Viewport));

    let after_cancel = present_and_read(&mut renderer, stored_run_graph(96, red));
    let reference = present_and_read(&mut renderer, stored_run_graph(96, red));
    assert_ne!(
        first, after_cancel,
        "the red command must draw red, not the green tables the store retained"
    );
    support::assert_same_bytes("after a cancelled packet", WIDTH, &after_cancel, &reference);
}
