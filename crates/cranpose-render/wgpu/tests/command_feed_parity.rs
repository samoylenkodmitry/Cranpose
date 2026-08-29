//! Pixel parity for the identity-fed retained path.
//!
//! Drives the REAL recorder machinery — commands record into a
//! `DrawScopeDefault`, a `CommandReplayState` verifies each frame, and the
//! resulting `CommandReplayFrame` rides the graph — then renders the same
//! frame sequence twice: once with the feed disabled (every primitive through
//! the full pipeline) and once with `CRANPOSE_COMMAND_FEED=1` (retained spans
//! drawn from identity-keyed slots). Frames must match pixel-for-pixel within
//! blending noise, and the fed run must actually have retained — a silently
//! fallen-back feed would pass parity vacuously.

mod support;

use cranpose_render_common::{
    Renderer,
    graph::{
        CachePolicy, DrawCommandId, DrawRunNode, IsolationReasons, LayerNode, PrimitivePhase,
        ProjectiveTransform, RenderGraph, RenderNode,
    },
    raster_cache::LayerRasterCacheHashes,
    style_shared::DrawPlacement,
};
use cranpose_ui_graphics::{
    Brush, Color, CommandReplayState, DrawScope, DrawScopeDefault, GraphicsLayer, Point, Rect,
};

const SIZE: u32 = 408;
const CENTER: f32 = 204.0;
/// Enough frames for the twinkle alpha cycle (period 11, `record_frame`'s
/// `% 11`) to wrap past the frame-1 capture: at frame 12 every twinkle
/// returns EXACTLY to its captured color, which must reach the GPU as an
/// explicit restore patch — a recorder that emits recolors only as
/// diff-vs-snapshot emits nothing there, the slot's paint mirror keeps
/// frame 11's colors, and the whole twinkle field renders stale. Eight
/// frames never wrapped the cycle, which is how that defect hid.
const FRAMES: usize = 13;

/// One frame of the synthetic boss through the RECORDING path: rings
/// rotating at distinct speeds under a breathing scale, churning sparks,
/// recoloring twinkles, movers whose count changes every frame.
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

/// Records every frame once through one live `CommandReplayState`, exactly
/// as the scene builder's verifier would, producing the graphs both render
/// runs share. `bypass` decides per retained slot whether its records skip
/// materialization — the production path consults renderer confirmations.
fn build_sequence(bypass: &mut dyn FnMut(u32) -> bool) -> Vec<RenderGraph> {
    let mut state = CommandReplayState::default();
    let command = DrawCommandId {
        node_id: 7,
        command_index: 0,
        placement: DrawPlacement::Behind,
    };
    (0..FRAMES)
        .map(|frame| {
            let scope = record_frame(frame);
            let outcome = state.advance(scope.recorded());
            let center = state.center();
            let (finished, replay) = scope.finish_replay(center, outcome, bypass);
            // Each frame owns the recording it was built from, exactly as
            // production attaches the published handle: a bypassed span's
            // only rematerialization source rides WITH the frame.
            let fallback = std::rc::Rc::new(finished.recording);
            let replay = replay.map(|mut frame| {
                frame.fallback = Some(fallback);
                Box::new(frame)
            });
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
                has_origin_sinks: false,
                isolation: IsolationReasons::default(),
                cache_policy: CachePolicy::None,
                cache_hashes: LayerRasterCacheHashes::default(),
                cache_hashes_valid: false,
                children: vec![RenderNode::DrawRun(DrawRunNode::for_command_replayed(
                    PrimitivePhase::BeforeChildren,
                    Some(command),
                    std::rc::Rc::new(finished.primitives),
                    replay,
                ))],
            })
        })
        .collect()
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

