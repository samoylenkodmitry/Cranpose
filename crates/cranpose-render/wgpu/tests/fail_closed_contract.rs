//! The fail-closed command feed and live bypass contract (sol's
//! correctness gates): a bypassed span — empty primitive range, skipped at
//! materialization because the renderer confirmed it holds the slot — must
//! NEVER be silently omitted from drawing. Feed disable, renderer
//! replacement, and recording loss must all end in either a full
//! rematerialization or a loud, counted, self-healing terminal.
//!
//! Same-position-control discipline (documented in `command_feed_parity`):
//! renders are only ever compared against renders taken from equivalent
//! renderer state, and byte-exactness is asserted only between passes whose
//! pipelines are deterministic (feed and flat detector both off, or both
//! at stable slot positions).

mod support;

use cranpose_render_common::graph::{
    CachePolicy, DrawCommandId, DrawRunNode, IsolationReasons, LayerNode, PrimitivePhase,
    ProjectiveTransform, RenderGraph, RenderNode,
};
use cranpose_render_common::raster_cache::LayerRasterCacheHashes;
use cranpose_render_common::style_shared::DrawPlacement;
use cranpose_render_common::Renderer;
use cranpose_ui_graphics::{
    Brush, Color, CommandRecording, CommandReplayState, DrawScope, DrawScopeDefault, GraphicsLayer,
    Point, Rect,
};

const SIZE: u32 = 408;
const CENTER: f32 = 204.0;
const FRAMES: usize = 8;

/// One frame of the synthetic boss through the RECORDING path, identical in
/// structure to `command_feed_parity`: rings rotating at distinct speeds
/// under a breathing scale, churning sparks, recoloring twinkles, movers
/// whose count changes every frame.
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
    RenderGraph::new(LayerNode {
        node_id: None,
        local_bounds: Rect {
            x: 0.0,
            y: 0.0,
            width: SIZE as f32,
            height: SIZE as f32,
        },
        transform_to_parent: ProjectiveTransform::identity(),
        content_offset: Point::default(),
        motion_context_animated: false,
        translated_content_context: false,
        translated_content_offset: Point::default(),
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
    })
}

fn command_for(node_id: usize) -> DrawCommandId {
    DrawCommandId {
        node_id,
        command_index: 0,
        placement: DrawPlacement::Behind,
    }
}

/// Records every frame through one live `CommandReplayState`, exactly as
/// the scene builder's verifier would, returning each frame's graph AND its
/// recording — the recording a production build would leave in the registry
/// for that frame, which `render_sequence` republishes before rendering the
/// frame so `materialize_command_span` sees frame-consistent tape ranges.
fn build_sequence(
    node_id: usize,
    bypass: &mut dyn FnMut(u32) -> bool,
) -> (Vec<RenderGraph>, Vec<CommandRecording>) {
    let mut state = CommandReplayState::default();
    let command = command_for(node_id);
    let mut graphs = Vec::with_capacity(FRAMES);
    let mut recordings = Vec::with_capacity(FRAMES);
    for frame in 0..FRAMES {
        let scope = record_frame(frame);
        let outcome = state.advance(scope.recorded());
        let center = state.center();
        let (finished, replay) = scope.finish_replay(center, outcome, bypass);
        recordings.push(finished.recording.clone());
        graphs.push(graph_for(vec![RenderNode::DrawRun(
            DrawRunNode::for_command_replayed(
                PrimitivePhase::BeforeChildren,
                Some(command),
                std::rc::Rc::new(finished.primitives),
                replay.map(Box::new),
            ),
        )]));
    }
    (graphs, recordings)
}

