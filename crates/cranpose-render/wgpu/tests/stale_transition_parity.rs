//! The stale-transition serve, end to end through the production seam.
//!
//! A mid-sequence content flip collapses a command's replay verification,
//! and the collapse frame otherwise re-materializes and re-encodes the
//! whole tape at once — the 22-75 ms transition stall on a watch-class
//! core. With `CRANPOSE_STALE_TRANSITION` set, the collapse frame instead
//! re-emits the PREVIOUS build's emission, byte-for-byte, and only that
//! one frame; everything else must render exactly as the flag-off build
//! does, and re-convergence must proceed on the frames after the flip.
//!
//! Sequences are built through `draw_command_nodes_for_tests` — the real
//! `draw_nodes` path (acquire, record, verify, publish, save/serve) —
//! so the graphs carry exactly what production would ship, then rendered
//! on the same headless harness as `command_feed_parity`.
//!
//! ENV HYGIENE: this file deliberately holds a single test. It toggles
//! process-global `CRANPOSE_*` variables between runs (the flag is read
//! fresh per build, like `CRANPOSE_COMMAND_FEED`, precisely so one
//! process can A/B it), and `cargo test` threads share the environment —
//! a second test in this binary could race the toggles. Integration test
//! binaries are separate processes, so other test files are unaffected.

mod support;

use std::rc::Rc;

use cranpose_render_common::graph::{
    CachePolicy, DrawRunNode, IsolationReasons, LayerNode, PrimitivePhase, ProjectiveTransform,
    RenderGraph, RenderNode,
};
use cranpose_render_common::raster_cache::LayerRasterCacheHashes;
use cranpose_render_common::scene_builder::{
    draw_command_nodes_for_tests, set_retained_feed_epoch,
};
use cranpose_render_common::style_shared::DrawPlacement;
use cranpose_render_common::Renderer;
use cranpose_ui::DrawCommand;
use cranpose_ui_graphics::{
    Brush, Color, DrawScope, DrawScopeDefault, FrameSpan, GraphicsLayer, Point, Rect, Size,
};

const SIZE: u32 = 408;
const CENTER: f32 = 204.0;
const FRAMES: usize = 13;
/// The content flip: every generator constant changes, so nothing
/// survives verification and the frame collapses to `AllDynamic`.
const FLIP: usize = 8;

