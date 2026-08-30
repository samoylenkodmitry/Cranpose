mod support;

use cranpose_render_common::{
    Renderer,
    graph::{DrawCommandId, DrawRunNode, PrimitivePhase, RenderGraph, RenderNode},
    style_shared::DrawPlacement,
};
use cranpose_ui_graphics::{
    Brush, Color, CommandReplayState, DrawScope, DrawScopeDefault, Point, Rect,
};

const SIZE: u32 = 408;
const CENTER: f32 = 204.0;
const FRAMES: usize = 8;

fn record_frame(frame: usize) -> DrawScopeDefault {
    let mut scope =
        DrawScopeDefault::new(cranpose_ui_graphics::Size::new(SIZE as f32, SIZE as f32));
    let breathing = 1.0 - 0.0005 * frame as f32;
    scope.draw_rect_at(
        Rect {
            x: 0.0,
            y: 0.0,
            width: SIZE as f32,
            height: SIZE as f32,
        },
        Brush::solid(Color(0.02, 0.02, 0.05, 1.0)),
    );
    for m in 0..(2 + frame % 3) {
        let x = 30.0 + frame as f32 * 7.0 + m as f32 * 15.0;
        scope.draw_circle(
            Brush::solid(Color(1.0, 1.0, 1.0, 1.0)),
            Point::new(x + 4.0, 44.0 + m as f32 * 12.0),
            4.0,
        );
    }
    for (ring, (radius, band, speed)) in [
        (150.0f32, 10.0f32, 0.013f32),
        (120.0, 9.0, -0.008),
        (90.0, 8.0, 0.019),
    ]
    .into_iter()
    .enumerate()
    {
        let radius = radius * breathing;
        let band = band * breathing;
        let count = 420usize;
        let sweep = std::f32::consts::TAU / count as f32 * 0.8;
        for i in 0..count {
            let start = i as f32 * (std::f32::consts::TAU / count as f32) + speed * frame as f32;
            scope.draw_annular_sector(
                Brush::solid(Color(0.3, 0.5 + (i % 5) as f32 * 0.08, 0.8, 1.0)),
                Point::new(CENTER, CENTER),
                radius - band,
                radius,
                start,
                sweep,
            );
        }
        if ring == 1 {
            for s in 0..(30 + (frame * 13) % 25) {
                let a = s as f32 * 0.7 + frame as f32 * 0.31;
                let r = 60.0 + ((s * 17 + frame * 29) % 90) as f32;
                scope.draw_circle(
                    Brush::solid(Color(1.0, 0.6, 0.2, 0.8)),
                    Point::new(CENTER + a.cos() * r, CENTER + a.sin() * r),
                    2.5,
                );
            }
        }
    }
    for d in 0..220 {
        let angle = d as f32 * 0.285;
        let orbit = 55.0 + (d % 7) as f32 * 3.0;
        let alpha = 0.25 + 0.7 * (((d + frame * 3) % 11) as f32 / 10.0);
        scope.draw_annular_sector(
            Brush::solid(Color(0.9, 0.85, 0.4, alpha)),
            Point::new(CENTER, CENTER),
            orbit - 3.0,
            orbit + 3.0,
            angle - 0.02,
            0.04,
        );
    }
    scope
}

fn graph_for(children: Vec<RenderNode>) -> RenderGraph {
    RenderGraph::new(support::layer_node(
        None,
        SIZE as f32,
        SIZE as f32,
        children,
    ))
}

fn command_for(node_id: usize) -> DrawCommandId {
    DrawCommandId {
        node_id,
        command_index: 0,
        placement: DrawPlacement::Behind,
    }
}

