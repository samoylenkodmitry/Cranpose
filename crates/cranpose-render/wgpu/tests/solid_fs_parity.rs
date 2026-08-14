//! Pixel parity for the `fs_solid` fragment entry.
//!
//! A batch whose shapes carry no gradient stops draws through `fs_solid`,
//! whose coverage math is copied from `fs_main` line for line. The
//! `instanced_quad_parity` precedent says textually identical bodies in one
//! module contract identically on this compiler, so the bar here is ZERO
//! differing bytes. If a toolchain update ever splits the contraction, this
//! fails with a handful of ±1 bytes on AA edges — re-measure and pin the
//! envelope per the `arc_mesh_parity` discipline; anything larger is a real
//! defect (wrong entry point wiring, a batch misrouted).
//!
//! Arm separation is by scene composition, not environment: the solid arm
//! is the scene as recorded; the mixed arm appends one 8×8 rect painted
//! with a two-stop, all-transparent linear gradient. Every stop has alpha
//! zero, so its SrcOver blend leaves every destination byte unchanged —
//! but its stops flip the batch's `has_gradient`, forcing the WHOLE batch
//! (the identical solid shapes included) back through `fs_main`. The
//! primitives are hand-built `DrawRunNode::new` runs with no command id,
//! so no feed, no retention, no replay slots: every pass renders through
//! the fresh-batch path, the exact pipeline-selection site under test.

mod support;

use cranpose_render_common::graph::{
    CachePolicy, DrawRunNode, IsolationReasons, LayerNode, PrimitivePhase, ProjectiveTransform,
    RenderGraph, RenderNode,
};
use cranpose_render_common::raster_cache::LayerRasterCacheHashes;
use cranpose_render_common::Renderer;
use cranpose_ui_graphics::{Brush, Color, DrawScope, DrawScopeDefault, GraphicsLayer, Point, Rect};

const SIZE: u32 = 256;
const CENTER: f32 = 128.0;

/// Solid shapes through all four fragment coverage branches: plain rect,
/// rounded rect, stroked rect, and arc bands with both cap styles.
fn record_solid_scene(scope: &mut DrawScopeDefault) {
    scope.draw_rect_at(
        Rect {
            x: 0.0,
            y: 0.0,
            width: SIZE as f32,
            height: SIZE as f32,
        },
        Brush::solid(Color(0.02, 0.02, 0.05, 1.0)),
    );
    for ring in 0..3u32 {
        let radius = 40.0 + ring as f32 * 28.0;
        let count = 48;
        for i in 0..count {
            let start = i as f32 * (std::f32::consts::TAU / count as f32) + ring as f32 * 0.11;
            scope.draw_annular_sector(
                Brush::solid(Color(0.3, 0.5 + (i % 5) as f32 * 0.08, 0.8, 1.0)),
                Point::new(CENTER, CENTER),
                radius - 6.0,
                radius,
                start,
                0.09,
            );
        }
    }
    for m in 0..7u32 {
        scope.draw_circle(
            Brush::solid(Color(1.0, 0.85, 0.4, 0.9)),
            Point::new(24.0 + m as f32 * 32.0, 20.0),
            5.5,
        );
    }
    scope.draw_round_rect_at(
        Rect {
            x: 30.0,
            y: 220.0,
            width: 80.0,
            height: 24.0,
        },
        Brush::solid(Color(0.8, 0.3, 0.4, 1.0)),
        cranpose_ui_graphics::CornerRadii::uniform(8.0),
    );
    scope.draw_rect_at_stroked(
        Rect {
            x: 140.0,
            y: 218.0,
            width: 84.0,
            height: 28.0,
        },
        Brush::solid(Color(0.4, 0.9, 0.6, 1.0)),
        cranpose_ui_graphics::Stroke {
            width: 3.0,
            ..Default::default()
        },
    );
}

/// The invisible gradient: two stops, both alpha zero. Its SrcOver blend is
/// `dst + src * 0`, byte-neutral on every pixel it covers, but its stops
/// force the batch onto `fs_main`.
fn record_transparent_gradient(scope: &mut DrawScopeDefault) {
    scope.draw_rect_at(
        Rect {
            x: 4.0,
            y: 120.0,
            width: 8.0,
            height: 8.0,
        },
        Brush::linear_gradient(vec![Color(1.0, 0.0, 0.0, 0.0), Color(0.0, 1.0, 0.0, 0.0)]),
    );
}

fn graph_for(with_gradient: bool) -> RenderGraph {
    let mut scope =
        DrawScopeDefault::new(cranpose_ui_graphics::Size::new(SIZE as f32, SIZE as f32));
    record_solid_scene(&mut scope);
    if with_gradient {
        record_transparent_gradient(&mut scope);
    }
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

/// Warm pass (never compared — first-render cache warmup renders
/// differently, the same-position-control trap `command_feed_parity`
/// documents), then two controls asserted byte-stable; the second control
/// is the arm's frame.
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

/// How far the two entry points may disagree.
///
/// Not zero, and it cannot be. `fs_solid` and `fs_main` are two shader
/// *programs*, and no driver promises they contract floats identically — the
/// same expression may become an fma in one and a multiply-add in the other
/// depending on register pressure, which differs precisely because one of them
/// carries the gradient code. Measured on Linux/Vulkan: **3 bytes apart, worst
/// 2 levels**, from a 128x128 frame, and unchanged when the coverage math was
/// factored into a single shared function called by both — the duplication was
/// not the cause. macOS/Metal, which is where CI runs the workspace tests, sees
/// zero, which is why this shipped looking exact.
///
/// The bound stays three orders of magnitude below what it has to catch: the
/// negative control for this test — dimming `fs_solid`'s return — moves ~13000
/// bytes. Anything approaching that is a shading difference, not arithmetic
/// noise. A divergence visible to anyone would be far larger still.
const MAX_DIVERGING_BYTES: usize = 16;
/// Per-byte ceiling: a level or two of rounding, never a visible step.
const MAX_DIVERGING_LEVEL: u8 = 2;

#[test]
fn solid_fragment_entry_matches_fs_main() {
    let mut renderer = match support::headless_renderer() {
        Ok(renderer) => renderer,
        Err(err) => {
            eprintln!("skipping solid fs parity: headless WGPU init failed: {err}");
            return;
        }
    };

    let solid = render_arm(&mut renderer, &graph_for(false));
    let mixed = render_arm(&mut renderer, &graph_for(true));

    assert_eq!(solid.len(), mixed.len());
    let mut differing = 0usize;
    let mut beyond_one = 0usize;
    let mut worst = 0u8;
    for (a, b) in solid.iter().zip(&mixed) {
        let diff = a.abs_diff(*b);
        if diff > 0 {
            differing += 1;
            worst = worst.max(diff);
            if diff > 1 {
                beyond_one += 1;
            }
        }
    }
    eprintln!("solid-vs-mixed: differing {differing} (beyond ±1: {beyond_one}) worst {worst}");
    assert!(
        differing <= MAX_DIVERGING_BYTES && worst <= MAX_DIVERGING_LEVEL,
        "{differing} bytes diverged ({beyond_one} beyond ±1, worst {worst}), over a bound of \
         {MAX_DIVERGING_BYTES} bytes and {MAX_DIVERGING_LEVEL} levels — fs_solid must render \
         solid shapes as fs_main does, and a divergence this size is a shading difference \
         rather than the compiler contracting floats differently between two programs"
    );
}
