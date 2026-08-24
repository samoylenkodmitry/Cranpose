//! Pipeline step 7a: packet validity and cancellation-by-protocol. A
//! packet built against a dead renderer instance, a stale surface
//! configuration, or another viewport must be refused at the head of the
//! present stage — before any encoding — with every buffer it carries
//! returned to the producer: the direct scene to the recycling pool and
//! the unconsumed replay plan to the planner (its releases re-queued so
//! pool slot ids never leak, its unconfirmable awaiting entries purged,
//! its buffers recycled). A cancel is an outcome, not an error, and a
//! packet built after the change presents normally.

mod support;

use std::path::Path;

use cranpose_core::NodeId;
use cranpose_render_common::{
    graph::{
        CachePolicy, DrawCommandId, DrawPrimitiveNode, IsolationReasons, LayerNode, PrimitiveEntry,
        PrimitiveNode, PrimitivePhase, ProjectiveTransform, RenderGraph, RenderNode,
    },
    raster_cache::LayerRasterCacheHashes,
    style_shared::DrawPlacement,
    Renderer,
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

/// A direct-eligible root (no shadow, no effects): its packets carry a
/// `PacketRoot::Direct` scene, which is what the cancel path must return.
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

fn command_for(node_id: usize) -> DrawCommandId {
    DrawCommandId {
        node_id,
        command_index: 0,
        placement: DrawPlacement::Behind,
    }
}

/// A render target on the renderer's CURRENT device (created fresh after
/// any reinit, since the old device's views die with it).
fn target_view(renderer: &support::LockedRenderer, width: u32, height: u32) -> wgpu::TextureView {
    let device = renderer
        .try_device()
        .expect("renderer GPU device was not initialized");
    let texture = device.create_texture(&wgpu::TextureDescriptor {
        label: Some("Cancellation Contract Render Target"),
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

/// 7a-1: renderer replacement with a packet in flight. The stale packet is
/// cancelled for its renderer epoch — never drawn against the new store —
/// its scene returns to the producer pool, the planner carries no leaked
/// state, and the next-built packet presents normally.
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
    cranpose_render_wgpu::inject_feed_capture_for_tests(command_for(7_201), 0, 0, 1);

    let packet = renderer
        .build_frame_packet_for_tests(WIDTH, HEIGHT)
        .expect("direct graph must lower into a packet");
    support::reinit_gpu(&mut renderer).expect("GPU reinit failed");

    let view = target_view(&renderer, WIDTH, HEIGHT);
    let outcome = renderer
        .render_held_packet_for_tests(&view, WIDTH, HEIGHT, packet)
        .expect("a cancel is a protocol outcome, not a draw error");
    assert_eq!(
        outcome,
        PresentOutcome::Cancelled(CancelReason::RendererEpoch),
        "a packet built against the dead renderer must cancel, not draw"
    );
    assert!(
        renderer.has_retained_direct_scene_for_tests(),
        "the cancelled packet's scene must return to the producer pool"
    );
    let (pending_releases, awaiting) = cranpose_render_wgpu::planner_replay_queue_stats_for_tests();
    assert_eq!(
        (pending_releases, awaiting),
        (0, 0),
        "nothing may leak into the planner queues across the replacement \
         (old slot ids never cross renderers; the cancelled capture purges)"
    );
    assert_eq!(
        cranpose_render_wgpu::pending_feed_capture_count_for_tests(),
        0,
        "the cancelled batch is consumed whole, never requeued as captures"
    );
    let (feed_slots, _, _) = cranpose_render_wgpu::command_feed_live_stats();
    assert_eq!(feed_slots, 0, "no cancelled capture may become a feed slot");

    let packet = renderer
        .build_frame_packet_for_tests(WIDTH, HEIGHT)
        .expect("the next build must lower normally");
    let outcome = renderer
        .render_held_packet_for_tests(&view, WIDTH, HEIGHT, packet)
        .expect("the post-replacement packet must draw");
    assert_eq!(
        outcome,
        PresentOutcome::Presented,
        "a packet built against the NEW renderer must present"
    );
}

/// 7a-2: surface reconfigure with a packet waiting. The packet cancels for
/// its surface epoch, its buffers return, and a packet built after the
/// reconfigure presents.
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

    let view = target_view(&renderer, WIDTH, HEIGHT);
    let outcome = renderer
        .render_held_packet_for_tests(&view, WIDTH, HEIGHT, packet)
        .expect("a cancel is a protocol outcome, not a draw error");
    assert_eq!(
        outcome,
        PresentOutcome::Cancelled(CancelReason::SurfaceEpoch),
        "a packet straddling a surface reconfigure must cancel, not draw"
    );
    assert!(
        renderer.has_retained_direct_scene_for_tests(),
        "the cancelled packet's scene must return to the producer pool"
    );

    let packet = renderer
        .build_frame_packet_for_tests(WIDTH, HEIGHT)
        .expect("the next build must lower normally");
    let outcome = renderer
        .render_held_packet_for_tests(&view, WIDTH, HEIGHT, packet)
        .expect("the post-reconfigure packet must draw");
    assert_eq!(outcome, PresentOutcome::Presented);
}

/// 7a-3: viewport mismatch. A packet lowered for one size, presented at
/// another, cancels — the payload's coordinates are wrong for the target.
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
    let view = target_view(&renderer, WIDTH / 2, HEIGHT / 2);
    let outcome = renderer
        .render_held_packet_for_tests(&view, WIDTH / 2, HEIGHT / 2, packet)
        .expect("a cancel is a protocol outcome, not a draw error");
    assert_eq!(
        outcome,
        PresentOutcome::Cancelled(CancelReason::Viewport),
        "a packet lowered for another viewport must cancel, not draw"
    );
    assert!(renderer.has_retained_direct_scene_for_tests());
}

/// 7a-4: a cancelled packet returns its scene AND its replay buffers. The
/// scene repopulates the producer pool (and feeds the next build), the
/// batch's awaiting-confirmation entries purge (they can never confirm —
/// every later frame could be a Surface frame with no ack), and the op
/// buffers recycle with capacity intact.
#[test]
fn cancelled_packet_returns_scene_and_replay_buffers() {
    let mut renderer = match support::headless_renderer() {
        Ok(renderer) => renderer,
        Err(err) => {
            eprintln!("skipping cancel buffer return: headless WGPU init failed: {err}");
            return;
        }
    };
    renderer.scene_mut().graph = Some(direct_graph());
    cranpose_render_wgpu::inject_feed_capture_for_tests(command_for(7_401), 0, 0, 1);

    let packet = renderer
        .build_frame_packet_for_tests(WIDTH, HEIGHT)
        .expect("direct graph must lower into a packet");
    let (_, awaiting) = cranpose_render_wgpu::planner_replay_queue_stats_for_tests();
    assert_eq!(
        awaiting, 1,
        "the plan must have recorded the capture as awaiting confirmation"
    );

    renderer.note_surface_reconfigured();
    let view = target_view(&renderer, WIDTH, HEIGHT);
    let outcome = renderer
        .render_held_packet_for_tests(&view, WIDTH, HEIGHT, packet)
        .expect("a cancel is a protocol outcome, not a draw error");
    assert_eq!(
        outcome,
        PresentOutcome::Cancelled(CancelReason::SurfaceEpoch)
    );

    assert!(
        renderer.has_retained_direct_scene_for_tests(),
        "retained_direct_scene must be repopulated by the cancel"
    );
    let (_, awaiting) = cranpose_render_wgpu::planner_replay_queue_stats_for_tests();
    assert_eq!(
        awaiting, 0,
        "the cancelled frame's awaiting entries must purge — no ack can \
         ever confirm them"
    );
    let (captures_cap, _, _) = cranpose_render_wgpu::recycled_ops_capacities_for_tests();
    assert!(
        captures_cap >= 1,
        "the cancelled batch's capture buffer must recycle with its \
         capacity intact"
    );

    // The recycled scene actually feeds the next build (taken from the
    // pool), closing the loop.
    let packet = renderer
        .build_frame_packet_for_tests(WIDTH, HEIGHT)
        .expect("the next build must lower normally");
    assert!(
        !renderer.has_retained_direct_scene_for_tests(),
        "the next build must take the recycled scene from the pool"
    );
    let outcome = renderer
        .render_held_packet_for_tests(&view, WIDTH, HEIGHT, packet)
        .expect("the next packet must draw");
    assert_eq!(outcome, PresentOutcome::Presented);
}

/// 7a-5: the slot-hold invariant, pinned structurally (render_contract.rs
/// pattern): a frame's retained slots cannot be released before the frame
/// completes, because the store frees slots in exactly ONE place — the
/// release drain at the head of `consume_replay_ops`, which only ever runs
/// on a frame's OWN ops batch before that frame encodes. Planner-side, a
/// displaced slot (5b) and a cancelled batch's releases both re-queue into
/// `pending_releases`, which drains ONLY into a later frame's ops.
#[test]
fn slot_releases_drain_only_inside_a_later_frames_consume() {
    let crate_dir = Path::new(env!("CARGO_MANIFEST_DIR"));
    let render_source =
        std::fs::read_to_string(crate_dir.join("src/render.rs")).expect("failed to read render.rs");
    assert_eq!(
        render_source.matches("self.release_replay_slot(").count(),
        1,
        "the store must free replay slots in exactly one place"
    );
    let consume_start = render_source
        .find("fn consume_replay_ops(")
        .expect("consume_replay_ops must exist");
    let consume_end = render_source[consume_start..]
        .find("fn replay_generation_drops(")
        .map(|offset| consume_start + offset)
        .expect("replay_generation_drops must follow consume_replay_ops");
    let consume_body = &render_source[consume_start..consume_end];
    assert!(
        consume_body.contains("for slot in ops.releases.drain(..)")
            && consume_body.contains("self.release_replay_slot(slot);"),
        "the only slot-free site must be the release drain of the frame's own ops"
    );

    let replay_source = std::fs::read_to_string(crate_dir.join("src/shape_replay.rs"))
        .expect("failed to read shape_replay.rs");
    assert!(
        replay_source.contains("self.pending_releases.push(old.gpu_slot);"),
        "a displaced slot must queue for the NEXT frame's ops, never free same-frame"
    );
    assert!(
        replay_source.contains("std::mem::swap(&mut ops.releases, &mut self.pending_releases);"),
        "the release queue must drain only into a frame's ops batch"
    );
    assert!(
        replay_source.contains("self.pending_releases.extend_from_slice(&ops.releases);"),
        "a cancelled batch's releases must re-queue through the same queue"
    );
}

/// 7a §6: the render-error scene return, pinned structurally: when the
/// direct draw errs, the packet's `CompositorScene` must still travel back
/// through `returns.scene` — `render_root_direct` yields the scene in BOTH
/// arms and the present backend stashes it before propagating the error.
#[test]
fn render_error_path_returns_the_direct_scene() {
    let crate_dir = Path::new(env!("CARGO_MANIFEST_DIR"));
    let paths_source =
        std::fs::read_to_string(crate_dir.join("src/surface_executor/render_paths.rs"))
            .expect("failed to read render_paths.rs");
    assert!(
        paths_source.contains("Err(error) => Err((error, local_scene)),"),
        "render_root_direct must hand the scene back on the error arm too"
    );
    let render_source =
        std::fs::read_to_string(crate_dir.join("src/render.rs")).expect("failed to read render.rs");
    let direct_start = render_source
        .find("let result = match execute_render_root_direct(")
        .expect("the direct arm must match on the scene-carrying result");
    let direct_end = render_source[direct_start..]
        .find("if result.is_ok()")
        .map(|offset| direct_start + offset)
        .expect("the overlay gate must follow the direct render");
    let direct_body = &render_source[direct_start..direct_end];
    assert_eq!(
        direct_body.matches("returns.scene = Some(scene);").count(),
        2,
        "the present backend must return the scene on BOTH the ok and error arms"
    );
}