fn build_sequence(node_id: usize, bypass: &mut dyn FnMut(u32) -> bool) -> Vec<RenderGraph> {
    let mut state = CommandReplayState::default();
    let command = command_for(node_id);
    let mut graphs = Vec::with_capacity(FRAMES);
    for frame in 0..FRAMES {
        let scope = record_frame(frame);
        let outcome = state.advance(scope.recorded());
        let center = state.center();
        let (finished, replay) = scope.finish_replay(center, outcome, bypass);
        let fallback = std::rc::Rc::new(finished.recording);
        let replay = replay.map(|mut frame| {
            frame.fallback = Some(fallback);
            Box::new(frame)
        });
        graphs.push(graph_for(vec![RenderNode::DrawRun(
            DrawRunNode::for_command_replayed(
                PrimitivePhase::BeforeChildren,
                Some(command),
                std::rc::Rc::new(finished.primitives),
                replay,
            ),
        )]));
    }
    graphs
}

fn render_sequence(renderer: &mut support::LockedRenderer, graphs: &[RenderGraph]) -> Vec<Vec<u8>> {
    graphs
        .iter()
        .enumerate()
        .map(|(frame, graph)| {
            renderer.scene_mut().graph = Some(graph.clone());
            let captured = renderer
                .capture_frame(SIZE, SIZE)
                .unwrap_or_else(|err| panic!("frame {frame} capture failed: {err:?}"));
            assert_eq!((captured.width, captured.height), (SIZE, SIZE));
            captured.pixels
        })
        .collect()
}

fn run_primitives(graphs: &[RenderGraph]) -> usize {
    graphs
        .iter()
        .map(|graph| match &graph.root.children[0] {
            RenderNode::DrawRun(run) => run.primitives.len(),
            _ => 0,
        })
        .sum()
}

fn differing_bytes(a: &[u8], b: &[u8]) -> usize {
    assert_eq!(a.len(), b.len());
    a.iter().zip(b).filter(|(a, b)| a != b).count()
}

fn channels_differing_over(a: &[u8], b: &[u8], threshold: u8) -> usize {
    assert_eq!(a.len(), b.len());
    a.iter()
        .zip(b)
        .filter(|(a, b)| a.abs_diff(**b) > threshold)
        .count()
}

fn assert_noise_only(label: &str, a: &[Vec<u8>], b: &[Vec<u8>]) {
    for (frame, (a, b)) in a.iter().zip(b).enumerate() {
        let worst = a
            .iter()
            .zip(b)
            .map(|(a, b)| a.abs_diff(*b))
            .max()
            .unwrap_or(0);
        eprintln!("{label} frame {frame}: worst {worst}");
        assert!(
            worst <= 3,
            "{label} frame {frame}: worst channel diff {worst} exceeds blending noise — \
             content differs"
        );
    }
}

fn assert_byte_exact(label: &str, a: &[Vec<u8>], b: &[Vec<u8>]) {
    for (frame, (a, b)) in a.iter().zip(b).enumerate() {
        let differing = differing_bytes(a, b);
        if differing != 0 {
            let mut worst = 0u8;
            let (mut min_x, mut min_y, mut max_x, mut max_y) = (u32::MAX, u32::MAX, 0u32, 0u32);
            for (i, (a, b)) in a.iter().zip(b).enumerate() {
                let diff = a.abs_diff(*b);
                if diff > 0 {
                    worst = worst.max(diff);
                    let pixel = (i / 4) as u32;
                    let (x, y) = (pixel % SIZE, pixel / SIZE);
                    min_x = min_x.min(x);
                    min_y = min_y.min(y);
                    max_x = max_x.max(x);
                    max_y = max_y.max(y);
                }
            }
            eprintln!(
                "{label} frame {frame}: differing {differing} worst {worst} \
                 region x {min_x}..{max_x} y {min_y}..{max_y}"
            );
        } else {
            eprintln!("{label} frame {frame}: differing 0");
        }
        assert_eq!(
            differing, 0,
            "{label} frame {frame}: {differing} bytes differ"
        );
    }
}

fn declare_current_epoch() {
    cranpose_render_common::scene_builder::set_retained_feed_epoch(Some(
        cranpose_render_wgpu::retained_feed_generation(),
    ));
}

