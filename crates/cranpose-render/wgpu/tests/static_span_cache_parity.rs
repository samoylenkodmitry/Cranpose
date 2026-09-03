mod support;

use cranpose_render_common::{
    Renderer,
    graph::{
        CachePolicy, DrawRunNode, IsolationReasons, LayerNode, PrimitivePhase, ProjectiveTransform,
        RenderGraph, RenderNode,
    },
    raster_cache::LayerRasterCacheHashes,
};
use cranpose_ui_graphics::{Brush, Color, DrawScope, DrawScopeDefault, GraphicsLayer, Point, Rect};

const SIZE: u32 = 240;

fn record_scene(
    scope: &mut DrawScopeDefault,
    frame: u32,
    bg: Color,
    vignette: Color,
    glow: Option<Color>,
) {
    scope.draw_rect_at(
        Rect {
            x: 0.0,
            y: 0.0,
            width: SIZE as f32,
            height: SIZE as f32,
        },
        Brush::solid(bg),
    );
    let radius = SIZE as f32 * 0.52;
    scope.draw_circle(
        Brush::radial_gradient(
            vec![Color(vignette.0, vignette.1, vignette.2, 0.0), vignette],
            Point::new(radius, radius),
            radius,
        ),
        Point::new(SIZE as f32 * 0.5, SIZE as f32 * 0.5),
        radius,
    );
    if let Some(glow) = glow {
        let glow_radius = SIZE as f32 * 0.18;
        scope.draw_circle(
            Brush::radial_gradient(
                vec![glow, Color(glow.0, glow.1, glow.2, 0.0)],
                Point::new(glow_radius, glow_radius),
                glow_radius,
            ),
            Point::new(SIZE as f32 * 0.32, SIZE as f32 * 0.3),
            glow_radius,
        );
    }
    for i in 0..7u32 {
        let angle = frame as f32 * 0.37 + i as f32 * (std::f32::consts::TAU / 7.0);
        scope.draw_circle(
            Brush::solid(Color(0.85, 0.55, 0.25, 1.0)),
            Point::new(
                SIZE as f32 * 0.5 + angle.cos() * SIZE as f32 * 0.33,
                SIZE as f32 * 0.5 + angle.sin() * SIZE as f32 * 0.33,
            ),
            9.0,
        );
    }
    scope.draw_circle(
        Brush::solid(Color(0.35, 0.8, 0.6, 0.6)),
        Point::new(
            SIZE as f32 * 0.5,
            SIZE as f32 * (0.3 + 0.02 * (frame % 5) as f32),
        ),
        14.0,
    );
}

fn scene_graph(frame: u32, bg: Color, vignette: Color, glow: Option<Color>) -> RenderGraph {
    let mut scope =
        DrawScopeDefault::new(cranpose_ui_graphics::Size::new(SIZE as f32, SIZE as f32));
    record_scene(&mut scope, frame, bg, vignette, glow);
    let bounds = Rect {
        x: 0.0,
        y: 0.0,
        width: SIZE as f32,
        height: SIZE as f32,
    };
    RenderGraph::new(LayerNode {
        node_id: None,
        wraps: None,
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
        children: vec![RenderNode::DrawRun(DrawRunNode::new(
            PrimitivePhase::BeforeChildren,
            scope.into_primitives(),
        ))],
    })
}

fn render_sequence<F>(
    renderer: &mut support::LockedRenderer,
    frames: std::ops::Range<u32>,
    graph_for_frame: F,
) -> Vec<Vec<u8>>
where
    F: Fn(u32) -> RenderGraph,
{
    let mut captured = Vec::new();
    for frame in frames {
        renderer.scene_mut().graph = Some(graph_for_frame(frame));
        let pixels = renderer
            .capture_frame(SIZE, SIZE)
            .unwrap_or_else(|err| panic!("capture failed at frame {frame}: {err:?}"));
        assert_eq!((pixels.width, pixels.height), (SIZE, SIZE));
        captured.push(pixels.pixels);
    }
    captured
}

fn assert_frames_identical(baseline: &[Vec<u8>], cached: &[Vec<u8>], label: &str) {
    assert_eq!(baseline.len(), cached.len());
    for (frame, (off, on)) in baseline.iter().zip(cached).enumerate() {
        assert_eq!(off.len(), on.len());
        let mut differing = 0usize;
        let mut worst = 0u8;
        for (a, b) in off.iter().zip(on) {
            let diff = a.abs_diff(*b);
            if diff > 0 {
                differing += 1;
                worst = worst.max(diff);
            }
        }
        assert_eq!(
            differing, 0,
            "{label}: frame {frame} diverged in {differing} bytes (worst {worst}) — \
             the span cache must be byte-exact on every frame, hit or miss"
        );
    }
}

const PALETTE_FLIP_FRAME: u32 = 6;
const FRAMES: u32 = 12;

fn full_flip_graph(frame: u32) -> RenderGraph {
    let (bg, vignette) = if frame < PALETTE_FLIP_FRAME {
        (Color(0.05, 0.05, 0.09, 1.0), Color(0.02, 0.03, 0.12, 0.8))
    } else {
        (Color(0.09, 0.04, 0.04, 1.0), Color(0.13, 0.05, 0.02, 0.8))
    };
    scene_graph(frame, bg, vignette, None)
}

