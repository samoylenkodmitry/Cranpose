//! Pixel parity for the retained-segment surface cache.
//!
//! Both arms drive the REAL recorder machinery exactly as
//! `command_feed_parity` does — commands record into a `DrawScopeDefault`,
//! a `CommandReplayState` verifies each frame, the resulting replay frame
//! rides the graph — and both arms render with the command feed ON. The
//! baseline arm is the retained instanced-quad path the cache REPLACES
//! (`CRANPOSE_SEGMENT_SURFACE=0` — the compiled default is ON); the cached arm opts in. The delta
//! between the arms is therefore exactly what the surface cache adds:
//! nothing on identity frames beyond the 8-bit premultiplied intermediate,
//! bounded resampling under rotation.
//!
//! The z-interleave bar rides the rotating scene: the spark span draws
//! BETWEEN two cached ring segments in z, its footprint overlapping both
//! rings' bands, with translucent color — any blending reorder would blow
//! the per-pixel bound by an order of magnitude.

mod support;

use cranpose_render_common::graph::{
    CachePolicy, DrawCommandId, DrawRunNode, IsolationReasons, LayerNode, PrimitivePhase,
    ProjectiveTransform, RenderGraph, RenderNode,
};
use cranpose_render_common::raster_cache::LayerRasterCacheHashes;
use cranpose_render_common::style_shared::DrawPlacement;
use cranpose_render_common::Renderer;
use cranpose_ui_graphics::{
    Brush, Color, CommandReplayState, DrawScope, DrawScopeDefault, GraphicsLayer, Point, Rect,
};

const SIZE: u32 = 408;
const CENTER: f32 = 204.0;