fn build_bypassed_sequence(node_id: usize, full_primitives: usize) -> Vec<RenderGraph> {
    declare_current_epoch();
    let command = command_for(node_id);
    let graphs = build_sequence(node_id, &mut |slot| {
        cranpose_render_common::scene_builder::retained_slot_confirmed(command, slot)
    });
    let bypassed_primitives = run_primitives(&graphs);
    assert!(
        bypassed_primitives < full_primitives / 2,
        "confirmed slots should have bypassed materialization \
         ({bypassed_primitives} of {full_primitives} still materialized)"
    );
    graphs
}

#[test]
fn feed_disabled_after_build_rematerializes_bypassed_spans() {
    let mut renderer = match support::headless_renderer() {
        Ok(renderer) => renderer,
        Err(err) => {
            eprintln!("skipping fail-closed feed-disable: headless WGPU init failed: {err}");
            return;
        }
    };
    cranpose_render_wgpu::set_debug_toggle("CRANPOSE_ARC_MESH", Some("0"));
    cranpose_render_wgpu::set_debug_toggle("CRANPOSE_COMMAND_FEED", Some("1"));

    let graphs = build_sequence(31, &mut |_| false);
    let _ = render_sequence(&mut renderer, &graphs);
    let bypassed_graphs = build_bypassed_sequence(31, run_primitives(&graphs));

    cranpose_render_wgpu::set_debug_toggle("CRANPOSE_COMMAND_FEED", Some("0"));
    let _ = render_sequence(&mut renderer, &graphs);
    let control = render_sequence(&mut renderer, &graphs);
    let rematerialized = render_sequence(&mut renderer, &bypassed_graphs);
    cranpose_render_wgpu::set_debug_toggle("CRANPOSE_COMMAND_FEED", None);
    cranpose_render_wgpu::set_debug_toggle("CRANPOSE_ARC_MESH", None);

    let (_, _, remat_misses) = cranpose_render_wgpu::command_feed_live_stats();
    assert_eq!(
        remat_misses, 0,
        "every bypassed span must have rebuilt from its frame's own recording"
    );
    assert_byte_exact("feed-disabled-vs-control", &control, &rematerialized);
}

#[test]
fn renderer_swap_revokes_confirmations_and_rematerializes() {
    let mut renderer = match support::headless_renderer() {
        Ok(renderer) => renderer,
        Err(err) => {
            eprintln!("skipping fail-closed renderer swap: headless WGPU init failed: {err}");
            return;
        }
    };
    cranpose_render_wgpu::set_debug_toggle("CRANPOSE_ARC_MESH", Some("0"));
    cranpose_render_wgpu::set_debug_toggle("CRANPOSE_COMMAND_FEED", Some("1"));
    let command = command_for(32);

    let graphs = build_sequence(32, &mut |_| false);
    let _ = render_sequence(&mut renderer, &graphs);
    let bypassed_graphs = build_bypassed_sequence(32, run_primitives(&graphs));

    let generation_before = cranpose_render_wgpu::retained_feed_generation();
    support::reinit_gpu(&mut renderer).expect("GPU reinit failed");
    assert_eq!(
        cranpose_render_wgpu::retained_feed_generation(),
        generation_before + 1,
        "renderer replacement must move the feed to a fresh generation"
    );
    let (feed_slots, _, _) = cranpose_render_wgpu::command_feed_live_stats();
    assert_eq!(feed_slots, 0, "the old renderer's feed slots must be gone");
    declare_current_epoch();
    for slot in 0..32 {
        assert!(
            !cranpose_render_common::scene_builder::retained_slot_confirmed(command, slot),
            "slot {slot} still confirmed after the swap"
        );
    }

    let last = FRAMES - 1;
    let remat_frame = render_sequence(&mut renderer, &bypassed_graphs[last..]);
    let (_, _, remat_misses) = cranpose_render_wgpu::command_feed_live_stats();
    assert_eq!(
        remat_misses, 0,
        "the frames own their recordings; no span may terminal-miss"
    );
    cranpose_render_wgpu::set_debug_toggle("CRANPOSE_COMMAND_FEED", Some("0"));
    let ordinary_frame = render_sequence(&mut renderer, &graphs[last..]);
    assert_noise_only("swap-remat-vs-ordinary", &ordinary_frame, &remat_frame);

    cranpose_render_wgpu::set_debug_toggle("CRANPOSE_COMMAND_FEED", Some("1"));
    let _ = render_sequence(&mut renderer, &bypassed_graphs);
    let (feed_slots, _, remat_misses) = cranpose_render_wgpu::command_feed_live_stats();
    assert!(
        feed_slots >= 4,
        "the new renderer must have re-earned feed slots, got {feed_slots}"
    );
    assert_eq!(remat_misses, 0);
    let rebuilt_graphs = build_bypassed_sequence(32, run_primitives(&graphs));
    let _ = render_sequence(&mut renderer, &graphs);
    let control = render_sequence(&mut renderer, &graphs);
    let rebuilt = render_sequence(&mut renderer, &rebuilt_graphs);
    cranpose_render_wgpu::set_debug_toggle("CRANPOSE_COMMAND_FEED", None);
    cranpose_render_wgpu::set_debug_toggle("CRANPOSE_ARC_MESH", None);
    assert_byte_exact("post-swap-rebuild-vs-control", &control, &rebuilt);
}