#[test]
fn span_cache_is_byte_exact_across_churn_and_a_palette_flip() {
    cranpose_render_wgpu::set_debug_toggle("CRANPOSE_DISABLE_DIRECT_SCENE_RANGE_CACHE", Some("1"));
    let mut renderer = match support::headless_renderer() {
        Ok(renderer) => renderer,
        Err(err) => {
            eprintln!("skipping static span parity: headless WGPU init failed: {err}");
            return;
        }
    };

    cranpose_render_wgpu::set_debug_toggle("CRANPOSE_STATIC_SPAN", Some("0"));
    let baseline = render_sequence(&mut renderer, 0..FRAMES, full_flip_graph);
    assert_eq!(
        renderer.static_span_stats(),
        (0, 0),
        "the kill switch must keep the cache fully idle"
    );

    cranpose_render_wgpu::set_debug_toggle("CRANPOSE_STATIC_SPAN", None);
    let cached = render_sequence(&mut renderer, 0..FRAMES, full_flip_graph);
    let (hits, recaptures) = renderer.static_span_stats();

    assert_eq!(
        recaptures, 2,
        "expected exactly the initial capture and the post-flip recapture"
    );
    assert_eq!(
        hits,
        (FRAMES - 4) as u64,
        "every stable frame after each capture must hit the cache"
    );

    assert_frames_identical(&baseline, &cached, "full palette flip");
}

const UPGRADE_FRAMES: u32 = 42;

fn glow_flip_graph(frame: u32) -> RenderGraph {
    let bg = Color(0.05, 0.05, 0.09, 1.0);
    let vignette = Color(0.02, 0.03, 0.12, 0.8);
    let glow = if frame < PALETTE_FLIP_FRAME {
        Color(0.3, 0.5, 0.9, 0.5)
    } else {
        Color(0.9, 0.5, 0.2, 0.5)
    };
    scene_graph(frame, bg, vignette, Some(glow))
}

#[test]
fn partial_span_invalidation_stays_byte_exact_and_recovers_the_full_span() {
    cranpose_render_wgpu::set_debug_toggle("CRANPOSE_DISABLE_DIRECT_SCENE_RANGE_CACHE", Some("1"));
    let mut renderer = match support::headless_renderer() {
        Ok(renderer) => renderer,
        Err(err) => {
            eprintln!("skipping static span upgrade parity: headless WGPU init failed: {err}");
            return;
        }
    };

    cranpose_render_wgpu::set_debug_toggle("CRANPOSE_STATIC_SPAN", Some("0"));
    let baseline = render_sequence(&mut renderer, 0..UPGRADE_FRAMES, glow_flip_graph);
    let idle_stats = renderer.static_span_stats();

    cranpose_render_wgpu::set_debug_toggle("CRANPOSE_STATIC_SPAN", None);
    let cached = render_sequence(&mut renderer, 0..UPGRADE_FRAMES, glow_flip_graph);
    let (hits, recaptures) = renderer.static_span_stats();
    let hits = hits - idle_stats.0;
    let recaptures = recaptures - idle_stats.1;

    assert_eq!(
        recaptures, 3,
        "expected initial capture, post-flip shrink capture, and one hysteresis upgrade"
    );
    assert!(
        hits >= 30,
        "hit streaks around the shrink/upgrade seams went missing (hits: {hits})"
    );

    assert_frames_identical(&baseline, &cached, "glow-only flip");
}

#[test]
fn span_capture_inside_a_culled_pass_neither_panics_nor_diverges() {
    use cranpose_render_wgpu::{DisplayVisibleRegion, display_clip_pixel_is_visible};

    cranpose_render_wgpu::set_debug_toggle("CRANPOSE_DISABLE_DIRECT_SCENE_RANGE_CACHE", Some("1"));
    let mut renderer = match support::headless_renderer() {
        Ok(renderer) => renderer,
        Err(err) => {
            eprintln!("skipping culled span capture: headless WGPU init failed: {err}");
            return;
        }
    };

    let flat = render_sequence(&mut renderer, 0..FRAMES, full_flip_graph);
    let (_, recaptures_flat) = renderer.static_span_stats();

    renderer.set_display_visible_region(DisplayVisibleRegion::InscribedCircle);
    cranpose_render_wgpu::set_debug_toggle("CRANPOSE_ROUND_CULL", Some("1"));
    let culled = render_sequence(&mut renderer, 0..FRAMES, full_flip_graph);
    cranpose_render_wgpu::set_debug_toggle("CRANPOSE_ROUND_CULL", None);
    renderer.set_display_visible_region(DisplayVisibleRegion::Full);

    let (hits, recaptures) = renderer.static_span_stats();
    assert!(
        recaptures > recaptures_flat,
        "the culled arm must recapture at the flip frame \
         (recaptures {recaptures_flat} -> {recaptures}, hits {hits})"
    );

    for (frame, (a, b)) in flat.iter().zip(&culled).enumerate() {
        for (i, (pa, pb)) in a
            .as_chunks::<4>()
            .0
            .iter()
            .zip(b.as_chunks::<4>().0)
            .enumerate()
        {
            let (x, y) = (i as u32 % SIZE, i as u32 / SIZE);
            if display_clip_pixel_is_visible(
                DisplayVisibleRegion::InscribedCircle,
                SIZE,
                SIZE,
                x,
                y,
            ) && pa != pb
            {
                panic!(
                    "frame {frame}: visible pixel ({x},{y}) diverged under the \
                     culled span capture: {pa:?} vs {pb:?}"
                );
            }
        }
    }
}
