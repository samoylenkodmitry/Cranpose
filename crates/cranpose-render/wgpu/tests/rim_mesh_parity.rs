//! Pixel parity for the transient rim band mesh.
//!
//! A huge stroked round-rect whose corner radius equals its geometry
//! half-extent is a circle ring — an arena "rim". The fused shape path
//! detects such shapes per frame (`rim_mesh_band`) and rasterizes only the
//! annulus band ± the antialiasing margin through the retained-mesh
//! machinery (`vs_mesh` + `fs_main`), instead of the ~100k-px bounding quad
//! whose fragments the stroked-round-rect SDF almost entirely discards.
//!
//! The bar is ZERO differing bytes against the quad-expansion arm: for a
//! radius equal to the half-extent the SDF degenerates exactly to an
//! annulus, the band mesh contains every pixel with `|dist| < 0.5`
//! (over-inclusion is free — the SDF discards those pixels identically),
//! and `fs_solid` is pinned byte-exact against `fs_main` by
//! `solid_fs_parity`. Arm separation is by the `CRANPOSE_RIM_MESH` kill
//! switch, which is read per fused-chunk prepare — no renderer relatch is
//! involved, but each arm still renders a throwaway warm pass plus two
//! byte-stable controls (the same-position-control discipline
//! `command_feed_parity` documents).
//!
//! The scene surrounds the rim with small solid circles on BOTH z sides,
//! several overlapping the ring itself, so the draw split (instances below
//! the rim, band mesh, instances above) is exercised and any z-order
//! mistake shows up as differing bytes.

mod support;

use cranpose_render_common::graph::{
    CachePolicy, DrawRunNode, IsolationReasons, LayerNode, PrimitivePhase, ProjectiveTransform,
    RenderGraph, RenderNode,
};
use cranpose_render_common::raster_cache::LayerRasterCacheHashes;
use cranpose_render_common::Renderer;
use cranpose_ui_graphics::{
    Brush, Color, CornerRadii, DrawScope, DrawScopeDefault, GraphicsLayer, Point, Rect, Stroke,
};

const SIZE: u32 = 400;
/// The rim's geometry rect: 320 × 320 at (40, 40). The emitted shape is
/// inflated by half the 8 px stroke (device box 328 × 328 ≈ 107k px², past
/// the 65536 px² gate), and the corner radius 160 equals the geometry
/// half-extent — the outline is a circle of diameter 320 about (200, 200).
const RIM_RECT: Rect = Rect {
    x: 40.0,
    y: 40.0,
    width: 320.0,
    height: 320.0,
};
const RIM_STROKE_WIDTH: f32 = 8.0;
const RIM_RADIUS: f32 = 160.0;

fn record_scene(scope: &mut DrawScopeDefault) {
    scope.draw_rect_at(
        Rect {
            x: 0.0,
            y: 0.0,
            width: SIZE as f32,
            height: SIZE as f32,
        },
        Brush::solid(Color(0.03, 0.03, 0.06, 1.0)),
    );
    // Neighbors BELOW the rim in z, several sitting on the ring itself so
    // the rim must cover them.
    for i in 0..5u32 {
        let angle = i as f32 * (std::f32::consts::TAU / 5.0);
        scope.draw_circle(
            Brush::solid(Color(0.9, 0.5, 0.2, 1.0)),
            Point::new(
                200.0 + angle.cos() * RIM_RADIUS,
                200.0 + angle.sin() * RIM_RADIUS,
            ),
            7.0,
        );
    }
    // The rim itself: a solid stroked circle round-rect, SrcOver, no clip.
    scope.draw_round_rect_at_stroked(
        RIM_RECT,
        Brush::solid(Color(0.35, 0.75, 0.95, 1.0)),
        CornerRadii::uniform(RIM_RADIUS),
        Stroke {
            width: RIM_STROKE_WIDTH,
            ..Default::default()
        },
    );
    // Neighbors ABOVE the rim in z: semi-transparent and overlapping the
    // band, so a draw-order mistake changes blended bytes, not just AA
    // fringes.
    for i in 0..5u32 {
        let angle = (i as f32 + 0.5) * (std::f32::consts::TAU / 5.0);
        scope.draw_circle(
            Brush::solid(Color(0.4, 0.95, 0.55, 0.8)),
            Point::new(
                200.0 + angle.cos() * RIM_RADIUS,
                200.0 + angle.sin() * RIM_RADIUS,
            ),
            6.0,
        );
    }
    for m in 0..6u32 {
        scope.draw_circle(
            Brush::solid(Color(1.0, 0.85, 0.4, 0.9)),
            Point::new(30.0 + m as f32 * 68.0, 14.0),
            5.0,
        );
    }
}

