//! Pixel parity for retained arc/ring meshes.
//!
//! Renders the same churning retained scene as `command_feed_parity` under
//! two capture regimes — `CRANPOSE_ARC_MESH=0` (plain quad expansion) and
//! `CRANPOSE_ARC_MESH=1` (the conservative mesh: indexed since P1b,
//! size-gated and default-ON since Stage 3) — and compares same-position
//! passes. Each sector ring is backed by a full annulus whose quad clears
//! the retained size gate, so the mesh arm still meshes under the default
//! threshold while the ~1.3k tiny sector bricks take the passthrough quad —
//! the engagement split the gate exists for, asserted below. The
//! instanced-quad selection is pinned OFF for the whole test so both arms
//! match the regime this envelope was measured under (see the note at the
//! top of the test body).
//!
//! THE MEASURED ENVELOPE, AND WHY IT IS NOT ZERO. The design bar was
//! byte-identical output, on the argument that any vertex stream carrying
//! (pos, uv) consistent with the quad's affine rect map reproduces
//! identical fragment inputs. That argument is falsified by the rasterizer:
//! attribute interpolation is derived per triangle from its own vertices,
//! so two triangulations of the SAME affine uv map disagree by an ulp of
//! interpolated `rect_pos` at a pixel, which after the SDF, smoothstep and
//! unorm8 quantization flips low bits on arc AA edges. A controlled
//! isolation run (all shapes forced through the mesh pipeline as
//! passthrough quads, i.e. geometry bitwise identical to the quad path)
//! measured: identity-transform frames byte-EXACT — the vertex path itself
//! is bit-clean — while rotated frames still differed by ≤27 single-ulp
//! channels because the two vertex entry points compile with different fma
//! contraction. With real arc meshes the divergence is ~5.3k of 666k bytes
//! (~0.8% of pixels) at ±1, plus O(10) pixels per rotated frame at the
//! tight-AABB tangent crop, where an ulp of edge position flips a
//! half-covered pixel (worst measured 78). Zero-byte parity across a
//! re-tessellation is therefore unattainable without deriving `rect_pos`
//! from the fragment coordinate instead of interpolated uv — a shared
//! `fs_main` change explicitly out of scope here.
//!
//! The asserted envelope is ~2x the measured ceiling and stays a real
//! tripwire: a containment bug (dropped band pixels), a seam defect
//! (double-blended or missed strip edges) or a uv mapping error each
//! produce full-color diffs in the thousands.
//!
//! Each arm records under its own command identity so it captures its own
//! slots (the mesh is built at capture time; flipping the environment after
//! capture changes nothing). The compared renders are the third and fourth
//! passes: `command_feed_parity` documents the same-position control trap —
//! the flat detector renders pre-retention frames differently before any
//! feed slot lives in the renderer, so a first pass must never be compared
//! against a later one.

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
        // A full backing annulus under each sector ring, rotating with it:
        // its ~32k-90k px² quad clears the retained size gate, so the mesh
        // arm keeps meshing now that the tiny sectors below pass through.
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
/// identity, so distinct ids retain into distinct renderer slots.
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
                has_origin_sinks: false,
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

