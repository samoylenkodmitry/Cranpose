mod support;

use std::{
    sync::{Arc, MutexGuard},
    time::{Duration, Instant},
};

use cranpose_core::NodeId;
use cranpose_render_common::{
    Renderer,
    graph::{CachePolicy, LayerNode, ProjectiveTransform, RenderGraph, RenderNode},
};
use cranpose_render_wgpu::{CancelReason, PresentOutcome, PublishOutcome, WgpuRenderer};
use cranpose_ui_graphics::{Color, Rect};

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
        Some(7_700),
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

#[test]
fn a_surface_never_sits_configured_with_nothing_presented_to_it() {
    let (_lock, mut renderer, device, queue, backend, downlevel) =
        inline_runtime_or_skip!("placeholder frame");
    let mut runtime = renderer.init_gpu_inline_for_tests(
        device,
        queue,
        wgpu::TextureFormat::Bgra8UnormSrgb,
        backend,
        downlevel,
    );

    let (_, presented_before, placeholder_before) = renderer
        .present_status_snapshot_for_tests()
        .expect("threaded mode must expose the status snapshot");
    assert_eq!(
        (presented_before, placeholder_before),
        (0, 0),
        "nothing has been installed yet"
    );

    let ack = renderer
        .send_attach_offscreen_unacked_for_tests(WIDTH, HEIGHT)
        .expect("inline runtime must accept controls");
    runtime.pump();
    ack.try_recv()
        .expect("the attach must ack only after the placeholder clear runs");

    let (_, presented_frames, placeholder_frames) = renderer
        .present_status_snapshot_for_tests()
        .expect("threaded mode must expose the status snapshot");
    assert_eq!(
        placeholder_frames, 1,
        "installing the surface must fire exactly one placeholder clear,          before the producer could possibly have a real packet ready"
    );
    assert_eq!(
        presented_frames, 0,
        "the placeholder must not be counted as a real content frame"
    );

    renderer.scene_mut().graph = Some(direct_graph());
    assert_eq!(
        renderer.publish_frame(WIDTH, HEIGHT),
        PublishOutcome::Published
    );
    runtime.pump();
    assert_eq!(renderer.drain_present_returns(), 1);
    let (_, presented_frames, placeholder_frames) = renderer
        .present_status_snapshot_for_tests()
        .expect("threaded mode must expose the status snapshot");
    assert_eq!(presented_frames, 1);
    assert_eq!(
        placeholder_frames, 1,
        "a real frame publishing must not fire another placeholder clear"
    );
}

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
    renderer.note_surface_reconfigured();
    let ack = renderer
        .send_reconfigure_unacked_for_tests(surface_config(WIDTH * 2, HEIGHT * 2))
        .expect("inline runtime must accept controls");
    assert!(
        ack.try_recv().is_err(),
        "no ack may fire before the runtime processed the invalidation"
    );

    runtime.pump();
    assert_eq!(
        drain_outcomes(&mut renderer),
        vec![(1, PresentOutcome::Cancelled(CancelReason::SurfaceEpoch))],
        "the waiting packet must cancel for its stale surface epoch"
    );
    ack.try_recv()
        .expect("the ack must have fired — after the cancelled returns were sent");

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

    assert_eq!(
        renderer.publish_frame(WIDTH, HEIGHT),
        PublishOutcome::Published
    );

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
}

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
    let (_, presented_frames, _) = renderer
        .present_status_snapshot_for_tests()
        .expect("threaded mode must expose the status snapshot");
    assert_eq!(presented_frames, 1);

    renderer.shutdown_present_runtime();
    assert!(
        !renderer.needs_frame_warmup(),
        "after shutdown the renderer reads as uninitialized"
    );
}

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