#[test]
fn registry_loss_no_longer_reaches_the_miss_terminal() {
    let mut renderer = match support::headless_renderer() {
        Ok(renderer) => renderer,
        Err(err) => {
            eprintln!("skipping fail-closed registry loss: headless WGPU init failed: {err}");
            return;
        }
    };
    cranpose_render_wgpu::set_debug_toggle("CRANPOSE_ARC_MESH", Some("0"));
    cranpose_render_wgpu::set_debug_toggle("CRANPOSE_COMMAND_FEED", Some("1"));

    let graphs = build_sequence(33, &mut |_| false);
    let _ = render_sequence(&mut renderer, &graphs);
    let bypassed_graphs = build_bypassed_sequence(33, run_primitives(&graphs));

    cranpose_render_common::scene_builder::clear_command_recordings_for_tests();
    support::reinit_gpu(&mut renderer).expect("GPU reinit failed");

    let last = FRAMES - 1;
    let remat_frame = render_sequence(&mut renderer, &bypassed_graphs[last..]);
    let (_, _, remat_misses) = cranpose_render_wgpu::command_feed_live_stats();
    assert_eq!(
        remat_misses, 0,
        "the frame-owned fallback must serve every bypassed span; registry \
         loss may no longer reach the terminal"
    );

    cranpose_render_wgpu::set_debug_toggle("CRANPOSE_COMMAND_FEED", Some("0"));
    let ordinary_frame = render_sequence(&mut renderer, &graphs[last..]);
    cranpose_render_wgpu::set_debug_toggle("CRANPOSE_COMMAND_FEED", None);
    cranpose_render_wgpu::set_debug_toggle("CRANPOSE_ARC_MESH", None);
    assert_noise_only("registry-loss-vs-ordinary", &ordinary_frame, &remat_frame);
}

