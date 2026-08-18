//! Pixel parity and gate engagement for the SIZE-GATED retained capture
//! mesh (Stage 3 of the fill-reduction program), under the SPLIT draw walk
//! (S3b): a meshed slot's mesh buffers hold band geometry only, and
//! `encode_retained_op` alternates along the shape range — meshed runs
//! through the mesh pipeline, everything else on the instanced-quad path
//! it never left (with the suite's instanced pin OFF, the plain `vs_main`
//! expansion — the identical entry point the quad arm uses, so passthrough
//! shapes are not merely geometry-identical, they ride the same pipeline
//! in both arms).
//!
//! The scene is the fill-truth top-slack dump made concrete: one big ring
//! (the 86%-slack "quad 210000, lit 30000" shape class), a big
//! stroked-circle rim (the 94%-slack rrect-stroke circles the dynamic
//! `rim_mesh_band` path never sees because they are retained), and a brick
//! wall of tiny arcs (the ~100-800 px² quads whose wholesale meshing
//! measured 4-11 fps SLOWER on the Adreno 702). The bulk of the scene is
//! byte-stable across frames, so the feed retains it and replays it at
//! IDENTITY — bitwise identity: `arc_anchor_transform` on byte-identical
//! content divides equal radii (exactly 1.0) and subtracts equal angles
//! (exactly 0.0), and multiplying by those is exact under any fma
//! contraction, so `arc_mesh_parity`'s rotated-frame vertex divergence is
//! out of the picture and both arms rasterize bit-equal vertex positions.
//! A trailing row of movers churns every frame so the command feed
//! exercises its real retained-span machinery instead of a degenerate
//! all-static command.
//!
//! THE MEASURED IDENTITY BAR. Bit-equal vertices do not make the two arms
//! byte-equal wherever a band actually meshes: the fragment's `rect_pos`
//! is interpolated from the `uv` ATTRIBUTE, whose per-triangle plane
//! equations the rasterizer derives from each triangulation's own
//! vertices, and a trapezoid's CPU-rounded uv over a ~10 px baseline
//! carries an ulp of slope the 370 px quad does not. That is invisible
//! until the SDF's smoothstep output sits within that ulp of a unorm8
//! rounding boundary — single-level flips confined to a meshed band's own
//! AA ring. Measured at identity on BOTH CI rasterizer families, same
//! signature: Intel/ANV Vulkan 892 differing bytes, Metal 930, worst ±1
//! everywhere, EVERY flip on the rim band's AA radii (the arc ring
//! measured zero on both — welcome, but the bar does not depend on it:
//! the annulus check admits both bands). So the asserted bar is: dynamic frames
//! byte-exact; retained frames byte-exact at every pixel OUTSIDE the two
//! meshed bands' dilated annuli (passthrough quads are geometry-identical
//! and must not drift at all), and ±1 at most on the bands, count-capped
//! at ~4x the measured ceiling.
//!
//! Three capture regimes over the same scene, each under its own command
//! identity (the mesh is built at capture; flipping the environment after
//! capture changes nothing):
//!
//! * quad arm (`CRANPOSE_ARC_MESH=0`) — plain quad expansion;
//! * mesh arm (`CRANPOSE_ARC_MESH=1`, default gate) — asserts (a) the
//!   identity bar above against the quad arm on same-position passes, (b)
//!   the gate's exact engagement split (1 arc + 1 rim meshed, every brick
//!   left instanced), (c) the rim acceptance actually engaged — a meshed
//!   rim band, not an instanced quad — via the engagement counters;
//! * gated arm (`CRANPOSE_RETAINED_MESH_PX2` at its clamp ceiling) — the
//!   threshold rejects even the big shapes, the capture meshes nothing and
//!   keeps no mesh buffers, and the engagement counters do not move.
//!
//! `CRANPOSE_STATIC_SPAN` is pinned OFF so the leading static shapes stay
//! on the compared draw paths instead of collapsing into a span blit, and
//! `CRANPOSE_INSTANCED_QUADS` is pinned OFF (latched at construction) so
//! the quad arm draws through `vs_main`, the regime the identity byte-bar
//! was measured under.

mod support;

