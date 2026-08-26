//! Pixel parity for cached retained render bundles.
//!
//! Renders the same churning retained scene as `command_feed_parity` with
//! `CRANPOSE_RETAINED_BUNDLES` off (direct per-op encoding) and on (cached
//! bundles) and requires ZERO differing bytes. Unlike the arc-mesh change
//! there is no rasterization difference to budget for: a bundle replays the
//! IDENTICAL command sequence through the identical pipelines, so any
//! divergence at all is a real defect (stale bundle, wrong dynamic offset,
//! scissor drift, dropped op).
//!
//! Same-position-control discipline (documented in `command_feed_parity`):
//! the flat detector renders pre-retention frames differently before any
//! feed slot lives in the renderer, so a first pass must never be compared
//! against a later one. Pass 1 warms every slot; the compared renders are
//! passes 2/3/4, whose renderer state is stable — proven here by asserting
//! the two OFF controls byte-equal each other before comparing the ON pass.
//!
//! The scene churns (recoloring twinkles, movers whose count changes every
//! frame, recapturing spark spans), so the ON passes also exercise the
//! rebuild path: the per-frame rebuild counters must keep moving after the
//! initial build (churn safety), while cached executes dominate rebuilds
//! (the cache is not vacuously rebuilding every frame).
//!
//! Since P1b the bundles must also cover the two new draw encodings, so
//! every pass drives `CRANPOSE_ARC_MESH` per position: ON for the frames
//! before the jolt (captures and revert-churn recaptures land with indexed
//! meshes — `draw_indexed` over the slot's u32 index buffer inside the
//! bundle) and OFF from the jolt on (its recaptures land meshless and ride
//! the default-ON instanced-quad path — `draw_indexed(0..6, 0,
//! first..last)` over the static u16 quad indices). Recaptures are
//! per-position deterministic, so every pass sees the identical encoding
//! mixture at the same frame and the same-position control holds; the
//! per-frame slot stats assert the ON pass really bundled BOTH encodings
//! at once. The zero-byte bar is unchanged: a bundle replays the IDENTICAL
//! commands the direct path encodes, whatever the encoding.

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
const FRAMES: usize = 16;
/// Frame at which the ring bands jolt non-similarly (see `record_frame`):
/// the recorder cannot express the change as a similarity transform, so the
/// affected spans re-verify and RECAPTURE mid-sequence — while the previous
/// frames' bundles are still live in the cache. That is the stale-bundle
/// hazard this suite must prove impossible.
const JOLT_FRAME: usize = 8;

/// One frame of the synthetic boss through the RECORDING path: rings
/// rotating at distinct speeds under a breathing scale (each backed by a
/// full annulus big enough for the retained mesh size gate), churning
/// sparks, recoloring twinkles, movers whose count changes every frame —
/// plus a non-similar band jolt at `JOLT_FRAME` to force mid-sequence
/// recaptures.
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
        // Thickening the band while the radius breathes is NOT a similarity
        // of the captured geometry: the span must recapture.
        let band = band * breathing * if frame >= JOLT_FRAME { 1.35 } else { 1.0 };
        // A full backing annulus under each sector ring: its ~32k-90k px²
        // quad clears the retained size gate, so pre-jolt captures still
        // land WITH meshes now that the tiny sectors pass through — and it
        // thickens with the same jolt, so its span recaptures mid-sequence
        // (meshless, ARC_MESH is off from the jolt) like the rest.
        scope.draw_annular_sector(
            Brush::solid(Color(0.08, 0.12, 0.22, 1.0)),
            Point::new(CENTER, CENTER),
            radius - band,
            radius,
            speed * frame as f32,
            std::f32::consts::TAU,
        );
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
/// as the scene builder's verifier would. `node_id` keys the command
/// identity, so this test's slots stay distinct from other suites'.
fn build_sequence(node_id: usize) -> Vec<RenderGraph> {
    let mut state = CommandReplayState::default();
    let command = DrawCommandId {
        node_id,
        command_index: 0,
        placement: DrawPlacement::Behind,
    };
    (0..FRAMES)
        .map(|frame| {
            let scope = record_frame(frame);
            let outcome = state.advance(scope.recorded());
            let center = state.center();
            let (finished, replay) = scope.finish_replay(center, outcome, &mut |_| false);
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
                    replay.map(Box::new),
                ))],
            })
        })
        .collect()
}