#[test]
fn orphaned_frame_terminal_counts_revokes_and_self_heals() {
    let mut renderer = match support::headless_renderer() {
        Ok(renderer) => renderer,
        Err(err) => {
            eprintln!("skipping fail-closed orphaned frame: headless WGPU init failed: {err}");
            return;
        }
    };
    cranpose_render_wgpu::set_debug_toggle("CRANPOSE_ARC_MESH", Some("0"));
    cranpose_render_wgpu::set_debug_toggle("CRANPOSE_COMMAND_FEED", Some("1"));
    let command = command_for(35);

    let graphs = build_sequence(35, &mut |_| false);
    let _ = render_sequence(&mut renderer, &graphs);
    let bypassed_graphs = build_bypassed_sequence(35, run_primitives(&graphs));

    let last = FRAMES - 1;
    let mut orphaned = bypassed_graphs[last].clone();
    {
        let RenderNode::DrawRun(run) = &mut orphaned.root.children[0] else {
            panic!("expected draw run");
        };
        let frame = run
            .replay
            .as_mut()
            .expect("the bypassed graph carries a replay frame");
        assert!(
            frame.fallback.is_some(),
            "the build path must have attached the frame's fallback"
        );
        frame.fallback = None;
    }
    cranpose_render_common::scene_builder::clear_command_recordings_for_tests();
    support::reinit_gpu(&mut renderer).expect("GPU reinit failed");

    renderer.scene_mut().graph = Some(orphaned);
    let missing_frame = renderer
        .capture_frame(SIZE, SIZE)
        .expect("the terminal must not panic or fail the frame");
    let (_, _, remat_misses) = cranpose_render_wgpu::command_feed_live_stats();
    assert!(
        remat_misses > 0,
        "an orphaned frame's bypassed spans must reach the miss terminal"
    );
    declare_current_epoch();
    for slot in 0..32 {
        assert!(
            !cranpose_render_common::scene_builder::retained_slot_confirmed(command, slot),
            "slot {slot} must be revoked after its miss"
        );
    }

    cranpose_render_wgpu::set_debug_toggle("CRANPOSE_COMMAND_FEED", Some("0"));
    let _ = render_sequence(&mut renderer, &graphs[last..]);
    let control = render_sequence(&mut renderer, &graphs[last..]);
    let missing_differs = channels_differing_over(&control[0], &missing_frame.pixels, 60);
    assert!(
        missing_differs > 10_000,
        "the miss frame should visibly lack the bypassed rings \
         ({missing_differs} channels differ by more than 60)"
    );
    let healed_graphs = build_sequence(35, &mut |slot| {
        cranpose_render_common::scene_builder::retained_slot_confirmed(command, slot)
    });
    assert_eq!(
        run_primitives(&healed_graphs),
        run_primitives(&graphs),
        "with every confirmation revoked the next build must materialize fully"
    );
    let healed = render_sequence(&mut renderer, &healed_graphs[last..]);
    cranpose_render_wgpu::set_debug_toggle("CRANPOSE_COMMAND_FEED", None);
    cranpose_render_wgpu::set_debug_toggle("CRANPOSE_ARC_MESH", None);
    assert_byte_exact("self-heal-vs-control", &control, &healed);
}

#[test]
fn mismatched_generation_replay_ops_drop_whole() {
    let mut renderer = match support::headless_renderer() {
        Ok(renderer) => renderer,
        Err(err) => {
            eprintln!("skipping fail-closed generation mismatch: headless WGPU init failed: {err}");
            return;
        }
    };
    support::reinit_gpu(&mut renderer).expect("GPU reinit failed");
    let command = command_for(36);
    cranpose_render_wgpu::inject_feed_capture_for_tests(command, 0, 0, 4);
    let confirmed = renderer.replay_ops_roundtrip_for_tests(u64::MAX);
    assert_eq!(
        confirmed, 0,
        "a stale-generation batch must confirm nothing"
    );
    assert_eq!(
        cranpose_render_wgpu::pending_feed_capture_count_for_tests(),
        0,
        "the batch is consumed whole, never requeued"
    );
    let (_, live_slots) = renderer.replay_slot_mesh_stats();
    assert_eq!(
        live_slots, 0,
        "no GPU slot may be captured from a dropped batch"
    );
    let (feed_slots, _, _) = cranpose_render_wgpu::command_feed_live_stats();
    assert_eq!(
        feed_slots, 0,
        "a dropped capture must never become a feed slot"
    );
    declare_current_epoch();
    assert!(
        !cranpose_render_common::scene_builder::retained_slot_confirmed(command, 0),
        "a dropped capture must never confirm its slot"
    );
}

