//! Pixel parity for instanced ordinary-shape quads (P1b part B).
//!
//! Renders the same churning retained scene as `command_feed_parity` twice —
//! once with `CRANPOSE_INSTANCED_QUADS=0` (the six-vertex `vs_main`
//! expansion) and once with the default-ON instanced path
//! (`vs_shape_instanced`, four vertices through the static quad index
//! buffer, shape index from `instance_index`) — and compares same-position
//! passes. The selection is LATCHED per `GpuRenderer` construction (cached
//! retained bundles encode it), so the arms bracket a `reinit_gpu`: the
//! renderer is rebuilt on a fresh headless device between them, exactly the
//! Android surface-recreation path.
//!
//! THE PARITY BAR: ZERO differing bytes, every frame. The instanced entry
//! point is expression-for-expression identical to `vs_main` and its index
//! pattern (0, 1, 2)(2, 1, 3) reproduces the six-slot corner order exactly,
//! so at the IDENTITY similarity byte-exactness is guaranteed (multiplying
//! by 1.0 and adding 0.0 is exact regardless of fma contraction) — that
//! covers the pre-retention frames, which draw every shape through the
//! fresh-batch path at identity. Retained frames replay under ROTATING
//! similarities, where the P1a lesson warned that two entry points may
//! compile with different fma contraction (P1a's isolation run measured
//! ≤ 27 single-ulp channels per rotated frame for the vs_main/vs_mesh
//! split). MEASURED HERE: zero differing bytes on all eight frames,
//! rotated retained replays included — with the entry bodies textually
//! identical (unlike vs_mesh, whose vertex-buffer input struct changes the
//! function signature), this Metal compiler contracts both identically. The
//! bar is therefore pinned at ZERO, the strongest possible tripwire: if a
//! toolchain update ever splits the contraction, this suite fails loudly
//! with a handful of ±1 bytes and the envelope gets re-measured and
//! documented, exactly the arc_mesh_parity discipline.
//!
//! Same-position-control discipline (documented in `command_feed_parity`):
//! pass 1 of each arm warms every slot and is never compared; passes 2 and 3
//! must byte-equal each other before the cross-arm compare means anything.

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
const FRAMES: usize = 8;

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
    // A moving DstOut punch mid-scene splits the fused shape chunk into
    // SrcOver / DstOut / SrcOver batches, so this suite covers BOTH
    // instanced blend pipelines AND a draw whose `vertex_start > 0` — on
    // the instanced arm that is `draw_indexed` with `first_instance > 0`,
    // the exact GL-hazard/native-guarantee point the design flags: byte
    // parity here proves `instance_index` includes `first_instance` on the
    // native backend (an off-by-`first_instance` shape index would shatter
    // every pixel of the trailing batch). It moves non-similarly so it
    // always rides the fresh-batch path.
    scope.draw_circle_blend(
        Brush::solid(Color(0.0, 0.0, 0.0, 0.6)),
        Point::new(CENTER + frame as f32 * 3.0, CENTER + 140.0),
        9.0,
        cranpose_ui_graphics::BlendMode::DstOut,
    );
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

fn assert_byte_exact(label: &str, a: &[Vec<u8>], b: &[Vec<u8>]) {
    for (frame, (a, b)) in a.iter().zip(b).enumerate() {
        assert_eq!(a.len(), b.len());
        let differing = a.iter().zip(b).filter(|(a, b)| a != b).count();
        assert_eq!(
            differing, 0,
            "{label} frame {frame}: {differing} bytes differ — same-position \
             passes of one arm must be byte-stable"
        );
    }
}

/// One arm: warm pass plus two same-position control passes, byte-equality
/// of the controls asserted, second control returned for the cross-arm
/// compare.
fn render_arm(
    label: &str,
    renderer: &mut support::LockedRenderer,
    graphs: &[RenderGraph],
) -> Vec<Vec<u8>> {
    let _warm = render_sequence(renderer, graphs);
    let control_a = render_sequence(renderer, graphs);
    let control_b = render_sequence(renderer, graphs);
    assert_byte_exact(label, &control_a, &control_b);
    control_b
}