#[test]
fn command_feed_matches_the_full_pipeline_pixel_for_pixel() {
    let mut renderer = match support::headless_renderer() {
        Ok(renderer) => renderer,
        Err(err) => {
            eprintln!("skipping command feed parity: headless WGPU init failed: {err}");
            return;
        }
    };
    let graphs = build_sequence(&mut |_| false);
    let retained_frames: usize = graphs
        .iter()
        .filter(|graph| match &graph.root.children[0] {
            RenderNode::DrawRun(run) => run.replay.is_some(),
            _ => false,
        })
        .count();
    assert!(
        retained_frames >= FRAMES - 2,
        "the recorder should retain from the partition frame on, got {retained_frames}"
    );

    // Baseline: everything through the full pipeline — no feed, no flat
    // detector (its retention is not what this test compares against), and
    // no retained arc meshes: this test documents the FEED's divergence
    // envelope, so its captures stay on the plain quad expansion (the mesh
    // adds its own interpolation noise, measured in arc_mesh_parity).
    cranpose_render_wgpu::set_debug_toggle("CRANPOSE_ARC_MESH", Some("0"));
    // The feed has defaulted ON since its parity was proven, so the
    // baseline must say "0" explicitly: with the variable merely unset the
    // baseline renders through the very retention path under test, and any
    // stale-slot defect cancels out of the comparison — which is exactly
    // how the mod-11 stale-recolor defect stayed invisible here.
    cranpose_render_wgpu::set_debug_toggle("CRANPOSE_COMMAND_FEED", Some("0"));
    let baseline = render_sequence(&mut renderer, &graphs);

    cranpose_render_wgpu::set_debug_toggle("CRANPOSE_COMMAND_FEED", Some("1"));
    let fed = render_sequence(&mut renderer, &graphs);

    // The fed pass confirmed captured slots exactly as production does;
    // rebuild the same deterministic sequence with the bypass consulting
    // those confirmations, so retained records never materialize at all,
    // and render it against the live slots. Confirmations are live only
    // under the feed generation they were stamped with, so declare it for
    // this build exactly as the production pipeline does per build.
    cranpose_render_common::scene_builder::set_retained_feed_epoch(Some(
        cranpose_render_wgpu::retained_feed_generation(),
    ));
    let command = DrawCommandId {
        node_id: 7,
        command_index: 0,
        placement: DrawPlacement::Behind,
    };
    let bypassed_graphs = build_sequence(&mut |slot| {
        cranpose_render_common::scene_builder::retained_slot_confirmed(command, slot)
    });
    let bypassed_primitives: usize = bypassed_graphs
        .iter()
        .map(|graph| match &graph.root.children[0] {
            RenderNode::DrawRun(run) => run.primitives.len(),
            _ => 0,
        })
        .sum();
    let full_primitives: usize = graphs
        .iter()
        .map(|graph| match &graph.root.children[0] {
            RenderNode::DrawRun(run) => run.primitives.len(),
            _ => 0,
        })
        .sum();
    eprintln!("primitives materialized: full {full_primitives} bypassed {bypassed_primitives}");
    assert!(
        bypassed_primitives < full_primitives / 2,
        "confirmed slots should have bypassed materialization \
         ({bypassed_primitives} of {full_primitives} still materialized)"
    );
    // Isolation control: the same graphs the fed pass drew, rendered again
    // from the current renderer state. Any drift here is pass-position
    // state (the flat detector's snapshot/capture schedule), not bypass.
    let fed2 = render_sequence(&mut renderer, &graphs);
    for (frame, (a, b)) in fed.iter().zip(&fed2).enumerate() {
        let differing = a.iter().zip(b).filter(|(a, b)| a.abs_diff(**b) > 0).count();
        eprintln!("fed-again frame {frame}: vs fed differing {differing}");
    }
    let bypassed = render_sequence(&mut renderer, &bypassed_graphs);
    cranpose_render_wgpu::set_debug_toggle("CRANPOSE_COMMAND_FEED", None);
    cranpose_render_wgpu::set_debug_toggle("CRANPOSE_ARC_MESH", None);

    let (feed_slots, patches, remat_misses) = cranpose_render_wgpu::command_feed_live_stats();
    assert_eq!(
        remat_misses, 0,
        "no bypassed span may have hit the fail-closed remat-miss terminal"
    );
    assert!(
        feed_slots >= 4,
        "the rings and twinkles should hold identity-fed slots, got {feed_slots}"
    );
    assert!(
        patches > 0,
        "twinkle recolors should have gone through the patch path"
    );

    for (frame, (baseline, fed)) in baseline.iter().zip(&fed).enumerate() {
        assert_eq!(baseline.len(), fed.len());
        let mut worst = 0u8;
        let mut differing = 0usize;
        let (mut min_x, mut min_y, mut max_x, mut max_y) = (u32::MAX, u32::MAX, 0u32, 0u32);
        for (i, (a, b)) in baseline.iter().zip(fed).enumerate() {
            let diff = a.abs_diff(*b);
            worst = worst.max(diff);
            if diff > 3 {
                differing += 1;
                let pixel = (i / 4) as u32;
                let (x, y) = (pixel % SIZE, pixel / SIZE);
                min_x = min_x.min(x);
                min_y = min_y.min(y);
                max_x = max_x.max(x);
                max_y = max_y.max(y);
            }
        }
        eprintln!(
            "frame {frame}: differing {differing} worst {worst} \
             region x {min_x}..{max_x} y {min_y}..{max_y}"
        );
        if frame < 2 {
            assert_eq!(
                differing, 0,
                "frame {frame}: dynamic and capture frames are byte-exact"
            );
        } else {
            // Retained frames deviate only at shape edges: a transformed
            // capture quad crops the AA falloff slightly differently than a
            // freshly computed tight quad. This is the envelope the flat
            // detector has always shipped — the feed measures IDENTICAL
            // per-frame counts on this scene (260..1815 channels of 666k
            // across the 13 frames, growing with accumulated rotation until
            // a recapture resets it). Wider divergence means a real defect:
            // the shifted self-similar anchor pairing this test once
            // caught, or the stale twinkle recolors at the frame-12 alpha
            // wrap (11398 channels) that the restore patches now reset.
            assert!(
                differing < 2500 && worst < 160,
                "frame {frame}: {differing} channels diverged (worst {worst}) — beyond the\n                 flat detector's edge-AA envelope"
            );
        }
    }

    // Bypassing changes what the CPU materializes, never what the GPU
    // draws: the bypassed sequence must reproduce the control sequence
    // byte-for-byte. The control is `fed2`, not `fed` — the flat detector
    // renders the two pre-retention frames ≤2/255 differently once feed
    // slots live in the renderer (a pass-position artifact of the detector
    // that predates the feed and dies with it), and both `fed2` and the
    // bypassed pass render from that same state.
    let mut deviated = false;
    for (frame, (control, bypassed)) in fed2.iter().zip(&bypassed).enumerate() {
        let differing = control
            .iter()
            .zip(bypassed)
            .filter(|(a, b)| a.abs_diff(**b) > 0)
            .count();
        eprintln!("bypassed frame {frame}: vs control differing {differing}");
        deviated |= differing != 0;
    }
    assert!(
        !deviated,
        "bypassed render deviates from the control render"
    );
}