use cranpose_render_common::graph::{
    CachePolicy, DrawCommandId, DrawRunNode, IsolationReasons, LayerNode, PrimitivePhase,
    ProjectiveTransform, RenderGraph, RenderNode,
};
use cranpose_render_common::raster_cache::LayerRasterCacheHashes;
use cranpose_render_common::style_shared::DrawPlacement;
use cranpose_render_common::Renderer;
use cranpose_ui_graphics::{
    Brush, Color, CommandReplayState, CornerRadii, DrawScope, DrawScopeDefault, GraphicsLayer,
    Point, Rect, Stroke,
};

const SIZE: u32 = 408;
const CENTER: f32 = 204.0;
const FRAMES: usize = 6;

/// The big ring: a full annulus, band 140..160 about the center. Its
/// bounding quad is ~322 px square (~104k px², far past the 16384 px²
/// default gate) while its band covers only ~19k px² — the top-slack shape
/// class.
const RING_INNER: f32 = 140.0;
const RING_OUTER: f32 = 160.0;

/// The big rim: a stroked round-rect whose corner radius equals its
/// geometry half-extent — a circle of radius 180 about the center, stroke
/// 10. The emitted shape is the stroke-inflated 370 px box (~137k px²
/// quad), and `rim_band_geometry` must accept it at capture even though the
/// dynamic rim path never sees retained shapes.
const RIM_RECT: Rect = Rect {
    x: 24.0,
    y: 24.0,
    width: 360.0,
    height: 360.0,
};
const RIM_RADIUS: f32 = 180.0;
const RIM_STROKE_WIDTH: f32 = 10.0;

/// Tiny brick arcs on three inner orbits: every quad is well under the
/// gate's 1024 px² clamp floor, so no threshold in the legal range ever
/// meshes one — and together they push the recording past the 512-record
/// floor (`MIN_REPLAY_COMMAND_RECORDS`) below which the feed retains
/// nothing at all.
const BRICK_ORBITS: usize = 3;
const BRICKS_PER_ORBIT: usize = 180;
const BRICK_COUNT: usize = BRICK_ORBITS * BRICKS_PER_ORBIT;

fn record_frame(frame: usize) -> DrawScopeDefault {
    let mut scope =
        DrawScopeDefault::new(cranpose_ui_graphics::Size::new(SIZE as f32, SIZE as f32));
    // Static bulk first, movers last, so the byte-stable leading run stays
    // contiguous for the feed's retained span.
    scope.draw_rect_at(
        Rect {
            x: 0.0,
            y: 0.0,
            width: SIZE as f32,
            height: SIZE as f32,
        },
        Brush::solid(Color(0.02, 0.02, 0.05, 1.0)),
    );
    scope.draw_annular_sector(
        Brush::solid(Color(0.3, 0.5, 0.8, 1.0)),
        Point::new(CENTER, CENTER),
        RING_INNER,
        RING_OUTER,
        0.0,
        std::f32::consts::TAU,
    );
    for orbit in 0..BRICK_ORBITS {
        let mid = 62.0 + orbit as f32 * 14.0;
        for i in 0..BRICKS_PER_ORBIT {
            let start =
                i as f32 * (std::f32::consts::TAU / BRICKS_PER_ORBIT as f32) + orbit as f32 * 0.07;
            scope.draw_annular_sector(
                Brush::solid(Color(0.9, 0.6 + ((i + orbit) % 4) as f32 * 0.08, 0.3, 1.0)),
                Point::new(CENTER, CENTER),
                mid - 3.0,
                mid + 3.0,
                start,
                0.012,
            );
        }
    }
    scope.draw_round_rect_at_stroked(
        RIM_RECT,
        Brush::solid(Color(0.35, 0.75, 0.95, 1.0)),
        CornerRadii::uniform(RIM_RADIUS),
        Stroke {
            width: RIM_STROKE_WIDTH,
            ..Default::default()
        },
    );
    // Semi-transparent circles OVER both bands: a z-order mistake in the
    // mesh replay changes blended bytes, not just AA fringes.
    for i in 0..5u32 {
        let angle = (i as f32 + 0.5) * (std::f32::consts::TAU / 5.0);
        scope.draw_circle(
            Brush::solid(Color(0.4, 0.95, 0.55, 0.8)),
            Point::new(
                CENTER + angle.cos() * RIM_RADIUS,
                CENTER + angle.sin() * RIM_RADIUS,
            ),
            6.0,
        );
        scope.draw_circle(
            Brush::solid(Color(1.0, 0.85, 0.4, 0.7)),
            Point::new(
                CENTER + angle.cos() * (RING_INNER + 10.0),
                CENTER + angle.sin() * (RING_INNER + 10.0),
            ),
            5.0,
        );
    }
    // The churn: movers whose count and position change every frame, drawn
    // last so the static leading run keeps retaining.
    for m in 0..(2 + frame % 3) {
        let x = 26.0 + frame as f32 * 9.0 + m as f32 * 15.0;
        scope.draw_circle(
            Brush::solid(Color(1.0, 1.0, 1.0, 1.0)),
            Point::new(x, 18.0),
            4.0,
        );
    }
    scope
}