/// Per-frame observations of one pass: pixels, bundle-cache (rebuilds,
/// cached executes) deltas, and (meshed, total) replay-slot stats.
type PassObservations = (Vec<Vec<u8>>, Vec<(u64, u64)>, Vec<(usize, usize)>);

/// Renders the sequence and returns each frame's pixels, the bundle cache's
/// per-frame (rebuilds, cached executes) deltas, and the per-frame (meshed,
/// total) replay-slot stats.
///
/// `CRANPOSE_ARC_MESH` is driven per position — ON before the jolt, OFF
/// from it — so captures land meshed while jolt recaptures land meshless.
/// Recaptures are per-position deterministic (the graphs replay
/// identically), so every pass sees the identical encoding mixture at the
/// same frame index and the same-position control stays valid.
fn render_sequence(
    renderer: &mut support::LockedRenderer,
    graphs: &[RenderGraph],
) -> PassObservations {
    let mut frames = Vec::with_capacity(graphs.len());
    let mut deltas = Vec::with_capacity(graphs.len());
    let mut mesh_stats = Vec::with_capacity(graphs.len());
    for (frame, graph) in graphs.iter().enumerate() {
        cranpose_render_wgpu::set_debug_toggle(
            "CRANPOSE_ARC_MESH",
            Some(if frame < JOLT_FRAME { "1" } else { "0" }),
        );
        let before = renderer.retained_bundle_stats();
        renderer.scene_mut().graph = Some(graph.clone());
        let captured = renderer
            .capture_frame(SIZE, SIZE)
            .unwrap_or_else(|err| panic!("frame {frame} capture failed: {err:?}"));
        assert_eq!((captured.width, captured.height), (SIZE, SIZE));
        let after = renderer.retained_bundle_stats();
        frames.push(captured.pixels);
        deltas.push((after.0 - before.0, after.1 - before.1));
        mesh_stats.push(renderer.replay_slot_mesh_stats());
    }
    (frames, deltas, mesh_stats)
}

fn assert_byte_exact(label: &str, a: &[Vec<u8>], b: &[Vec<u8>]) {
    for (frame, (a, b)) in a.iter().zip(b).enumerate() {
        assert_eq!(a.len(), b.len());
        let differing = a.iter().zip(b).filter(|(a, b)| a != b).count();
        eprintln!("{label} frame {frame}: differing {differing}");
        assert_eq!(
            differing, 0,
            "{label} frame {frame}: {differing} bytes differ — bundles must replay \
             byte-identical commands"
        );
    }
}