fn rim_graph() -> RenderGraph {
    let mut scope =
        DrawScopeDefault::new(cranpose_ui_graphics::Size::new(SIZE as f32, SIZE as f32));
    record_scene(&mut scope);
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
        children: vec![RenderNode::DrawRun(DrawRunNode::new(
            PrimitivePhase::BeforeChildren,
            scope.into_primitives(),
        ))],
    })
}

/// Warm pass (never compared), then two controls asserted byte-stable; the
/// second control is the arm's frame.
fn render_arm(renderer: &mut support::LockedRenderer, graph: &RenderGraph) -> Vec<u8> {
    let mut passes = Vec::new();
    for _ in 0..3 {
        renderer.scene_mut().graph = Some(graph.clone());
        let captured = renderer
            .capture_frame(SIZE, SIZE)
            .unwrap_or_else(|err| panic!("capture failed: {err:?}"));
        assert_eq!((captured.width, captured.height), (SIZE, SIZE));
        passes.push(captured.pixels);
    }
    assert_eq!(
        passes[1], passes[2],
        "same-graph control passes must be byte-stable before the cross-arm compare"
    );
    passes.pop().unwrap()
}

#[test]
fn rim_band_mesh_matches_the_quad_expansion() {
    // Arm A must run under the DEFAULT switch state.
    std::env::remove_var("CRANPOSE_RIM_MESH");
    let mut renderer = match support::headless_renderer() {
        Ok(renderer) => renderer,
        Err(err) => {
            eprintln!("skipping rim mesh parity: headless WGPU init failed: {err}");
            return;
        }
    };
    if !renderer.instanced_quads_active() {
        // The rim path exists only where the instanced/storage shape path
        // does; a uniform-mode device draws every rim as a quad already.
        eprintln!("skipping rim mesh parity: instanced quads inactive (uniform mode)");
        return;
    }

    let graph = rim_graph();

    let emitted_before = renderer.rim_meshes_emitted();
    let meshed = render_arm(&mut renderer, &graph);
    let emitted_meshed = renderer.rim_meshes_emitted();
    assert!(
        emitted_meshed > emitted_before,
        "arm A must actually draw the rim as a band mesh (counter stayed at {emitted_before})"
    );

    // The switch is read per fused-chunk prepare, so no renderer relatch is
    // needed; one throwaway frame after flipping keeps the arms' warm
    // discipline identical anyway.
    std::env::set_var("CRANPOSE_RIM_MESH", "0");
    let quad = render_arm(&mut renderer, &graph);
    let emitted_after_off = renderer.rim_meshes_emitted();
    std::env::remove_var("CRANPOSE_RIM_MESH");
    assert_eq!(
        emitted_after_off, emitted_meshed,
        "arm B must draw every rim through the quad expansion"
    );

    assert_eq!(meshed.len(), quad.len());
    let mut differing = 0usize;
    let mut beyond_one = 0usize;
    let mut worst = 0u8;
    for (a, b) in meshed.iter().zip(&quad) {
        let diff = a.abs_diff(*b);
        if diff > 0 {
            differing += 1;
            worst = worst.max(diff);
            if diff > 1 {
                beyond_one += 1;
            }
        }
    }
    eprintln!(
        "rim-meshed-vs-quad: differing {differing} (beyond ±1: {beyond_one}) worst {worst}; \
         rims meshed {}",
        emitted_meshed - emitted_before
    );
    assert_eq!(
        differing, 0,
        "{differing} bytes diverged ({beyond_one} beyond ±1, worst {worst}) — \
         the rim band mesh must rasterize byte-identically to the bounding quad"
    );
}