#[test]
fn higher_generation_replay_ops_adopt_forward() {
    let mut renderer = match support::headless_renderer() {
        Ok(renderer) => renderer,
        Err(err) => {
            eprintln!("skipping adopt-forward: headless WGPU init failed: {err}");
            return;
        }
    };
    cranpose_render_wgpu::set_debug_toggle("CRANPOSE_ARC_MESH", Some("0"));
    cranpose_render_wgpu::set_debug_toggle("CRANPOSE_COMMAND_FEED", Some("1"));

    let graphs = build_sequence(37, &mut |_| false);
    let _ = render_sequence(&mut renderer, &graphs);
    let (feed_slots, _, _) = cranpose_render_wgpu::command_feed_live_stats();
    if feed_slots == 0 {
        eprintln!("skipping adopt-forward: device has no retained replay support");
        cranpose_render_wgpu::set_debug_toggle("CRANPOSE_COMMAND_FEED", None);
        cranpose_render_wgpu::set_debug_toggle("CRANPOSE_ARC_MESH", None);
        return;
    }
    let drops_before = renderer.replay_generation_drops_for_tests();
    let generation_before = cranpose_render_wgpu::retained_feed_generation();

    renderer.scene_mut().graph = Some(graphs[0].clone());
    renderer
        .capture_frame_with_scale(SIZE, SIZE, 2.0)
        .expect("scale-change frame failed");
    assert_eq!(
        cranpose_render_wgpu::retained_feed_generation(),
        generation_before + 1,
        "the scale change must have retired the feed to a fresh generation"
    );
    assert_eq!(
        renderer.replay_generation_drops_for_tests(),
        drops_before,
        "a higher-generation batch must be adopted and served, never dropped"
    );

    for graph in &graphs {
        renderer.scene_mut().graph = Some(graph.clone());
        renderer
            .capture_frame_with_scale(SIZE, SIZE, 2.0)
            .expect("post-adopt frame failed");
    }
    let (feed_slots_after, _, _) = cranpose_render_wgpu::command_feed_live_stats();
    cranpose_render_wgpu::set_debug_toggle("CRANPOSE_COMMAND_FEED", None);
    cranpose_render_wgpu::set_debug_toggle("CRANPOSE_ARC_MESH", None);
    assert!(
        feed_slots_after > 0,
        "the feed must re-earn slots under the adopted generation"
    );
    assert_eq!(
        renderer.replay_generation_drops_for_tests(),
        drops_before,
        "no batch of the adopted generation may count a generation drop"
    );
}