#[test]
fn retained_bundles_replay_byte_exact_and_rebuild_on_churn() {
    let mut renderer = match support::headless_renderer() {
        Ok(renderer) => renderer,
        Err(err) => {
            eprintln!("skipping retained bundle parity: headless WGPU init failed: {err}");
            return;
        }
    };
    cranpose_render_wgpu::set_debug_toggle("CRANPOSE_COMMAND_FEED", Some("1"));

    let graphs = build_sequence(9);

    // Pass 1 (warm): captures land, slots churn, bundles are live — its
    // pixels are never compared, but its counters prove churn safety.
    // (`render_sequence` drives CRANPOSE_ARC_MESH per position: meshed
    // captures before the jolt, meshless recaptures from it.)
    cranpose_render_wgpu::set_debug_toggle("CRANPOSE_RETAINED_BUNDLES", Some("1"));
    let (_warm, warm_deltas, _warm_mesh) = render_sequence(&mut renderer, &graphs);

    // Passes 2..4 at stable renderer positions: OFF control, ON, OFF control.
    cranpose_render_wgpu::set_debug_toggle("CRANPOSE_RETAINED_BUNDLES", Some("0"));
    let (off_frames, off_deltas, _off_mesh) = render_sequence(&mut renderer, &graphs);
    cranpose_render_wgpu::set_debug_toggle("CRANPOSE_RETAINED_BUNDLES", Some("1"));
    let (on_frames, on_deltas, on_mesh) = render_sequence(&mut renderer, &graphs);
    cranpose_render_wgpu::set_debug_toggle("CRANPOSE_RETAINED_BUNDLES", Some("0"));
    let (off2_frames, _off2_deltas, _off2_mesh) = render_sequence(&mut renderer, &graphs);

    cranpose_render_wgpu::set_debug_toggle("CRANPOSE_RETAINED_BUNDLES", None);
    cranpose_render_wgpu::set_debug_toggle("CRANPOSE_COMMAND_FEED", None);
    cranpose_render_wgpu::set_debug_toggle("CRANPOSE_ARC_MESH", None);

    eprintln!("warm deltas (rebuilds, executes): {warm_deltas:?}");
    eprintln!("on   deltas (rebuilds, executes): {on_deltas:?}");

    // Non-vacuity for the P1b encodings: within the ON pass the live slot
    // population must have held BOTH meshed slots (indexed-mesh
    // `draw_indexed` inside the executed bundles) and meshless slots
    // (instanced-quad draws on the latched default-ON selection) — the
    // per-frame stats prove the mixture existed while bundles drew.
    eprintln!("on-pass (meshed, total) slots per frame: {on_mesh:?}");
    assert!(
        renderer.instanced_quads_active(),
        "the default-ON instanced selection must have latched at construction"
    );
    assert!(
        on_mesh
            .iter()
            .any(|&(meshed, total)| meshed >= 1 && meshed < total),
        "the ON pass must have bundled a mixture of meshed and meshless \
         slots (per-frame stats {on_mesh:?})"
    );

    // The kill switch must actually kill: the OFF pass may not touch the
    // bundle path at all.
    let off_total: (u64, u64) = off_deltas
        .iter()
        .fold((0, 0), |acc, d| (acc.0 + d.0, acc.1 + d.1));
    assert_eq!(
        off_total,
        (0, 0),
        "CRANPOSE_RETAINED_BUNDLES=0 must bypass the bundle cache entirely"
    );

    // Same-position control: two OFF passes at stable positions must agree
    // byte-for-byte, or comparing the ON pass against either is meaningless.
    assert_byte_exact("off-vs-off2", &off_frames, &off2_frames);
    // The parity bar itself: ZERO differing bytes, every frame, including
    // the capture/recapture/split frames this churning scene produces.
    assert_byte_exact("on-vs-off", &on_frames, &off_frames);

    // Non-vacuity: the ON pass really drew through bundles...
    let on_rebuilds: u64 = on_deltas.iter().map(|d| d.0).sum();
    let on_executes: u64 = on_deltas.iter().map(|d| d.1).sum();
    assert!(
        on_rebuilds > 0,
        "the ON pass must have built bundles (cache was evicted during the OFF pass)"
    );
    // ...reusing them across frames rather than rebuilding every stretch
    // every frame.
    assert!(
        on_executes > on_rebuilds,
        "cached executes ({on_executes}) should exceed rebuilds ({on_rebuilds}) — \
         a cache that rebuilds every frame is vacuous"
    );

    // Churn safety: while slots recapture (warm pass frames after the first
    // retained frame), the rebuild counter must keep moving — a recaptured
    // slot may NEVER draw through its stale bundle, and the epoch key change
    // shows up as a rebuild.
    let first_retained = warm_deltas
        .iter()
        .position(|d| d.0 + d.1 > 0)
        .expect("the warm pass must reach retained frames");
    let late_rebuilds: u64 = warm_deltas[first_retained + 1..].iter().map(|d| d.0).sum();
    assert!(
        late_rebuilds > 0,
        "slot churn after frame {first_retained} must have invalidated bundles \
         (deltas {warm_deltas:?})"
    );
}