/// One frame of ring-plus-twinkle content. `variant` selects a disjoint
/// constant set — radii, bands, sweeps, counts, twinkle orbits — so a
/// capture of one variant dies whole against another, while frames within
/// a variant move by per-ring rotations and per-frame recolors exactly
/// like the game scene the recorder was built for.
fn record_content(scope: &mut DrawScopeDefault, frame: usize, variant: usize) {
    scope.draw_rect_at(
        Rect {
            x: 0.0,
            y: 0.0,
            width: SIZE as f32,
            height: SIZE as f32,
        },
        Brush::solid(Color(0.02, 0.02, 0.05, 1.0)),
    );
    let rings: &[(f32, f32, f32, usize)] = match variant {
        0 => &[
            (150.0, 10.0, 0.013, 420),
            (120.0, 9.0, -0.008, 420),
            (90.0, 8.0, 0.019, 420),
        ],
        1 => &[
            (165.0, 12.0, -0.011, 350),
            (135.0, 7.0, 0.017, 350),
            (105.0, 9.0, -0.013, 350),
            (75.0, 6.0, 0.021, 350),
        ],
        _ => &[(140.0, 14.0, 0.009, 520), (80.0, 11.0, -0.018, 520)],
    };
    for &(radius, band, speed, count) in rings {
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
    }
    let (orbit_base, angle_step) = match variant {
        0 => (55.0f32, 0.285f32),
        1 => (49.0, 0.301),
        _ => (63.0, 0.267),
    };
    for d in 0..220 {
        let angle = d as f32 * angle_step;
        let orbit = orbit_base + (d % 7) as f32 * 3.0;
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
}

/// Builds one sequence through the production `draw_nodes` seam. Each
/// frame is one build: the recording generation advances, the command
/// re-records under its stable identity, verification runs, and the
/// stale-transition save/serve engages exactly as it would in an app.
/// `flips` lists the frames at which the content variant advances.
fn build_graphs(node_id: usize, flips: &[usize]) -> Vec<RenderGraph> {
    (0..FRAMES)
        .map(|frame| {
            let variant = flips.iter().filter(|&&flip| frame >= flip).count();
            let command = DrawCommand::Behind(Rc::new(move |scope: &mut DrawScopeDefault| {
                record_content(scope, frame, variant);
            }));
            let children = draw_command_nodes_for_tests(
                node_id,
                std::slice::from_ref(&command),
                DrawPlacement::Behind,
                Size::new(SIZE as f32, SIZE as f32),
                PrimitivePhase::BeforeChildren,
            );
            assert!(
                !children.is_empty(),
                "frame {frame}: the command must emit a node"
            );
            let bounds = Rect {
                x: 0.0,
                y: 0.0,
                width: SIZE as f32,
                height: SIZE as f32,
            };
            RenderGraph::new(LayerNode {
                node_id: None,
                local_bounds: bounds,
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
        })
        .collect()
}

fn run_of(graph: &RenderGraph) -> &DrawRunNode {
    match &graph.root.children[0] {
        RenderNode::DrawRun(run) => run,
        _ => panic!("expected a draw run"),
    }
}

/// What one frame's render moved in the renderer's lifetime feed stats:
/// live slot count, recolor patches, remat misses.
struct FeedStatsDelta {
    slots: i64,
    patches: i64,
    remat: i64,
}

/// Renders the sequence, returning per-frame pixels and the feed-stats
/// delta each frame's render produced.
fn render_sequence(
    renderer: &mut support::LockedRenderer,
    graphs: &[RenderGraph],
) -> (Vec<Vec<u8>>, Vec<FeedStatsDelta>) {
    let mut frames = Vec::with_capacity(graphs.len());
    let mut deltas = Vec::with_capacity(graphs.len());
    for (frame, graph) in graphs.iter().enumerate() {
        let (slots0, patches0, remat0) = cranpose_render_wgpu::command_feed_live_stats();
        renderer.scene_mut().graph = Some(graph.clone());
        let captured = renderer
            .capture_frame(SIZE, SIZE)
            .unwrap_or_else(|err| panic!("frame {frame} capture failed: {err:?}"));
        assert_eq!((captured.width, captured.height), (SIZE, SIZE));
        let (slots1, patches1, remat1) = cranpose_render_wgpu::command_feed_live_stats();
        frames.push(captured.pixels);
        deltas.push(FeedStatsDelta {
            slots: slots1 as i64 - slots0 as i64,
            patches: patches1 as i64 - patches0 as i64,
            remat: remat1 as i64 - remat0 as i64,
        });
    }
    (frames, deltas)
}

fn differing(a: &[u8], b: &[u8]) -> usize {
    assert_eq!(a.len(), b.len());
    a.iter().zip(b).filter(|(a, b)| a.abs_diff(**b) > 0).count()
}

fn worst_diff(a: &[u8], b: &[u8]) -> u8 {
    a.iter()
        .zip(b)
        .map(|(a, b)| a.abs_diff(*b))
        .max()
        .unwrap_or(0)
}

#[test]
fn a_collapse_frame_re_serves_the_previous_emission_exactly_once() {
    // Plain quad expansion, as in command_feed_parity: this test documents
    // the serve's byte-level contract, not the arc mesh's interpolation.
    std::env::set_var("CRANPOSE_ARC_MESH", "0");
    std::env::remove_var("CRANPOSE_STALE_TRANSITION");
    let mut renderer = match support::headless_renderer() {
        Ok(renderer) => renderer,
        Err(err) => {
            eprintln!("skipping stale transition parity: headless WGPU init failed: {err}");
            return;
        }
    };

    // Feed-free baseline graphs: no declared epoch means no verification
    // at all; rendered later (warm) with CRANPOSE_COMMAND_FEED=0 so the
    // renderer stays on the full pipeline.
    std::env::set_var("CRANPOSE_COMMAND_FEED", "0");
    set_retained_feed_epoch(None);
    let baseline_graphs = build_graphs(11, &[FLIP]);

    // Everything below is the fed configuration production ships.
    std::env::set_var("CRANPOSE_COMMAND_FEED", "1");
    set_retained_feed_epoch(Some(cranpose_render_wgpu::retained_feed_generation()));

    // Flag OFF, twice: pins current behavior and the determinism floor.
    let off_graphs = build_graphs(12, &[FLIP]);
    let off2_graphs = build_graphs(13, &[FLIP]);
    let off3_graphs = build_graphs(15, &[FLIP, FLIP + 1]);

    // Flag ON: the single-flip sequence and the consecutive-collapse one.
    std::env::set_var("CRANPOSE_STALE_TRANSITION", "1");
    let on_graphs = build_graphs(14, &[FLIP]);
    let on2_graphs = build_graphs(16, &[FLIP, FLIP + 1]);
    std::env::set_var("CRANPOSE_STALE_TRANSITION", "0");

    // Scenario validity before believing any pixel number: flag off, the
    // flip frame collapsed to AllDynamic (no replay frame rides the run);
    // the frames around it verified or recaptured (replay frames present).
    assert!(
        run_of(&off_graphs[FLIP - 1]).replay.is_some(),
        "the frame before the flip must be verified"
    );
    assert!(
        run_of(&off_graphs[FLIP]).replay.is_none(),
        "the flip frame must collapse to AllDynamic under today's behavior"
    );
    assert!(
        run_of(&off_graphs[FLIP + 1]).replay.is_some(),
        "the frame after the flip must re-partition"
    );
    // Flag on, the flip frame carries the SERVED frame instead: the
    // previous build's primitives handle (aliased, not copied) and its
    // sanitized spans — no capture re-queues, no recolor patches.
    let served = run_of(&on_graphs[FLIP]);
    let served_frame = served
        .replay
        .as_ref()
        .expect("the flip frame must re-serve the previous emission");
    assert!(
        Rc::ptr_eq(&served.primitives, &run_of(&on_graphs[FLIP - 1]).primitives),
        "the served frame must alias the previous build's primitives"
    );
    assert!(!served_frame.spans.is_empty());
    for span in &served_frame.spans {
        match span {
            FrameSpan::Retained {
                capture, recolors, ..
            } => {
                assert!(!capture, "served spans must not re-queue captures");
                assert!(recolors.is_empty(), "served spans must carry no recolors");
            }
            FrameSpan::Dynamic { .. } => {}
        }
    }
    // Re-convergence in the graphs: the capture right after the flip, and
    // retained (non-capture) spans within three frames of it.
    let recaptured = run_of(&on_graphs[FLIP + 1])
        .replay
        .as_ref()
        .is_some_and(|frame| {
            frame
                .spans
                .iter()
                .any(|span| matches!(span, FrameSpan::Retained { capture: true, .. }))
        });
    assert!(recaptured, "the frame after the served flip must recapture");
    let reconverged = (FLIP + 1..=FLIP + 3).any(|frame| {
        run_of(&on_graphs[frame])
            .replay
            .as_ref()
            .is_some_and(|frame| {
                frame
                    .spans
                    .iter()
                    .any(|span| matches!(span, FrameSpan::Retained { capture: false, .. }))
            })
    });
    assert!(
        reconverged,
        "retained spans must reappear within 3 frames of the flip"
    );

    // Two discarded warmup passes: the renderer's first passes over a
    // scene render its heavy ordinary frames up to 2/255 differently from
    // every later pass — the same pass-position artifact
    // command_feed_parity documents (it compares against its `fed2`
    // control, not its first fed run; measured here, the artifact settles
    // from the third pass on). Every compared run below starts from the
    // settled state.
    let _ = render_sequence(&mut renderer, &off_graphs);
    let _ = render_sequence(&mut renderer, &off_graphs);
    std::env::set_var("CRANPOSE_COMMAND_FEED", "0");
    let (baseline, _) = render_sequence(&mut renderer, &baseline_graphs);
    std::env::set_var("CRANPOSE_COMMAND_FEED", "1");
    let (off, _) = render_sequence(&mut renderer, &off_graphs);
    let (off2, _) = render_sequence(&mut renderer, &off2_graphs);
    let (on, on_deltas) = render_sequence(&mut renderer, &on_graphs);
    let (off3, _) = render_sequence(&mut renderer, &off3_graphs);
    let (on2, _) = render_sequence(&mut renderer, &on2_graphs);
    std::env::remove_var("CRANPOSE_STALE_TRANSITION");
    std::env::remove_var("CRANPOSE_COMMAND_FEED");
    std::env::remove_var("CRANPOSE_ARC_MESH");
    set_retained_feed_epoch(None);

    for frame in 0..FRAMES {
        eprintln!(
            "frame {frame}: off-vs-off2 {} off-vs-on {} off-vs-baseline {} (worst {})",
            differing(&off[frame], &off2[frame]),
            differing(&off[frame], &on[frame]),
            differing(&off[frame], &baseline[frame]),
            worst_diff(&off[frame], &baseline[frame]),
        );
    }

    // Determinism floor: two flag-off runs of the same sequence must be
    // byte-identical, or nothing below can be read as a measurement.
    for (frame, (a, b)) in off.iter().zip(&off2).enumerate() {
        assert_eq!(
            differing(a, b),
            0,
            "frame {frame}: two flag-off runs must render identically"
        );
    }
    // The content genuinely moved at the flip — the stale assertion below
    // cannot pass vacuously.
    assert!(
        differing(&off[FLIP], &off[FLIP - 1]) > 0,
        "the flip must change the rendered frame"
    );

    // THE assertion: flag on, the flip frame is byte-identical to the
    // PREVIOUS frame's fed render — the emission was re-served, not
    // rebuilt — and genuinely differs from what a fresh emission draws.
    assert_eq!(
        differing(&on[FLIP], &on[FLIP - 1]),
        0,
        "the served flip frame must re-render the previous frame exactly"
    );
    assert!(
        differing(&on[FLIP], &off[FLIP]) > 0,
        "the served frame must differ from the fresh collapse frame"
    );
    // Every other frame is untouched by the flag: byte-identical to the
    // flag-off run, including the re-convergence frames after the flip.
    for frame in (0..FRAMES).filter(|&frame| frame != FLIP) {
        assert_eq!(
            differing(&on[frame], &off[frame]),
            0,
            "frame {frame}: the flag must only touch the collapse frame"
        );
    }
    // The serve queues nothing into the frame's ops: zero recolor patches,
    // zero new feed slots, zero remat misses — while an ordinary verified
    // frame right before it does push recolor patches.
    let serve = &on_deltas[FLIP];
    assert_eq!(serve.patches, 0, "a served frame must patch no colors");
    assert_eq!(serve.slots, 0, "a served frame must confirm no new slots");
    assert_eq!(serve.remat, 0, "a served frame must not hit the remat path");
    assert!(
        on_deltas[FLIP - 1].patches > 0,
        "the twinkle recolors must flow as patches on ordinary frames"
    );

    // Kill-switch pin: flag off, the collapse frame IS today's behavior —
    // the whole tape re-materializes through the full pipeline. Against
    // the feed-free baseline only the ≤2/255 pass-position artifact may
    // remain (command_feed_parity documents the same artifact on its
    // pre-retention frames): a frame that had regressed to serving
    // retained content instead would sit at the fed envelope, two orders
    // of magnitude above (worst ~150), and its graph would carry a replay
    // frame — asserted None above.
    assert!(
        worst_diff(&off[FLIP], &baseline[FLIP]) <= 2,
        "flag off, the collapse frame must render the full fresh pipeline \
         (worst {} channels {})",
        worst_diff(&off[FLIP], &baseline[FLIP]),
        differing(&off[FLIP], &baseline[FLIP]),
    );

    // The one-frame cap, end to end: consecutive collapses (content flips
    // at FLIP and FLIP+1) serve stale exactly once. The second collapse
    // finds the saved emission consumed and renders fresh — structurally
    // (no replay frame rides the run, where a serve always carries one)
    // and on pixels, where only the ≤2/255 pass-position artifact remains
    // (the frame BEFORE it renders differently in the two runs — served
    // vs fresh — which is exactly the state the artifact tracks).
    assert!(
        run_of(&on2_graphs[FLIP]).replay.is_some(),
        "the first of two consecutive collapses must serve"
    );
    assert!(
        run_of(&on2_graphs[FLIP + 1]).replay.is_none(),
        "the second consecutive collapse must not serve — nothing is saved"
    );
    assert_eq!(
        differing(&on2[FLIP], &on2[FLIP - 1]),
        0,
        "the first of two consecutive collapses serves stale"
    );
    assert!(
        differing(&on2[FLIP + 1], &on2[FLIP]) > 0,
        "the second consecutive collapse must NOT serve stale"
    );
    assert!(
        worst_diff(&on2[FLIP + 1], &off3[FLIP + 1]) <= 6,
        "the second collapse frame must render fresh, as flag-off does \
         (worst {} channels {})",
        worst_diff(&on2[FLIP + 1], &off3[FLIP + 1]),
        differing(&on2[FLIP + 1], &off3[FLIP + 1]),
    );
    let stale_frames = (1..FRAMES)
        .filter(|&frame| differing(&on2[frame], &on2[frame - 1]) == 0)
        .count();
    assert_eq!(
        stale_frames, 1,
        "two forced consecutive collapses must produce exactly one stale frame"
    );
}