#[test]
fn cancelled_packet_requeues_feed_releases() {
    let mut renderer = match support::headless_renderer() {
        Ok(renderer) => renderer,
        Err(err) => {
            eprintln!("skipping cancelled-release requeue: headless WGPU init failed: {err}");
            return;
        }
    };
    cranpose_render_wgpu::set_debug_toggle("CRANPOSE_ARC_MESH", Some("0"));
    cranpose_render_wgpu::set_debug_toggle("CRANPOSE_COMMAND_FEED", Some("1"));

    let graphs = build_sequence(38, &mut |_| false);
    let _ = render_sequence(&mut renderer, &graphs);
    let (feed_slots, _, _) = cranpose_render_wgpu::command_feed_live_stats();
    if feed_slots == 0 {
        eprintln!("skipping cancelled-release requeue: device has no retained replay support");
        cranpose_render_wgpu::set_debug_toggle("CRANPOSE_COMMAND_FEED", None);
        cranpose_render_wgpu::set_debug_toggle("CRANPOSE_ARC_MESH", None);
        return;
    }
    let (_, live_before) = renderer.replay_slot_mesh_stats();
    let drops_before = renderer.replay_generation_drops_for_tests();

    renderer.set_root_scale(2.0);
    renderer.scene_mut().graph = Some(graphs[0].clone());
    let packet = renderer
        .build_frame_packet_for_tests(SIZE, SIZE)
        .expect("scale-change packet must build");
    let (queued, _) = cranpose_render_wgpu::planner_replay_queue_stats_for_tests();
    assert_eq!(queued, 0, "the packet's plan must have taken the releases");

    renderer.note_surface_reconfigured();
    let device = renderer
        .try_device()
        .expect("renderer GPU device was not initialized");
    let texture = device.create_texture(&wgpu::TextureDescriptor {
        label: Some("Cancelled Release Requeue Target"),
        size: wgpu::Extent3d {
            width: SIZE,
            height: SIZE,
            depth_or_array_layers: 1,
        },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: wgpu::TextureFormat::Bgra8UnormSrgb,
        usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
        view_formats: &[],
    });
    let view = texture.create_view(&wgpu::TextureViewDescriptor::default());
    let outcome = renderer
        .render_held_packet_for_tests(&view, SIZE, SIZE, packet)
        .expect("a cancel is a protocol outcome, not a draw error");
    assert_eq!(
        outcome,
        cranpose_render_wgpu::PresentOutcome::Cancelled(
            cranpose_render_wgpu::CancelReason::SurfaceEpoch
        )
    );

    let (requeued, _) = cranpose_render_wgpu::planner_replay_queue_stats_for_tests();
    assert!(
        requeued >= feed_slots,
        "the cancelled retirement releases must re-queue whole \
         ({requeued} queued, {feed_slots} retired slots)"
    );
    assert_eq!(
        renderer.replay_slot_mesh_stats().1,
        live_before,
        "a cancelled packet must not free store slots"
    );

    renderer.scene_mut().graph = Some(graphs[0].clone());
    renderer
        .capture_frame(SIZE, SIZE)
        .expect("post-cancel frame failed");
    let (queued_after, _) = cranpose_render_wgpu::planner_replay_queue_stats_for_tests();
    assert_eq!(
        queued_after, 0,
        "the next frame's batch must carry the re-queued releases"
    );
    assert_eq!(
        renderer.replay_generation_drops_for_tests(),
        drops_before,
        "serving the re-queued releases must not count a generation drop"
    );
    cranpose_render_wgpu::set_debug_toggle("CRANPOSE_COMMAND_FEED", None);
    cranpose_render_wgpu::set_debug_toggle("CRANPOSE_ARC_MESH", None);
}

#[test]
fn stale_feed_captures_are_dropped_at_the_drain() {
    let mut renderer = match support::headless_renderer() {
        Ok(renderer) => renderer,
        Err(err) => {
            eprintln!("skipping fail-closed stale capture: headless WGPU init failed: {err}");
            return;
        }
    };
    let command = command_for(34);

    let finished = record_frame(0).finish();
    assert!(finished.primitives.len() > 128);
    let graph = graph_for(vec![RenderNode::DrawRun(
        DrawRunNode::for_command_replayed(
            PrimitivePhase::BeforeChildren,
            Some(command),
            std::rc::Rc::new(finished.primitives),
            None,
        ),
    )]);
    renderer.scene_mut().graph = Some(graph.clone());
    renderer
        .capture_frame(SIZE, SIZE)
        .expect("warm frame failed");

    cranpose_render_wgpu::inject_feed_capture_for_tests(command, 0, 0, 64);
    assert_eq!(
        cranpose_render_wgpu::pending_feed_capture_count_for_tests(),
        1
    );
    renderer
        .capture_frame(SIZE, SIZE)
        .expect("drain frame failed");

    assert_eq!(
        cranpose_render_wgpu::pending_feed_capture_count_for_tests(),
        0,
        "the drain must consume the stale capture"
    );
    let (feed_slots, _, _) = cranpose_render_wgpu::command_feed_live_stats();
    assert_eq!(
        feed_slots, 0,
        "a stale capture must never become a live feed slot"
    );
    declare_current_epoch();
    assert!(
        !cranpose_render_common::scene_builder::retained_slot_confirmed(command, 0),
        "a stale capture must never confirm its slot"
    );
}