/// One frame of the synthetic boss. `rotate` spins the rings at distinct
/// speeds (the similarity-motion case); `false` freezes every segment at
/// identity. Twinkles recolor per frame when `twinkle_churn`, else they
/// hold one palette until `recolor_at` and flip once (the
/// invalidate-within-one-frame case).
fn record_frame(
    frame: usize,
    rotate: bool,
    breathe: bool,
    twinkle_churn: bool,
    recolor_at: Option<usize>,
) -> DrawScopeDefault {
    let mut scope =
        DrawScopeDefault::new(cranpose_ui_graphics::Size::new(SIZE as f32, SIZE as f32));
    scope.draw_rect_at(
        Rect {
            x: 0.0,
            y: 0.0,
            width: SIZE as f32,
            height: SIZE as f32,
        },
        Brush::solid(Color(0.02, 0.02, 0.05, 1.0)),
    );
    // Leading dynamic span: movers whose count changes every frame.
    for m in 0..(2 + frame % 3) {
        let x = 30.0 + frame as f32 * 7.0 + m as f32 * 15.0;
        scope.draw_circle(
            Brush::solid(Color(1.0, 1.0, 1.0, 1.0)),
            Point::new(x + 4.0, 44.0 + m as f32 * 12.0),
            4.0,
        );
    }
    let breathing = if breathe {
        1.0 - 0.001 * frame as f32
    } else {
        1.0
    };
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
        let rotation = if rotate { speed * frame as f32 } else { 0.0 };
        for i in 0..count {
            let start = i as f32 * (std::f32::consts::TAU / count as f32) + rotation;
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
            // The interleaved dynamic span: churning translucent sparks
            // whose orbits (60..150) overlap BOTH the 90 and the 120 ring
            // bands, drawn between them in z.
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
        let alpha = if twinkle_churn {
            0.25 + 0.7 * (((d + frame * 3) % 11) as f32 / 10.0)
        } else if recolor_at.is_some_and(|at| frame >= at) {
            0.85
        } else {
            0.35
        };
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

fn build_sequence(
    frames: usize,
    rotate: bool,
    breathe: bool,
    twinkle_churn: bool,
    recolor_at: Option<usize>,
) -> Vec<RenderGraph> {
    let mut state = CommandReplayState::default();
    let command = DrawCommandId {
        node_id: 7,
        command_index: 0,
        placement: DrawPlacement::Behind,
    };
    (0..frames)
        .map(|frame| {
            let scope = record_frame(frame, rotate, breathe, twinkle_churn, recolor_at);
            let outcome = state.advance(scope.recorded());
            let center = state.center();
            let (finished, replay) = scope.finish_replay(center, outcome, &mut |_| false);
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
            captured.pixels
        })
        .collect()
}

struct FrameDelta {
    /// Channels differing at all.
    differing: usize,
    worst: u8,
    /// Differing pixels outside the segments' dilated footprint (the
    /// center-annulus that covers every ring band and the twinkle orbit).
    outside_footprint: usize,
}

/// The segments all live in center-annuli; a diff pixel outside every
/// dilated annulus cannot be resampling and means the composite leaked.
fn frame_delta(a: &[u8], b: &[u8]) -> FrameDelta {
    const FOOTPRINT_PAD: f32 = 4.0;
    let mut delta = FrameDelta {
        differing: 0,
        worst: 0,
        outside_footprint: 0,
    };
    for (i, (a, b)) in a.iter().zip(b).enumerate() {
        let diff = a.abs_diff(*b);
        if diff == 0 {
            continue;
        }
        delta.differing += 1;
        delta.worst = delta.worst.max(diff);
        let pixel = (i / 4) as u32;
        let (x, y) = ((pixel % SIZE) as f32, (pixel / SIZE) as f32);
        let r = ((x - CENTER).powi(2) + (y - CENTER).powi(2)).sqrt();
        let in_ring =
            |inner: f32, outer: f32| r >= inner - FOOTPRINT_PAD && r <= outer + FOOTPRINT_PAD;
        // Ring bands 140..150, 111..120, 82..90; twinkle orbit 52..76.
        let inside = in_ring(140.0, 150.0)
            || in_ring(111.0, 120.0)
            || in_ring(82.0, 90.0)
            || in_ring(52.0, 76.0);
        if !inside {
            delta.outside_footprint += 1;
        }
    }
    delta
}

fn set_common_env() {
    // The feed ON in both arms: the baseline is the retained instanced-quad
    // path the surface cache replaces, so the delta isolates the cache.
    // Arc meshes off for the same reason command_feed_parity keeps them
    // off: their interpolation envelope is measured in arc_mesh_parity.
    std::env::set_var("CRANPOSE_SIMILARITY_REPLAY", "1");
    std::env::set_var("CRANPOSE_COMMAND_FEED", "1");
    std::env::set_var("CRANPOSE_ARC_MESH", "0");
    // The synthetic bricks are much sparser than the mega scene's (1.8 px
    // chords in 10 px bands), so the r=150 ring prices at ~3.9x its member
    // pixels and the DEFAULT ratio (3.0) correctly rejects it. These tests
    // pin CORRECTNESS of the composite path, not the default economics —
    // the module unit tests pin the gate — so they widen the ratio to
    // admit all three rings.
    std::env::set_var("CRANPOSE_SEGMENT_SURFACE_COST_RATIO", "6.0");
}

fn clear_env() {
    std::env::remove_var("CRANPOSE_SIMILARITY_REPLAY");
    std::env::remove_var("CRANPOSE_COMMAND_FEED");
    std::env::remove_var("CRANPOSE_ARC_MESH");
    std::env::set_var("CRANPOSE_SEGMENT_SURFACE", "0");
    std::env::remove_var("CRANPOSE_SEGMENT_SURFACE_COST_RATIO");
}

/// Identity transforms end-to-end: static rings, one twinkle palette flip.
/// Bar (MEASURED): the composite frames are byte-identical to the retained
/// path outside the segments' footprint, and inside it deviate by at most
/// 2 levels. The ±2 (not ±1) is the 8-bit premultiplied intermediate — the
/// same quantization class Compose's own layer surfaces carry — rounding
/// once per member blended at a pixel: this scene's bricks sit 0.45 px
/// apart at r=150, so adjacent AA fringes overlap and two members land on
/// one texel. Frames before engagement measure 0 differing channels, and
/// the flip frame must recapture WITHIN that frame.
#[test]
fn identity_segments_composite_within_one_level_and_recolor_recaptures_same_frame() {
    let mut renderer = match support::headless_renderer() {
        Ok(renderer) => renderer,
        Err(err) => {
            eprintln!("skipping segment surface parity: headless WGPU init failed: {err}");
            return;
        }
    };
    const FRAMES: usize = 24;
    const RECOLOR_AT: usize = 16;
    set_common_env();
    let graphs = build_sequence(FRAMES, false, false, false, Some(RECOLOR_AT));
    // First run warms the flat detector's pass-position state; the CONTROL
    // is the second run, exactly as command_feed_parity compares bypassed
    // frames against `fed2`, not the first fed run — the detector renders
    // the two pre-retention frames up to 2/255 differently between a cold
    // and a warmed renderer, which has nothing to do with this cache.
    let _warmup = render_sequence(&mut renderer, &graphs);
    let baseline = render_sequence(&mut renderer, &graphs);
    let stats_before = renderer.segment_surface_stats();
    assert_eq!(
        stats_before,
        (0, 0, 0, 0, 0),
        "the cache must stay silent while the kill switch is off"
    );

    std::env::set_var("CRANPOSE_SEGMENT_SURFACE", "1");
    let cached = render_sequence(&mut renderer, &graphs);
    let (captures, composites, dirty_recaptures, rejected_churn, _) =
        renderer.segment_surface_stats();
    clear_env();
    eprintln!(
        "identity: captures {captures} composites {composites} \
         dirty_recaptures {dirty_recaptures} rejected_churn {rejected_churn}"
    );
    assert!(
        captures >= 2,
        "the static segments should have captured surfaces, got {captures}"
    );
    assert!(
        composites >= 20,
        "the composite path should have served the warm frames, got {composites}"
    );
    assert!(
        dirty_recaptures >= 1,
        "the palette flip must recapture through the dirty path"
    );

    for (frame, (baseline, cached)) in baseline.iter().zip(&cached).enumerate() {
        let delta = frame_delta(baseline, cached);
        eprintln!(
            "identity frame {frame}: differing {} worst {} outside {}",
            delta.differing, delta.worst, delta.outside_footprint
        );
        assert!(
            delta.worst <= 2,
            "frame {frame}: identity composite deviated by {} levels — the \
             8-bit premultiplied intermediate rounds at most once per \
             overlapping member (two on this scene's sub-pixel brick gaps)",
            delta.worst
        );
        assert_eq!(
            delta.outside_footprint, 0,
            "frame {frame}: composite ink leaked outside the segment footprint"
        );
    }
}

/// Rings rotating at distinct speeds, translucent sparks interleaved
/// between two cached rings in z with overlapping footprint, twinkles
/// recoloring EVERY frame. Bars: churn is rejected (twinkles never
/// capture), rotation resampling stays inside a measured envelope within
/// the dilated footprint, and the interleaved blending survives — a
/// z-reorder under the 0.8-alpha sparks would deviate by far more than the
/// resampling bound.
#[test]
fn rotating_segments_stay_inside_the_resampling_envelope_and_churn_is_rejected() {
    let mut renderer = match support::headless_renderer() {
        Ok(renderer) => renderer,
        Err(err) => {
            eprintln!("skipping segment surface parity: headless WGPU init failed: {err}");
            return;
        }
    };
    const FRAMES: usize = 24;
    set_common_env();
    let graphs = build_sequence(FRAMES, true, false, true, None);
    // Warmed control — see the identity test.
    let _warmup = render_sequence(&mut renderer, &graphs);
    let baseline = render_sequence(&mut renderer, &graphs);

    std::env::set_var("CRANPOSE_SEGMENT_SURFACE", "1");
    let cached = render_sequence(&mut renderer, &graphs);
    let (captures, composites, _, rejected_churn, rejected_economics) =
        renderer.segment_surface_stats();
    clear_env();
    eprintln!(
        "rotation: captures {captures} composites {composites} \
         rejected_churn {rejected_churn} rejected_economics {rejected_economics}"
    );
    assert!(
        captures >= 3,
        "the three rings should have captured surfaces, got {captures}"
    );
    assert!(
        rejected_churn > 0,
        "the per-frame twinkle recolors must be churn-rejected"
    );
    assert!(
        composites >= 30,
        "the rings should composite across the warm frames, got {composites}"
    );

    let mut worst = 0u8;
    let mut worst_differing = 0usize;
    for (frame, (baseline, cached)) in baseline.iter().zip(&cached).enumerate() {
        let delta = frame_delta(baseline, cached);
        eprintln!(
            "rotation frame {frame}: differing {} worst {} outside {}",
            delta.differing, delta.worst, delta.outside_footprint
        );
        assert_eq!(
            delta.outside_footprint, 0,
            "frame {frame}: composite ink leaked outside the segment footprint"
        );
        worst = worst.max(delta.worst);
        worst_differing = worst_differing.max(delta.differing);
    }
    // MEASURED envelope (lavapipe, this scene, 24 frames), asserted at a
    // small multiple: bilinear resampling of the rotated ring bands against
    // analytic redraw. A z-order defect under the interleaved sparks blows
    // these by an order of magnitude.
    assert!(
        worst <= SEGMENT_ROTATION_WORST_BOUND,
        "rotation resampling worst delta {worst} exceeded the measured envelope"
    );
    assert!(
        worst_differing <= SEGMENT_ROTATION_DIFFERING_BOUND,
        "rotation resampling differing count {worst_differing} exceeded the measured envelope"
    );
}

/// Measured on lavapipe (24 frames: worst 132, differing 71,280 channels
/// of 666k) and asserted at a 1.5x margin; see the test.
const SEGMENT_ROTATION_WORST_BOUND: u8 = 200;
const SEGMENT_ROTATION_DIFFERING_BOUND: usize = 110_000;

/// Breathing scale: the span's scale drifts from its captured scale until
/// the threshold recaptures at the new scale.
#[test]
fn scale_drift_beyond_the_threshold_recaptures() {
    let mut renderer = match support::headless_renderer() {
        Ok(renderer) => renderer,
        Err(err) => {
            eprintln!("skipping segment surface parity: headless WGPU init failed: {err}");
            return;
        }
    };
    const FRAMES: usize = 48;
    set_common_env();
    // A tight drift threshold makes the recapture land well inside the
    // breathing run: the rings shrink 0.1% per frame, so from a capture at
    // scale s0 the drift crosses 0.004 roughly every four frames.
    std::env::set_var("CRANPOSE_SEGMENT_SURFACE_SCALE_EPS", "0.004");
    std::env::set_var("CRANPOSE_SEGMENT_SURFACE", "1");
    let graphs = build_sequence(FRAMES, false, true, false, None);
    let _ = render_sequence(&mut renderer, &graphs);
    let stats = renderer.segment_surface_stats();
    std::env::remove_var("CRANPOSE_SEGMENT_SURFACE_SCALE_EPS");
    clear_env();
    eprintln!("scale drift (captures, composites, dirty, churn, economics): {stats:?}");
    let (captures, composites, _, _, _) = stats;
    assert!(
        captures >= 4,
        "the breathing rings must recapture as their scale drifts past the \
         threshold, got {captures} captures"
    );
    assert!(
        composites > captures,
        "the rings should composite between recaptures, got {composites} \
         composites over {captures} captures"
    );
}