#[test]
fn retained_arc_mesh_stays_within_the_interpolation_envelope() {
    // Pin the instanced-quad selection OFF for this renderer (the flag is
    // latched at construction, so it must be set BEFORE the renderer
    // exists): this suite's envelope was measured against `vs_main` quads,
    // and P1b's indexed mesh keeps triangle geometry byte-identical, so the
    // per-frame numbers must reproduce byte-for-byte. Letting the quad arm
    // drift onto `vs_shape_instanced` would fold the separate instancing
    // fma envelope (covered by `instanced_quad_parity`) into this one.
    cranpose_render_wgpu::set_debug_toggle("CRANPOSE_INSTANCED_QUADS", Some("0"));
    let mut renderer = match support::headless_renderer() {
        Ok(renderer) => renderer,
        Err(err) => {
            cranpose_render_wgpu::set_debug_toggle("CRANPOSE_INSTANCED_QUADS", None);
            eprintln!("skipping arc mesh parity: headless WGPU init failed: {err}");
            return;
        }
    };
    cranpose_render_wgpu::set_debug_toggle("CRANPOSE_COMMAND_FEED", Some("1"));

    // Identical scenes under distinct command identities: node 7's slots
    // capture with the mesh disabled, node 8's with it enabled. The flag is
    // (re)set around every pass so any recapture lands under its arm's
    // regime.
    let graphs_quad = build_sequence(7);
    let graphs_mesh = build_sequence(8);

    cranpose_render_wgpu::set_debug_toggle("CRANPOSE_ARC_MESH", Some("0"));
    let _capture_quad = render_sequence(&mut renderer, &graphs_quad);
    cranpose_render_wgpu::set_debug_toggle("CRANPOSE_ARC_MESH", Some("1"));
    let _capture_mesh = render_sequence(&mut renderer, &graphs_mesh);

    // Same-position control passes: every slot of both arms is live from
    // here on, matching the renderer positions command_feed_parity proved
    // byte-stable against each other.
    cranpose_render_wgpu::set_debug_toggle("CRANPOSE_ARC_MESH", Some("0"));
    let quad_frames = render_sequence(&mut renderer, &graphs_quad);
    cranpose_render_wgpu::set_debug_toggle("CRANPOSE_ARC_MESH", Some("1"));
    let mesh_frames = render_sequence(&mut renderer, &graphs_mesh);

    cranpose_render_wgpu::set_debug_toggle("CRANPOSE_ARC_MESH", None);
    cranpose_render_wgpu::set_debug_toggle("CRANPOSE_COMMAND_FEED", None);
    cranpose_render_wgpu::set_debug_toggle("CRANPOSE_INSTANCED_QUADS", None);
    assert!(
        !renderer.instanced_quads_active(),
        "the pinned-off selection must have latched at construction"
    );

    // Non-vacuity: the mesh arm must actually hold meshed slots, and the
    // quad arm's slots must exist without meshes.
    let (mesh_slots, total_slots) = renderer.replay_slot_mesh_stats();
    eprintln!("arc-mesh slots: {mesh_slots} of {total_slots}");
    assert!(
        mesh_slots >= 1,
        "the mesh arm should have captured meshed slots, got {mesh_slots}"
    );
    assert!(
        mesh_slots < total_slots,
        "the quad arm's slots must have captured without meshes \
         ({mesh_slots} of {total_slots} meshed)"
    );
    // The size gate's engagement split: the big backing annuli meshed, the
    // ~1.3k tiny sector bricks per capture took the passthrough quad (the
    // regime that meshed them wholesale measured 4-11 fps slower on the
    // watch), and nothing in this scene is a stroked-circle rim.
    let (arcs_meshed, rims_meshed, passthrough) = renderer.replay_slot_mesh_engagement();
    eprintln!("arc-mesh engagement: {arcs_meshed} arcs, {rims_meshed} rims, {passthrough} quads");
    assert!(
        arcs_meshed >= 1,
        "the backing annuli must clear the size gate"
    );
    assert_eq!(rims_meshed, 0, "no shape here is a stroked-circle rim");
    assert!(
        passthrough > arcs_meshed,
        "the tiny sector bricks must stay on the passthrough quad \
         ({passthrough} quads vs {arcs_meshed} meshed)"
    );

    for (frame, (quad, mesh)) in quad_frames.iter().zip(&mesh_frames).enumerate() {
        assert_eq!(quad.len(), mesh.len());
        let mut differing = 0usize;
        let mut beyond_one = 0usize;
        let mut worst = 0u8;
        for (a, b) in quad.iter().zip(mesh) {
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
        if frame < 2 {
            // Pre-retention frames ride the fresh-batch path in both arms
            // and must stay byte-exact — any drift here is renderer state,
            // not the mesh.
            assert_eq!(
                differing, 0,
                "frame {frame}: dynamic frames must be byte-exact"
            );
        } else {
            // Retained frames: the interpolation-rounding envelope described
            // in the module docs (measured ceiling 5476 / 28 / 78 on Metal).
            assert!(
                differing < 12_000 && beyond_one < 200 && worst < 160,
                "frame {frame}: {differing} bytes diverged ({beyond_one} beyond ±1, \
                 worst {worst}) — beyond the mesh interpolation envelope"
            );
        }
    }
}