#[test]
fn instanced_quads_match_the_six_vertex_expansion() {
    // Both flags must be in place BEFORE the renderer exists: the instanced
    // selection latches at GpuRenderer construction. The arc mesh stays out
    // of this suite entirely (its own envelope lives in arc_mesh_parity) so
    // every retained draw rides the quad path under measurement.
    cranpose_render_wgpu::set_debug_toggle("CRANPOSE_INSTANCED_QUADS", Some("0"));
    cranpose_render_wgpu::set_debug_toggle("CRANPOSE_ARC_MESH", Some("0"));
    let mut renderer = match support::headless_renderer() {
        Ok(renderer) => renderer,
        Err(err) => {
            cranpose_render_wgpu::set_debug_toggle("CRANPOSE_INSTANCED_QUADS", None);
            cranpose_render_wgpu::set_debug_toggle("CRANPOSE_ARC_MESH", None);
            eprintln!("skipping instanced quad parity: headless WGPU init failed: {err}");
            return;
        }
    };
    cranpose_render_wgpu::set_debug_toggle("CRANPOSE_COMMAND_FEED", Some("1"));

    let graphs = build_sequence(12);

    assert!(
        !renderer.instanced_quads_active(),
        "arm A must have latched the six-vertex path"
    );
    let six_vertex_frames = render_arm("six-vertex-control", &mut renderer, &graphs);

    // Arm B: re-latch ON through the renderer-replacement path (fresh
    // device, retired slots — the same lifecycle a real surface recreation
    // runs), then warm and control exactly like arm A.
    cranpose_render_wgpu::set_debug_toggle("CRANPOSE_INSTANCED_QUADS", Some("1"));
    if let Err(err) = support::reinit_gpu(&mut renderer) {
        cranpose_render_wgpu::set_debug_toggle("CRANPOSE_INSTANCED_QUADS", None);
        cranpose_render_wgpu::set_debug_toggle("CRANPOSE_ARC_MESH", None);
        cranpose_render_wgpu::set_debug_toggle("CRANPOSE_COMMAND_FEED", None);
        eprintln!("skipping instanced quad parity: reinit failed: {err}");
        return;
    }
    assert!(
        renderer.instanced_quads_active(),
        "arm B must have latched the instanced path"
    );
    let instanced_frames = render_arm("instanced-control", &mut renderer, &graphs);

    cranpose_render_wgpu::set_debug_toggle("CRANPOSE_INSTANCED_QUADS", None);
    cranpose_render_wgpu::set_debug_toggle("CRANPOSE_ARC_MESH", None);
    cranpose_render_wgpu::set_debug_toggle("CRANPOSE_COMMAND_FEED", None);

    for (frame, (six_vertex, instanced)) in
        six_vertex_frames.iter().zip(&instanced_frames).enumerate()
    {
        assert_eq!(six_vertex.len(), instanced.len());
        let mut differing = 0usize;
        let mut beyond_one = 0usize;
        let mut worst = 0u8;
        for (a, b) in six_vertex.iter().zip(instanced) {
            let diff = a.abs_diff(*b);
            if diff > 0 {
                differing += 1;
                worst = worst.max(diff);
                if diff > 1 {
                    beyond_one += 1;
                }
            }
        }
        eprintln!("frame {frame}: differing {differing} (beyond ±1: {beyond_one}) worst {worst}");
        // Measured ZERO on every frame — identity fresh batches by
        // construction, rotated retained replays empirically (see the module
        // docs). A failure here with a handful of ±1 bytes on frames >= 2
        // means the compiler started contracting the two entry points
        // differently: re-measure and pin the envelope per the module docs.
        // Anything larger is a real defect (wrong instance range, flipped
        // diagonal, corner/uv mismatch).
        assert_eq!(
            differing, 0,
            "frame {frame}: {differing} bytes diverged ({beyond_one} beyond ±1, \
             worst {worst}) — instanced quads must replay byte-identical pixels"
        );
    }
}
