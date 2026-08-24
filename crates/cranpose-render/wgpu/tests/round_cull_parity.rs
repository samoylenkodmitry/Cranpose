//! Display clip region cull: byte-exactness is defined over VISIBLE pixels.
//!
//! The platform (stood in for here by `set_display_visible_region`) tells
//! the renderer which region of the surface the panel physically shows;
//! the renderer culls everything outside it on the full-frame pass. The
//! suite is parameterized by [`DisplayVisibleRegion`] — the round display
//! is the first provider (`InscribedCircle`), and a future region variant
//! joins by extending `CULLABLE_REGIONS`. The contract pinned per region:
//!
//! 1. Every pixel the region shows is bitwise identical with the cull on
//!    and off, for a scene that crosses the region boundary with solid
//!    shapes, AA arc edges and gradients.
//! 2. The occluder is conservative: with the cull on, a full-frame opaque
//!    fill still reaches every visible pixel — the occluder never covers
//!    one.
//! 3. Pixels outside the region may differ (that is the cull working),
//!    and at least one must: an arm that fails to engage would silently
//!    prove nothing.
//! 4. `CRANPOSE_ROUND_CULL=0` (the kill switch keeps its first provider's
//!    name) restores whole-buffer bitwise identity even while the
//!    platform reports a cullable region.
//!
//! Every other suite runs with the cull structurally off (no test surface
//! declares a region), which this file's default-`Full` arm doubles as
//! evidence for.

mod support;

use cranpose_render_common::{
    graph::{
        CachePolicy, DrawRunNode, IsolationReasons, LayerNode, PrimitivePhase, ProjectiveTransform,
        RenderGraph, RenderNode,
    },
    raster_cache::LayerRasterCacheHashes,
    Renderer,
};
use cranpose_render_wgpu::{display_clip_pixel_is_visible, DisplayVisibleRegion};
use cranpose_ui_graphics::{Brush, Color, DrawScope, DrawScopeDefault, GraphicsLayer, Point, Rect};

/// Watch-like square surface: the inscribed circle has radius 204
/// centered at (204, 204), leaving ~35 kpx of corners outside it.
const SIZE: u32 = 408;
const CENTER: f32 = 204.0;

/// Every cullable region the framework currently implements. A new
/// provider's region joins this list and inherits the whole suite.
const CULLABLE_REGIONS: [DisplayVisibleRegion; 1] = [DisplayVisibleRegion::InscribedCircle];

/// A scene that deliberately straddles the visible-region boundary for
/// every region under test: a full-frame background (edges and corners
/// included), arc rings crossing the inscribed circle, corner-hugging
/// circles, a gradient sweep across the top-left corner, and a stroked
/// rect across the right edge.
fn record_scene(scope: &mut DrawScopeDefault) {
    scope.draw_rect_at(
        Rect {
            x: 0.0,
            y: 0.0,
            width: SIZE as f32,
            height: SIZE as f32,
        },
        Brush::solid(Color(0.9, 0.88, 0.82, 1.0)),
    );
    scope.draw_rect_at(
        Rect {
            x: -40.0,
            y: -40.0,
            width: 260.0,
            height: 260.0,
        },
        Brush::linear_gradient(vec![Color(0.9, 0.2, 0.2, 0.9), Color(0.1, 0.3, 0.9, 0.4)]),
    );
    for ring in 0..3u32 {
        let radius = 150.0 + ring as f32 * 26.0;
        let count = 40;
        for i in 0..count {
            let start = i as f32 * (std::f32::consts::TAU / count as f32) + ring as f32 * 0.17;
            scope.draw_annular_sector(
                Brush::solid(Color(0.2, 0.4 + (i % 4) as f32 * 0.1, 0.7, 0.95)),
                Point::new(CENTER, CENTER),
                radius - 7.0,
                radius,
                start,
                0.11,
            );
        }
    }
    for (cx, cy) in [(10.0, 10.0), (398.0, 10.0), (10.0, 398.0), (398.0, 398.0)] {
        scope.draw_circle(
            Brush::solid(Color(0.95, 0.7, 0.2, 1.0)),
            Point::new(cx, cy),
            26.0,
        );
    }
    scope.draw_round_rect_at(
        Rect {
            x: 300.0,
            y: 300.0,
            width: 120.0,
            height: 120.0,
        },
        Brush::solid(Color(0.2, 0.7, 0.4, 0.85)),
        cranpose_ui_graphics::CornerRadii::uniform(18.0),
    );
    scope.draw_rect_at_stroked(
        Rect {
            x: 340.0,
            y: 120.0,
            width: 90.0,
            height: 60.0,
        },
        Brush::solid(Color(0.1, 0.1, 0.2, 1.0)),
        cranpose_ui_graphics::Stroke {
            width: 4.0,
            ..Default::default()
        },
    );
}