/// Renders the sequence, republishing each frame's recording under the
/// command first — the production invariant that the recording in the
/// registry is the one the frame being rendered was built from.
fn render_sequence(
    renderer: &mut support::LockedRenderer,
    command: DrawCommandId,
    graphs: &[RenderGraph],
    recordings: &[CommandRecording],
) -> Vec<Vec<u8>> {
    graphs
        .iter()
        .zip(recordings)
        .enumerate()
        .map(|(frame, (graph, recording))| {
            cranpose_render_common::scene_builder::publish_command_recording_for_tests(
                command,
                recording.clone(),
            );
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

/// Asserts two frames differ by at most the renderer's pass-position
/// blending noise (≤2/255, documented in `command_feed_parity`): identical
/// CONTENT through different pipeline positions. Missing content would put
/// tens of thousands of channels at diffs near 255.
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

/// Declares the current feed generation as the build epoch, exactly as the
/// production pipeline does before every build.
fn declare_current_epoch() {
    cranpose_render_common::scene_builder::set_retained_feed_epoch(Some(
        cranpose_render_wgpu::retained_feed_generation(),
    ));
}

/// Builds the bypassed sequence against live confirmations and asserts the
/// bypass actually engaged (materialization halved), so the fail-closed
/// assertions that follow are not vacuous.
fn build_bypassed_sequence(
    node_id: usize,
    full_primitives: usize,
) -> (Vec<RenderGraph>, Vec<CommandRecording>) {
    declare_current_epoch();
    let command = command_for(node_id);
    let (graphs, recordings) = build_sequence(node_id, &mut |slot| {
        cranpose_render_common::scene_builder::retained_slot_confirmed(command, slot)
    });
    let bypassed_primitives = run_primitives(&graphs);
    assert!(
        bypassed_primitives < full_primitives / 2,
        "confirmed slots should have bypassed materialization \
         ({bypassed_primitives} of {full_primitives} still materialized)"
    );
    (graphs, recordings)
}

/// T2: graphs built WITH bypass (confirmations live) must render complete
/// after the feed is disabled — the fail-closed walk rebuilds every
/// bypassed span from its recording, byte-identical to a never-bypassed
/// control through the pure ordinary pipeline.
#[test]
fn feed_disabled_after_build_rematerializes_bypassed_spans() {
    let mut renderer = match support::headless_renderer() {
        Ok(renderer) => renderer,
        Err(err) => {
            eprintln!("skipping fail-closed feed-disable: headless WGPU init failed: {err}");
            return;
        }
    };
    std::env::set_var("CRANPOSE_ARC_MESH", "0");
    std::env::set_var("CRANPOSE_SIMILARITY_REPLAY", "1");
    std::env::set_var("CRANPOSE_COMMAND_FEED", "1");
    let command = command_for(31);

    // Warm: captures land and slots are confirmed.
    let (graphs, recordings) = build_sequence(31, &mut |_| false);
    let _ = render_sequence(&mut renderer, command, &graphs, &recordings);
    let (bypassed_graphs, bypassed_recordings) =
        build_bypassed_sequence(31, run_primitives(&graphs));

    // Flip the feed (and the flat detector) off: from here both passes are
    // deterministic CPU emission with no retention state at all. One warm
    // pass absorbs the pass-position transition (same-position-control
    // discipline, see `command_feed_parity`) before anything is compared.
    std::env::set_var("CRANPOSE_COMMAND_FEED", "0");
    std::env::set_var("CRANPOSE_SIMILARITY_REPLAY", "0");
    let _ = render_sequence(&mut renderer, command, &graphs, &recordings);
    let control = render_sequence(&mut renderer, command, &graphs, &recordings);
    let rematerialized = render_sequence(
        &mut renderer,
        command,
        &bypassed_graphs,
        &bypassed_recordings,
    );
    std::env::remove_var("CRANPOSE_COMMAND_FEED");
    std::env::remove_var("CRANPOSE_SIMILARITY_REPLAY");
    std::env::remove_var("CRANPOSE_ARC_MESH");

    let (_, _, remat_misses) = cranpose_render_wgpu::command_feed_live_stats();
    assert_eq!(
        remat_misses, 0,
        "every bypassed span must have rebuilt from its recording"
    );
    assert_byte_exact("feed-disabled-vs-control", &control, &rematerialized);
}

/// T3: replacing the GPU renderer under live confirmations (surface
/// recreation semantics) must revoke every confirmation and bump the feed
/// generation BEFORE the new renderer exists; in-flight bypassed graphs
/// rematerialize completely, and after one rebuild the fed output is
/// byte-identical to a control render.
#[test]
fn renderer_swap_revokes_confirmations_and_rematerializes() {
    let mut renderer = match support::headless_renderer() {
        Ok(renderer) => renderer,
        Err(err) => {
            eprintln!("skipping fail-closed renderer swap: headless WGPU init failed: {err}");
            return;
        }
    };
    std::env::set_var("CRANPOSE_ARC_MESH", "0");
    std::env::set_var("CRANPOSE_SIMILARITY_REPLAY", "1");
    std::env::set_var("CRANPOSE_COMMAND_FEED", "1");
    let command = command_for(32);

    let (graphs, recordings) = build_sequence(32, &mut |_| false);
    let _ = render_sequence(&mut renderer, command, &graphs, &recordings);
    let (bypassed_graphs, bypassed_recordings) =
        build_bypassed_sequence(32, run_primitives(&graphs));

    // The swap: same replacement path Android's surface recreation takes.
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

    // The in-flight frame: a bypassed graph rendered on the NEW renderer
    // without rebuilding. Every bypassed span must rematerialize from its
    // recording — byte-identical to the pure ordinary pipeline.
    let last = FRAMES - 1;
    let remat_frame = render_sequence(
        &mut renderer,
        command,
        &bypassed_graphs[last..],
        &bypassed_recordings[last..],
    );
    let (_, _, remat_misses) = cranpose_render_wgpu::command_feed_live_stats();
    assert_eq!(
        remat_misses, 0,
        "recordings exist; no span may terminal-miss"
    );
    // The remat frame is by definition a first-pass-at-position render (the
    // in-flight frame right after the swap), so it is compared under the
    // blending-noise envelope, not byte-exactly; T2 proves byte-exactness
    // of the rematerialization walk from stable positions.
    std::env::set_var("CRANPOSE_COMMAND_FEED", "0");
    std::env::set_var("CRANPOSE_SIMILARITY_REPLAY", "0");
    let ordinary_frame =
        render_sequence(&mut renderer, command, &graphs[last..], &recordings[last..]);
    assert_noise_only("swap-remat-vs-ordinary", &ordinary_frame, &remat_frame);

    // Heal: a full fed pass re-earns slots and confirmations on the new
    // renderer, and one rebuild later bypass output is byte-identical to a
    // same-position fed control.
    std::env::set_var("CRANPOSE_COMMAND_FEED", "1");
    std::env::set_var("CRANPOSE_SIMILARITY_REPLAY", "1");
    let _ = render_sequence(
        &mut renderer,
        command,
        &bypassed_graphs,
        &bypassed_recordings,
    );
    let (feed_slots, _, remat_misses) = cranpose_render_wgpu::command_feed_live_stats();
    assert!(
        feed_slots >= 4,
        "the new renderer must have re-earned feed slots, got {feed_slots}"
    );
    assert_eq!(remat_misses, 0);
    let (rebuilt_graphs, rebuilt_recordings) = build_bypassed_sequence(32, run_primitives(&graphs));
    // Same-position-control discipline: one warm pass, then the compared
    // control and bypassed passes render from stable renderer state.
    let _ = render_sequence(&mut renderer, command, &graphs, &recordings);
    let control = render_sequence(&mut renderer, command, &graphs, &recordings);
    let rebuilt = render_sequence(&mut renderer, command, &rebuilt_graphs, &rebuilt_recordings);
    std::env::remove_var("CRANPOSE_COMMAND_FEED");
    std::env::remove_var("CRANPOSE_SIMILARITY_REPLAY");
    std::env::remove_var("CRANPOSE_ARC_MESH");
    assert_byte_exact("post-swap-rebuild-vs-control", &control, &rebuilt);
}

/// T4: a bypassed span whose recording AND retained buffer are both gone is
/// the terminal: counted, revoked, and self-healing at the next build.
#[test]
fn remat_miss_terminal_counts_revokes_and_self_heals() {
    let mut renderer = match support::headless_renderer() {
        Ok(renderer) => renderer,
        Err(err) => {
            eprintln!("skipping fail-closed remat-miss: headless WGPU init failed: {err}");
            return;
        }
    };
    std::env::set_var("CRANPOSE_ARC_MESH", "0");
    std::env::set_var("CRANPOSE_SIMILARITY_REPLAY", "1");
    std::env::set_var("CRANPOSE_COMMAND_FEED", "1");
    let command = command_for(33);

    let (graphs, recordings) = build_sequence(33, &mut |_| false);
    let _ = render_sequence(&mut renderer, command, &graphs, &recordings);
    let (bypassed_graphs, _) = build_bypassed_sequence(33, run_primitives(&graphs));

    // Kill both draw sources for the bypassed spans: the recordings and
    // (via the swap) the retained slots.
    cranpose_render_common::scene_builder::clear_command_recordings_for_tests();
    support::reinit_gpu(&mut renderer).expect("GPU reinit failed");

    let last = FRAMES - 1;
    renderer.scene_mut().graph = Some(bypassed_graphs[last].clone());
    let missing_frame = renderer
        .capture_frame(SIZE, SIZE)
        .expect("the terminal must not panic or fail the frame");
    let (_, _, remat_misses) = cranpose_render_wgpu::command_feed_live_stats();
    assert!(
        remat_misses > 0,
        "clearing recordings must drive bypassed spans into the miss terminal"
    );
    declare_current_epoch();
    for slot in 0..32 {
        assert!(
            !cranpose_render_common::scene_builder::retained_slot_confirmed(command, slot),
            "slot {slot} must be revoked after its miss"
        );
    }

    // Non-vacuity + self-heal, both against the pure ordinary pipeline: the
    // miss frame is missing its rings; the next build (nothing confirmed,
    // so nothing bypasses) renders byte-identical to control. One warm
    // pass absorbs the pass-position transition before comparing.
    std::env::set_var("CRANPOSE_COMMAND_FEED", "0");
    std::env::set_var("CRANPOSE_SIMILARITY_REPLAY", "0");
    let _ = render_sequence(&mut renderer, command, &graphs[last..], &recordings[last..]);
    let control = render_sequence(&mut renderer, command, &graphs[last..], &recordings[last..]);
    // Missing rings put many channels far beyond blending noise (≤2/255).
    let missing_differs = channels_differing_over(&control[0], &missing_frame.pixels, 60);
    assert!(
        missing_differs > 10_000,
        "the miss frame should visibly lack the bypassed rings \
         ({missing_differs} channels differ by more than 60)"
    );
    let (healed_graphs, healed_recordings) = build_sequence(33, &mut |slot| {
        cranpose_render_common::scene_builder::retained_slot_confirmed(command, slot)
    });
    assert_eq!(
        run_primitives(&healed_graphs),
        run_primitives(&graphs),
        "with every confirmation revoked the next build must materialize fully"
    );
    let healed = render_sequence(
        &mut renderer,
        command,
        &healed_graphs[last..],
        &healed_recordings[last..],
    );
    std::env::remove_var("CRANPOSE_COMMAND_FEED");
    std::env::remove_var("CRANPOSE_SIMILARITY_REPLAY");
    std::env::remove_var("CRANPOSE_ARC_MESH");
    assert_byte_exact("self-heal-vs-control", &control, &healed);
}

/// T5: a feed capture that outlives the frame it was queued on (aborted
/// collection) must be dropped at the drain — never captured against
/// another frame's shapes, never confirmed. (That `retire_all` also clears
/// the queue is unit-tested in `shape_replay`.)
#[test]
fn stale_feed_captures_are_dropped_at_the_drain() {
    let mut renderer = match support::headless_renderer() {
        Ok(renderer) => renderer,
        Err(err) => {
            eprintln!("skipping fail-closed stale capture: headless WGPU init failed: {err}");
            return;
        }
    };
    // The flat detector stays out so the injected capture is the only
    // replay-adjacent state in play.
    std::env::set_var("CRANPOSE_SIMILARITY_REPLAY", "0");
    let command = command_for(34);

    // A plain (non-replayed) scene with far more shapes than the injected
    // capture asks for: without the frame stamp, the drain WOULD capture
    // shapes 0..64 of the wrong frame and confirm the slot.
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
    std::env::remove_var("CRANPOSE_SIMILARITY_REPLAY");

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