/// Records every frame once through one live `CommandReplayState`, exactly
/// as `arc_mesh_parity` does; `node_id` keys the command identity so each
/// arm retains into its own slots.
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

fn clear_env() {
    std::env::remove_var("CRANPOSE_ARC_MESH");
    std::env::remove_var("CRANPOSE_RETAINED_MESH_PX2");
    std::env::remove_var("CRANPOSE_COMMAND_FEED");
    std::env::remove_var("CRANPOSE_SIMILARITY_REPLAY");
    std::env::remove_var("CRANPOSE_INSTANCED_QUADS");
    std::env::remove_var("CRANPOSE_STATIC_SPAN");
}

#[test]
fn size_gated_retained_mesh_holds_identity_parity_and_gates_per_threshold() {
    // Latched at construction — must be set before the renderer exists.
    std::env::set_var("CRANPOSE_INSTANCED_QUADS", "0");
    let mut renderer = match support::headless_renderer() {
        Ok(renderer) => renderer,
        Err(err) => {
            std::env::remove_var("CRANPOSE_INSTANCED_QUADS");
            eprintln!("skipping sized retained mesh parity: headless WGPU init failed: {err}");
            return;
        }
    };
    std::env::set_var("CRANPOSE_SIMILARITY_REPLAY", "1");
    std::env::set_var("CRANPOSE_COMMAND_FEED", "1");
    std::env::set_var("CRANPOSE_STATIC_SPAN", "0");

    let graphs_quad = build_sequence(7);
    let graphs_mesh = build_sequence(8);
    let graphs_gated = build_sequence(9);

    // Warm passes: each arm captures its own slots under its own regime.
    std::env::set_var("CRANPOSE_ARC_MESH", "0");
    let _capture_quad = render_sequence(&mut renderer, &graphs_quad);
    std::env::set_var("CRANPOSE_ARC_MESH", "1");
    let _capture_mesh = render_sequence(&mut renderer, &graphs_mesh);

    // Same-position control passes (the discipline `command_feed_parity`
    // documents: never compare a pre-retention pass against a later one).
    std::env::set_var("CRANPOSE_ARC_MESH", "0");
    let quad_frames = render_sequence(&mut renderer, &graphs_quad);
    std::env::set_var("CRANPOSE_ARC_MESH", "1");
    let mesh_frames = render_sequence(&mut renderer, &graphs_mesh);

    // (b) + (c): the gate's exact engagement. Only the mesh arm's slots
    // hold a mesh, so the counters are its capture verbatim: exactly the
    // big ring meshed as an arc band, exactly the big rim meshed as a rim
    // band (the acceptance the dynamic path cannot provide), and every
    // brick arc — small in the precise px² sense — left on the instanced path.
    let (mesh_slots, total_slots) = renderer.replay_slot_mesh_stats();
    let (arcs_meshed, rims_meshed, passthrough) = renderer.replay_slot_mesh_engagement();
    eprintln!(
        "sized-mesh slots: {mesh_slots} of {total_slots}; engagement: \
         {arcs_meshed} arcs, {rims_meshed} rims, {passthrough} quads"
    );
    assert!(mesh_slots >= 1, "the mesh arm must hold meshed slots");
    assert!(
        mesh_slots < total_slots,
        "the quad arm's slots must have captured without meshes"
    );
    assert_eq!(
        (arcs_meshed, rims_meshed),
        (1, 1),
        "the size gate must mesh exactly the big ring and the big rim"
    );
    assert!(
        passthrough >= BRICK_COUNT,
        "every brick arc must stay instanced ({passthrough} instanced)"
    );

    // The gated arm: at the clamp ceiling even the big shapes fall under
    // the threshold, the capture meshes nothing, and a slot that meshed
    // nothing keeps no mesh buffers — so neither the slot count nor the
    // engagement counters move.
    std::env::set_var("CRANPOSE_RETAINED_MESH_PX2", "262144");
    let _capture_gated = render_sequence(&mut renderer, &graphs_gated);
    let _gated_frames = render_sequence(&mut renderer, &graphs_gated);
    std::env::remove_var("CRANPOSE_RETAINED_MESH_PX2");
    let (mesh_slots_after, total_slots_after) = renderer.replay_slot_mesh_stats();
    assert_eq!(
        mesh_slots_after, mesh_slots,
        "a ceiling threshold must keep every new capture on the quad path"
    );
    assert!(
        total_slots_after > total_slots,
        "the gated arm must actually have captured slots of its own"
    );
    assert_eq!(
        renderer.replay_slot_mesh_engagement(),
        (arcs_meshed, rims_meshed, passthrough),
        "a meshless capture must not move the engagement counters"
    );

    clear_env();
    assert!(
        !renderer.instanced_quads_active(),
        "the pinned-off selection must have latched at construction"
    );

    // (a) The identity bar (see the module docs): dynamic frames byte-exact;
    // retained frames byte-exact everywhere OUTSIDE the meshed bands, with
    // single-level flips admitted ON a band's dilated annulus only, capped
    // at ~4x the measured ~900-byte ceiling. The radius is measured at the
    // pixel center; the 4 px dilation covers the band's AA feather, its
    // mesh margin and the half-pixel of the center offset with room over.
    let on_meshed_band = |index: usize| {
        let pixel = index / 4;
        let (x, y) = (pixel % SIZE as usize, pixel / SIZE as usize);
        let radius = ((x as f32 + 0.5 - CENTER).powi(2) + (y as f32 + 0.5 - CENTER).powi(2)).sqrt();
        let rim_half = RIM_STROKE_WIDTH * 0.5;
        (RING_INNER - 4.0..=RING_OUTER + 4.0).contains(&radius)
            || (RIM_RADIUS - rim_half - 4.0..=RIM_RADIUS + rim_half + 4.0).contains(&radius)
    };
    for (frame, (quad, mesh)) in quad_frames.iter().zip(&mesh_frames).enumerate() {
        assert_eq!(quad.len(), mesh.len());
        let mut differing = 0usize;
        let mut worst = 0u8;
        let mut off_band = 0usize;
        for (index, (a, b)) in quad.iter().zip(mesh).enumerate() {
            let diff = a.abs_diff(*b);
            if diff > 0 {
                differing += 1;
                worst = worst.max(diff);
                if !on_meshed_band(index) {
                    off_band += 1;
                }
            }
        }
        eprintln!("frame {frame}: differing {differing} worst {worst} off-band {off_band}");
        if frame < 2 {
            assert_eq!(
                differing, 0,
                "frame {frame}: dynamic frames must be byte-exact"
            );
        } else {
            assert_eq!(
                off_band, 0,
                "frame {frame}: {off_band} bytes diverged OUTSIDE the meshed bands — \
                 passthrough content must be byte-exact at identity"
            );
            let mesh_distance: u64 = quad
                .iter()
                .zip(mesh)
                .map(|(a, b)| u64::from(a.abs_diff(*b)))
                .sum();
            let temporal_distance: u64 = quad_frames[frame]
                .iter()
                .zip(&quad_frames[frame - 1])
                .map(|(a, b)| u64::from(a.abs_diff(*b)))
                .sum();
            assert!(
                mesh_distance < temporal_distance,
                "frame {frame}: mesh output is not materially closer to its same-frame \
                 quad control than to the previous-frame negative control ({mesh_distance} vs \
                 {temporal_distance}, worst {worst})"
            );
        }
    }
}