fn graph_from(record: impl FnOnce(&mut DrawScopeDefault)) -> RenderGraph {
    let mut scope =
        DrawScopeDefault::new(cranpose_ui_graphics::Size::new(SIZE as f32, SIZE as f32));
    record(&mut scope);
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
/// second control is the arm's frame — the discipline every parity suite
/// here follows.
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

/// The reference predicate for one region: whether the display shows the
/// pixel — the set over which the cull promises bitwise identity.
fn visible(region: DisplayVisibleRegion, x: u32, y: u32) -> bool {
    display_clip_pixel_is_visible(region, SIZE, SIZE, x, y)
}

/// Splits a full-buffer diff into (differing visible pixels, differing
/// invisible pixels), reporting the first visible offender.
fn diff_by_region(
    region: DisplayVisibleRegion,
    flat: &[u8],
    culled: &[u8],
) -> (usize, usize, Option<(u32, u32)>) {
    assert_eq!(flat.len(), culled.len());
    let mut visible_diffs = 0usize;
    let mut invisible_diffs = 0usize;
    let mut first_visible = None;
    for y in 0..SIZE {
        for x in 0..SIZE {
            let at = ((y * SIZE + x) * 4) as usize;
            if flat[at..at + 4] == culled[at..at + 4] {
                continue;
            }
            if visible(region, x, y) {
                visible_diffs += 1;
                first_visible.get_or_insert((x, y));
            } else {
                invisible_diffs += 1;
            }
        }
    }
    (visible_diffs, invisible_diffs, first_visible)
}

#[test]
fn cull_is_bitwise_invisible_on_every_pixel_the_region_shows() {
    let mut renderer = match support::headless_renderer() {
        Ok(renderer) => renderer,
        Err(err) => {
            eprintln!("skipping display clip parity: headless WGPU init failed: {err}");
            return;
        }
    };
    let graph = graph_from(record_scene);

    // Arm A: the full region — the structural default every other suite
    // runs under.
    let flat = render_arm(&mut renderer, &graph);

    for region in CULLABLE_REGIONS {
        // Arm B: the platform reports a cullable visible region. The cull is
        // opt-in while the on-device abort is under investigation, so the
        // active arms opt in the way a device build would.
        renderer.set_display_visible_region(region);
        std::env::set_var("CRANPOSE_ROUND_CULL", "1");
        let culled = render_arm(&mut renderer, &graph);
        std::env::remove_var("CRANPOSE_ROUND_CULL");
        renderer.set_display_visible_region(DisplayVisibleRegion::Full);

        let (visible_diffs, invisible_diffs, first_visible) =
            diff_by_region(region, &flat, &culled);
        assert_eq!(
            visible_diffs, 0,
            "{region:?}: cull changed {visible_diffs} pixels the display SHOWS \
             (first at {first_visible:?}) — the occluder touched a visible pixel"
        );
        assert!(
            invisible_diffs > 0,
            "{region:?}: no invisible pixel changed: the cull never engaged and \
             this parity run proved nothing"
        );
        eprintln!(
            "display-clip parity {region:?}: visible diffs 0, invisible diffs {invisible_diffs}"
        );
    }
}

/// Conservative containment, checked through the renderer itself: a
/// full-frame opaque white fill with the cull ON must still reach every
/// pixel the region shows. Any visible pixel that is not white was
/// covered by the occluder.
#[test]
fn occluder_never_covers_a_pixel_the_region_shows() {
    let mut renderer = match support::headless_renderer() {
        Ok(renderer) => renderer,
        Err(err) => {
            eprintln!("skipping display clip containment: headless WGPU init failed: {err}");
            return;
        }
    };
    let graph = graph_from(|scope| {
        scope.draw_rect_at(
            Rect {
                x: 0.0,
                y: 0.0,
                width: SIZE as f32,
                height: SIZE as f32,
            },
            Brush::solid(Color(1.0, 1.0, 1.0, 1.0)),
        );
    });

    for region in CULLABLE_REGIONS {
        renderer.set_display_visible_region(region);
        std::env::set_var("CRANPOSE_ROUND_CULL", "1");
        let culled = render_arm(&mut renderer, &graph);
        std::env::remove_var("CRANPOSE_ROUND_CULL");
        renderer.set_display_visible_region(DisplayVisibleRegion::Full);

        let mut covered_visible = 0usize;
        let mut first = None;
        let mut culled_invisible = 0usize;
        for y in 0..SIZE {
            for x in 0..SIZE {
                let at = ((y * SIZE + x) * 4) as usize;
                let white = culled[at..at + 4] == [255, 255, 255, 255];
                if visible(region, x, y) {
                    if !white {
                        covered_visible += 1;
                        first.get_or_insert((x, y));
                    }
                } else if !white {
                    culled_invisible += 1;
                }
            }
        }
        assert_eq!(
            covered_visible, 0,
            "{region:?}: occluder covered {covered_visible} visible pixels (first at {first:?})"
        );
        assert!(
            culled_invisible > 0,
            "{region:?}: the full-frame fill survived everywhere invisible: \
             the occluder drew nothing"
        );
        eprintln!(
            "display-clip containment {region:?}: 0 visible covered, \
             {culled_invisible} invisible px culled"
        );
    }
}

/// The kill switch: `CRANPOSE_ROUND_CULL=0` restores whole-buffer bitwise
/// identity even while the platform reports a cullable region.
#[test]
fn kill_switch_disables_the_cull_bitwise() {
    let mut renderer = match support::headless_renderer() {
        Ok(renderer) => renderer,
        Err(err) => {
            eprintln!("skipping display clip kill switch: headless WGPU init failed: {err}");
            return;
        }
    };
    let graph = graph_from(record_scene);

    let flat = render_arm(&mut renderer, &graph);

    for region in CULLABLE_REGIONS {
        renderer.set_display_visible_region(region);
        std::env::set_var("CRANPOSE_ROUND_CULL", "0");
        let disabled = render_arm(&mut renderer, &graph);
        std::env::remove_var("CRANPOSE_ROUND_CULL");
        renderer.set_display_visible_region(DisplayVisibleRegion::Full);

        assert_eq!(
            flat, disabled,
            "{region:?}: CRANPOSE_ROUND_CULL=0 must render bitwise identically to Full"
        );
    }
}
