//! Pipeline step 7b: the present runtime's depth-one protocol. Credit
//! gates packet building (backpressure BEFORE lowering), an invalidating
//! control message cancels the waiting packet and sends its returns
//! BEFORE it is acknowledged, a dead surface refuses packets with their
//! buffers intact, the recycled ack-confirmations buffer rides the next
//! packet back to the store, the store's replay ack leaves on its own
//! channel ahead of the frame's returns, and the producer's warmup read
//! is the present thread's atomic snapshot — never the renderer.
//!
//! Most tests drive the state machine INLINE (no thread) through
//! `init_gpu_inline_for_tests`, pumping the message queue by hand so
//! every ordering is deterministic; one smoke test runs the real spawned
//! thread end to end.

mod support;

use std::sync::{Arc, MutexGuard};
use std::time::{Duration, Instant};

use cranpose_core::NodeId;
use cranpose_render_common::graph::{
    CachePolicy, DrawCommandId, DrawPrimitiveNode, IsolationReasons, LayerNode, PrimitiveEntry,
    PrimitiveNode, PrimitivePhase, ProjectiveTransform, RenderGraph, RenderNode,
};
use cranpose_render_common::raster_cache::LayerRasterCacheHashes;
use cranpose_render_common::style_shared::DrawPlacement;
use cranpose_render_common::Renderer;
use cranpose_render_wgpu::{CancelReason, PresentOutcome, PublishOutcome, WgpuRenderer};
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

/// A direct-eligible root: its packets carry a `PacketRoot::Direct` scene,
/// which is what the cancel/recycle assertions observe.
fn direct_graph() -> RenderGraph {
    RenderGraph::new(test_layer(
        Some(7_700),
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

/// A root whose CHILD carries a shadow: the first render must miss the
/// shadow shape cache, which is one of the warmup triggers.
fn shadowed_child_graph() -> RenderGraph {
    let mut child = test_layer(Some(7_701), vec![]);
    child.local_bounds = Rect {
        x: 24.0,
        y: 20.0,
        width: 48.0,
        height: 32.0,
    };
    child.graphics_layer.shadow_elevation = 6.0;
    RenderGraph::new(test_layer(
        Some(7_702),
        vec![RenderNode::Layer(Box::new(child))],
    ))
}

fn command_for(node_id: usize) -> DrawCommandId {
    DrawCommandId {
        node_id,
        command_index: 0,
        placement: DrawPlacement::Behind,
    }
}

fn surface_config(width: u32, height: u32) -> wgpu::SurfaceConfiguration {
    wgpu::SurfaceConfiguration {
        usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
        format: wgpu::TextureFormat::Bgra8UnormSrgb,
        width,
        height,
        present_mode: wgpu::PresentMode::Fifo,
        alpha_mode: wgpu::CompositeAlphaMode::Auto,
        view_formats: vec![],
        desired_maximum_frame_latency: 2,
    }
}

/// An UNINITIALIZED renderer plus the device/queue the test hands to the
/// runtime itself (unlike `support::headless_renderer`, which claims them
/// for a sync `init_gpu`).
#[allow(clippy::type_complexity)]
fn threaded_parts() -> Result<
    (
        MutexGuard<'static, ()>,
        WgpuRenderer,
        Arc<wgpu::Device>,
        Arc<wgpu::Queue>,
        wgpu::Backend,
        wgpu::DownlevelFlags,
    ),
    String,
> {
    let lock = support::gpu_test_lock();
    let mut instance_descriptor = wgpu::InstanceDescriptor::new_without_display_handle();
    instance_descriptor.backends = wgpu::Backends::all();
    let instance = wgpu::Instance::new(instance_descriptor);
    let adapter = pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions {
        power_preference: wgpu::PowerPreference::LowPower,
        compatible_surface: None,
        force_fallback_adapter: false,
    }))
    .map_err(|err| format!("adapter request failed: {err:?}"))?;
    let (device, queue) = pollster::block_on(adapter.request_device(&wgpu::DeviceDescriptor {
        label: Some("Present Runtime Contract Test Device"),
        required_features: wgpu::Features::empty(),
        required_limits: wgpu::Limits::default(),
        experimental_features: wgpu::ExperimentalFeatures::disabled(),
        memory_hints: wgpu::MemoryHints::default(),
        trace: wgpu::Trace::Off,
    }))
    .map_err(|err| format!("device request failed: {err:?}"))?;
    Ok((
        lock,
        WgpuRenderer::new(&[support::TEST_FONT]),
        Arc::new(device),
        Arc::new(queue),
        adapter.get_info().backend,
        adapter.get_downlevel_capabilities().flags,
    ))
}

/// Inline runtime with the offscreen surrogate target attached, so the
/// full validate → render path runs headlessly.
macro_rules! inline_runtime_or_skip {
    ($name:literal) => {{
        match threaded_parts() {
            Ok(parts) => parts,
            Err(err) => {
                eprintln!("skipping {}: headless WGPU init failed: {err}", $name);
                return;
            }
        }
    }};
}

fn drain_outcomes(renderer: &mut WgpuRenderer) -> Vec<(u64, PresentOutcome)> {
    let mut outcomes = Vec::new();
    renderer.drain_present_returns_with(&mut |frame_id, outcome, _| {
        outcomes.push((frame_id, outcome));
    });
    outcomes
}

/// 7b-1: depth-one credit — one packet rendering AND one waiting. Two
/// publishes fit (that pair is what lets the producer lower N+1 while the
/// present thread draws N); the THIRD reports `NoCredit` WITHOUT lowering
/// a packet, and credit returns as frames drain.
#[test]
fn depth_one_credit_gates_publish_before_lowering() {
    let (_lock, mut renderer, device, queue, backend, downlevel) =
        inline_runtime_or_skip!("depth-one credit");
    let mut runtime = renderer.init_gpu_inline_for_tests(
        device,
        queue,
        wgpu::TextureFormat::Bgra8UnormSrgb,
        backend,
        downlevel,
    );
    let ack = renderer
        .send_attach_offscreen_unacked_for_tests(WIDTH, HEIGHT)
        .expect("inline runtime must accept controls");
    runtime.pump();
    ack.try_recv().expect("attach must ack after the pump");

    renderer.scene_mut().graph = Some(direct_graph());
    assert!(renderer.has_frame_credit());
    assert_eq!(
        renderer.publish_frame(WIDTH, HEIGHT),
        PublishOutcome::Published
    );
    assert_eq!(renderer.last_published_frame_id(), 1);
    assert!(
        renderer.has_frame_credit(),
        "one rendering plus one waiting: the second credit is what makes \
         the producer and present stages overlap"
    );
    assert_eq!(
        renderer.publish_frame(WIDTH, HEIGHT),
        PublishOutcome::Published
    );
    assert_eq!(renderer.last_published_frame_id(), 2);
    assert!(
        !renderer.has_frame_credit(),
        "two packets in flight is the bound; the producer stalls here so \
         it can never run away from the screen"
    );
    assert_eq!(
        renderer.publish_frame(WIDTH, HEIGHT),
        PublishOutcome::NoCredit,
        "both slots are occupied"
    );
    assert_eq!(
        renderer.last_published_frame_id(),
        2,
        "a NoCredit publish must not build a packet: backpressure lands \
         before the lowering work"
    );

    runtime.pump();
    runtime.pump();
    assert_eq!(
        drain_outcomes(&mut renderer),
        vec![
            (1, PresentOutcome::Presented),
            (2, PresentOutcome::Presented)
        ],
        "the offscreen surrogate must render both packets, in publish order"
    );
    assert!(
        renderer.has_frame_credit(),
        "drained returns free the publish credit"
    );
    assert_eq!(
        renderer.publish_frame(WIDTH, HEIGHT),
        PublishOutcome::Published
    );
    assert_eq!(renderer.last_published_frame_id(), 3);
}

/// 7b-2: invalidation before republish. With a packet waiting in the
/// slot, a `Reconfigure` cancels it — its returns are sent BEFORE the ack
/// fires — and a packet published under the new epoch renders.
#[test]
fn reconfigure_cancels_waiting_packet_before_ack() {
    let (_lock, mut renderer, device, queue, backend, downlevel) =
        inline_runtime_or_skip!("invalidation-before-ack");
    let mut runtime = renderer.init_gpu_inline_for_tests(
        device,
        queue,
        wgpu::TextureFormat::Bgra8UnormSrgb,
        backend,
        downlevel,
    );
    let ack = renderer
        .send_attach_offscreen_unacked_for_tests(WIDTH, HEIGHT)
        .expect("inline runtime must accept controls");
    runtime.pump();
    ack.try_recv().expect("attach must ack after the pump");
    renderer.scene_mut().graph = Some(direct_graph());

    assert_eq!(
        renderer.publish_frame(WIDTH, HEIGHT),
        PublishOutcome::Published
    );
    // Producer-side resize while frame 1 waits: epoch bump first, then
    // the control message stamped with the new epoch.
    renderer.note_surface_reconfigured();
    let ack = renderer
        .send_reconfigure_unacked_for_tests(surface_config(WIDTH * 2, HEIGHT * 2))
        .expect("inline runtime must accept controls");
    assert!(
        ack.try_recv().is_err(),
        "no ack may fire before the runtime processed the invalidation"
    );

    // One pump: the runtime stashes the waiting packet, processes the
    // queued Reconfigure against it (cancel + returns + ack, in that
    // order), and has nothing left to consume.
    runtime.pump();
    assert_eq!(
        drain_outcomes(&mut renderer),
        vec![(1, PresentOutcome::Cancelled(CancelReason::SurfaceEpoch))],
        "the waiting packet must cancel for its stale surface epoch"
    );
    ack.try_recv()
        .expect("the ack must have fired — after the cancelled returns were sent");
    assert!(
        renderer.has_retained_direct_scene_for_tests(),
        "the cancelled packet's scene must return to the producer pool"
    );

    // The producer republishes under the new epoch at the new size and
    // the frame renders.
    assert_eq!(
        renderer.publish_frame(WIDTH * 2, HEIGHT * 2),
        PublishOutcome::Published
    );
    runtime.pump();
    assert_eq!(
        drain_outcomes(&mut renderer),
        vec![(2, PresentOutcome::Presented)],
        "a packet published under the new epoch must render"
    );
}

/// 7b-3: `DropSurface` with a packet waiting. The packet cannot render at
/// all: it cancels — pinned: `Cancelled(SurfaceUnavailable)`, the runtime
/// cancels it directly without touching the GPU — with its scene AND its
/// replay plan returned (planner re-queue proof, like 7a's). A packet
/// published after the drop cancels too (`SurfaceEpoch`: the producer's
/// bump outran the runtime's copy, which `DropSurface` — carrying no
/// epoch — never updates).
#[test]
fn drop_surface_cancels_waiting_packet_with_buffers_returned() {
    let (_lock, mut renderer, device, queue, backend, downlevel) =
        inline_runtime_or_skip!("drop-surface cancel");
    let mut runtime = renderer.init_gpu_inline_for_tests(
        device,
        queue,
        wgpu::TextureFormat::Bgra8UnormSrgb,
        backend,
        downlevel,
    );
    let ack = renderer
        .send_attach_offscreen_unacked_for_tests(WIDTH, HEIGHT)
        .expect("inline runtime must accept controls");
    runtime.pump();
    ack.try_recv().expect("attach must ack after the pump");
    renderer.scene_mut().graph = Some(direct_graph());
    cranpose_render_wgpu::inject_feed_capture_for_tests(command_for(7_731), 0, 0, 1);

    assert_eq!(
        renderer.publish_frame(WIDTH, HEIGHT),
        PublishOutcome::Published
    );
    let (_, awaiting) = cranpose_render_wgpu::planner_replay_queue_stats_for_tests();
    assert_eq!(awaiting, 1, "the packet's plan must carry the capture");

    // TerminateWindow flow: epoch bump, then the surface dies.
    renderer.note_surface_reconfigured();
    let ack = renderer
        .send_drop_surface_unacked_for_tests()
        .expect("inline runtime must accept controls");
    runtime.pump();
    assert_eq!(
        drain_outcomes(&mut renderer),
        vec![(
            1,
            PresentOutcome::Cancelled(CancelReason::SurfaceUnavailable)
        )],
        "a packet waiting when the surface died cancels as SurfaceUnavailable"
    );
    ack.try_recv().expect("drop must ack after the cancel");
    assert!(
        renderer.has_retained_direct_scene_for_tests(),
        "the cancelled packet's scene must return to the producer pool"
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
        "the cancelled batch's capture buffer must recycle with capacity intact"
    );

    // Publishing against the dead surface refuses the packet whole too.
    assert_eq!(
        renderer.publish_frame(WIDTH, HEIGHT),
        PublishOutcome::Published
    );
    runtime.pump();
    assert_eq!(
        drain_outcomes(&mut renderer),
        vec![(2, PresentOutcome::Cancelled(CancelReason::SurfaceEpoch))],
        "after the drop the producer's epoch is ahead of the runtime's \
         (DropSurface carries none), so the next packet cancels for it"
    );
    assert!(renderer.has_retained_direct_scene_for_tests());
}

/// 7b-4: the confirmations capacity round-trip. In threaded mode the
/// planner-drained ack confirmations vec cannot return to the store
/// synchronously; it rides the NEXT packet and the store adopts it as its
/// ack backing buffer — capacity preserved, no per-frame allocation.
#[test]
fn confirmations_capacity_rides_next_packet_back_to_store() {
    let (_lock, mut renderer, device, queue, backend, downlevel) =
        inline_runtime_or_skip!("confirmations round-trip");
    let mut runtime = renderer.init_gpu_inline_for_tests(
        device,
        queue,
        wgpu::TextureFormat::Bgra8UnormSrgb,
        backend,
        downlevel,
    );
    let ack = renderer
        .send_attach_offscreen_unacked_for_tests(WIDTH, HEIGHT)
        .expect("inline runtime must accept controls");
    runtime.pump();
    ack.try_recv().expect("attach must ack after the pump");
    renderer.scene_mut().graph = Some(direct_graph());

    // A previous frame's drained ack buffer, parked with real capacity
    // (seeded: producing a genuinely confirmed capture requires the
    // multi-frame verification heuristics, which are not this contract).
    const SEEDED_CAPACITY: usize = 7;
    renderer.seed_recycled_confirmations_for_tests(SEEDED_CAPACITY);

    // The publish carries the parked buffer into the packet...
    assert_eq!(
        renderer.publish_frame(WIDTH, HEIGHT),
        PublishOutcome::Published
    );
    assert_eq!(
        renderer.pending_recycled_confirmations_capacity_for_tests(),
        None,
        "the packet must take the parked buffer with it"
    );
    // ...the store adopts it, and the SAME frame's ack immediately takes
    // it back out as its confirmations buffer (the store holds the
    // capacity only between adoption and consumption — afterwards its
    // backing slot is the taken-out empty)...
    runtime.pump();
    assert_eq!(
        runtime.store_ack_confirmations_capacity(),
        0,
        "the frame's ack must have taken the adopted buffer out of the store"
    );
    // ...and the drain parks it producer-side again. Capacity arriving
    // back here is THE proof of the full revolution: without the store
    // adoption the ack would have carried a fresh zero-capacity vec.
    assert_eq!(renderer.drain_present_returns(), 1);
    assert_eq!(
        renderer.pending_recycled_confirmations_capacity_for_tests(),
        Some(SEEDED_CAPACITY),
        "the drained ack must park the buffer with its capacity preserved \
         through store adoption and the planner drain — no per-frame alloc"
    );

    // Second revolution: the cycle is self-sustaining.
    assert_eq!(
        renderer.publish_frame(WIDTH, HEIGHT),
        PublishOutcome::Published
    );
    assert_eq!(
        renderer.pending_recycled_confirmations_capacity_for_tests(),
        None
    );
    runtime.pump();
    assert_eq!(renderer.drain_present_returns(), 1);
    assert_eq!(
        renderer.pending_recycled_confirmations_capacity_for_tests(),
        Some(SEEDED_CAPACITY)
    );
}

/// 7b-7: the early replay ack. The store answers a Direct packet's replay
/// plan on the ack channel BEFORE the frame draws (pre-acquire in the
/// threaded runtime) — the confirmation-latency fix that makes depth-one
/// overlap pay: the producer folds the ack in ahead of its next planning,
/// the same one-frame latency the synchronous path has. Pinned here as
/// protocol shape: the ack is drainable while the frame's returns are
/// still queued, arrives exactly once, and the returns no longer carry
/// an ack to double-apply.
#[test]
fn early_replay_ack_precedes_returns() {
    let (_lock, mut renderer, device, queue, backend, downlevel) =
        inline_runtime_or_skip!("early replay ack");
    let mut runtime = renderer.init_gpu_inline_for_tests(
        device,
        queue,
        wgpu::TextureFormat::Bgra8UnormSrgb,
        backend,
        downlevel,
    );
    let ack = renderer
        .send_attach_offscreen_unacked_for_tests(WIDTH, HEIGHT)
        .expect("inline runtime must accept controls");
    runtime.pump();
    ack.try_recv().expect("attach must ack after the pump");
    renderer.scene_mut().graph = Some(direct_graph());

    // Seeded capacity is the tracer: it can only come back to the
    // producer inside the frame's ReplayAck (store adoption → ack buffer
    // → planner drain), so its arrival IS the ack's arrival.
    const SEEDED_CAPACITY: usize = 5;
    renderer.seed_recycled_confirmations_for_tests(SEEDED_CAPACITY);
    assert_eq!(
        renderer.publish_frame(WIDTH, HEIGHT),
        PublishOutcome::Published
    );
    runtime.pump();

    assert_eq!(
        renderer.drain_replay_acks(),
        1,
        "the frame's ack must be available WITHOUT draining its returns"
    );
    assert_eq!(
        renderer.pending_recycled_confirmations_capacity_for_tests(),
        Some(SEEDED_CAPACITY),
        "the ack alone must complete the capacity round-trip"
    );
    assert_eq!(
        drain_outcomes(&mut renderer),
        vec![(1, PresentOutcome::Presented)],
        "the returns still arrive whole behind the ack"
    );
    assert_eq!(
        renderer.drain_replay_acks(),
        0,
        "exactly one ack per consumed Direct packet"
    );
    assert_eq!(
        renderer.pending_recycled_confirmations_capacity_for_tests(),
        Some(SEEDED_CAPACITY),
        "the returns carry no second ack; nothing double-applies"
    );
}

/// 7b-5: the warmup snapshot. The present thread mirrors
/// `needs_frame_warmup` into the shared atomic after every consumed
/// packet, and the producer's `Renderer` trait read reports exactly that
/// atomic — there is no `GpuRenderer` on the producer side to consult.
#[test]
fn needs_frame_warmup_reads_present_thread_atomic() {
    let (_lock, mut renderer, device, queue, backend, downlevel) =
        inline_runtime_or_skip!("warmup atomic");
    let mut runtime = renderer.init_gpu_inline_for_tests(
        device,
        queue,
        wgpu::TextureFormat::Bgra8UnormSrgb,
        backend,
        downlevel,
    );
    let ack = renderer
        .send_attach_offscreen_unacked_for_tests(WIDTH, HEIGHT)
        .expect("inline runtime must accept controls");
    runtime.pump();
    ack.try_recv().expect("attach must ack after the pump");

    assert!(
        !renderer.needs_frame_warmup(),
        "before any frame the snapshot must read false"
    );

    // A first shadow render misses the shadow shape cache — one of the
    // warmup triggers — so the present thread must raise the atomic.
    renderer.scene_mut().graph = Some(shadowed_child_graph());
    assert_eq!(
        renderer.publish_frame(WIDTH, HEIGHT),
        PublishOutcome::Published
    );
    runtime.pump();
    assert_eq!(renderer.drain_present_returns(), 1);

    let (atomic_warmup, _, _) = renderer
        .present_status_snapshot_for_tests()
        .expect("threaded mode must expose the status snapshot");
    assert!(
        atomic_warmup,
        "the first shadow frame's cache miss must raise the warmup snapshot"
    );
    assert_eq!(
        renderer.needs_frame_warmup(),
        atomic_warmup,
        "the producer trait read must be exactly the atomic"
    );

    // Warmup frames decay as the caches stop missing; the trait read must
    // follow the atomic down without ever touching present state.
    for _ in 0..4 {
        if !renderer.needs_frame_warmup() {
            break;
        }
        assert_eq!(
            renderer.publish_frame(WIDTH, HEIGHT),
            PublishOutcome::Published
        );
        runtime.pump();
        assert_eq!(renderer.drain_present_returns(), 1);
    }
    let (atomic_warmup, _, _) = renderer
        .present_status_snapshot_for_tests()
        .expect("threaded mode must expose the status snapshot");
    assert_eq!(renderer.needs_frame_warmup(), atomic_warmup);
    assert!(
        !atomic_warmup,
        "warmup must settle once the shadow cache stops missing"
    );
}

/// 7b-6: real-thread smoke. The spawned runtime constructs its renderer
/// on its own thread, refuses a surfaceless packet with buffers returned,
/// acknowledges controls across the thread boundary, renders against the
/// offscreen surrogate, wakes the producer through the injected waker,
/// and shuts down joinable.
#[test]
fn real_thread_runtime_smoke() {
    let (_lock, mut renderer, device, queue, backend, downlevel) =
        inline_runtime_or_skip!("real-thread smoke");
    let (wake_tx, wake_rx) = std::sync::mpsc::channel::<()>();
    renderer
        .init_gpu_threaded(
            device,
            queue,
            wgpu::TextureFormat::Bgra8UnormSrgb,
            backend,
            downlevel,
            Arc::new(move || {
                let _ = wake_tx.send(());
            }),
            None,
        )
        .expect("present thread must spawn");
    renderer.scene_mut().graph = Some(direct_graph());

    // No surface attached: the packet must come back cancelled, buffers
    // intact, and the waker must have fired.
    assert_eq!(
        renderer.publish_frame(WIDTH, HEIGHT),
        PublishOutcome::Published
    );
    wake_rx
        .recv_timeout(Duration::from_secs(10))
        .expect("the present thread must wake the producer after returns");
    let outcomes = drain_with_timeout(&mut renderer, 1);
    assert_eq!(
        outcomes,
        vec![(
            1,
            PresentOutcome::Cancelled(CancelReason::SurfaceUnavailable)
        )],
        "a surfaceless runtime must refuse the packet, not drop it"
    );
    assert!(renderer.has_retained_direct_scene_for_tests());

    // Control ack round-trips across the thread; a publish under the new
    // epoch against the offscreen target renders for real.
    renderer.note_surface_reconfigured();
    assert!(
        renderer.present_reconfigure(surface_config(WIDTH, HEIGHT)),
        "reconfigure must be acknowledged"
    );
    assert!(
        renderer.present_attach_offscreen_for_tests(WIDTH, HEIGHT),
        "offscreen attach must be acknowledged"
    );
    assert_eq!(
        renderer.publish_frame(WIDTH, HEIGHT),
        PublishOutcome::Published
    );
    let outcomes = drain_with_timeout(&mut renderer, 1);
    assert_eq!(
        outcomes,
        vec![(2, PresentOutcome::Presented)],
        "the real present thread must render against the offscreen target"
    );
    let (_, _, presented_frames) = renderer
        .present_status_snapshot_for_tests()
        .expect("threaded mode must expose the status snapshot");
    assert_eq!(presented_frames, 1);

    renderer.shutdown_present_runtime();
    assert!(
        !renderer.needs_frame_warmup(),
        "after shutdown the renderer reads as uninitialized"
    );
}

/// Polls the returns channel until `count` returns arrived or a generous
/// deadline passed — the real-thread test's only nondeterminism absorber.
fn drain_with_timeout(renderer: &mut WgpuRenderer, count: usize) -> Vec<(u64, PresentOutcome)> {
    let deadline = Instant::now() + Duration::from_secs(10);
    let mut outcomes = Vec::new();
    while outcomes.len() < count && Instant::now() < deadline {
        renderer.drain_present_returns_with(&mut |frame_id, outcome, _| {
            outcomes.push((frame_id, outcome));
        });
        if outcomes.len() < count {
            std::thread::sleep(Duration::from_millis(5));
        }
    }
    outcomes
}
