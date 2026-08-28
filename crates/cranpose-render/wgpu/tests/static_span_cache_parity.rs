//! Byte parity and engagement for the opaque static leading-span cache.
//!
//! The frame's first draws — a full-screen opaque background rect and a
//! radial-gradient vignette disc — are static for hundreds of frames while
//! everything above them churns. The cache captures that leading span once
//! into a pooled offscreen and replaces it with a single full-target blit on
//! every frame whose leading converted `ShapeData` records (plus gradient
//! stop payloads) memcmp-match the capture.
//!
//! The bar is ZERO differing bytes against the cache-off arm, on every frame
//! of a churning sequence that includes a mid-run palette mutation of the
//! leading shapes (invalidation + recapture) — the mechanism's claim is
//! byte-exactness by construction (opaque clear below, SrcOver-only span,
//! identical pipelines at identical device coordinates, Nearest-sampled
//! alpha-255 blit), so any tolerance would hide a broken link in that chain.
//! Arm separation is by the `CRANPOSE_STATIC_SPAN` kill switch, read per
//! engagement, and engagement is asserted through the renderer's
//! (hits, recaptures) counters: a parity pass with zero hits would prove
//! nothing.
//!
//! Both tests latch `CRANPOSE_DISABLE_DIRECT_SCENE_RANGE_CACHE` (a
//! process-wide `OnceLock`, so it must be set before the first frame): a
//! small static test scene would otherwise be absorbed whole by the
//! direct-scene range cache and never reach the shape path as leading
//! records. On the watch that cache does NOT take the span — `fill-diag`
//! measured the background + vignette submitted as gradient quads every
//! frame, which is the very condition this stage exists for — and when the
//! range cache does absorb a leading chunk, the span cache correctly stays
//! disengaged (the chunk no longer starts with a shape batch).

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

/// One frame of the scene: the static leading span (opaque background rect,
/// then a radial-gradient vignette disc, then optionally a smaller gradient
/// glow disc), then churning content whose bytes change every frame.
/// `bg`/`vignette`/`glow` parameterize the span palette so a test can
/// mutate a leading shape mid-run.
fn record_scene(
    scope: &mut DrawScopeDefault,
    frame: u32,
    bg: Color,
    vignette: Color,
    glow: Option<Color>,
) {
    // 1. Full-screen opaque background — the span's alpha == 1 base.
    scope.draw_rect_at(
        Rect {
            x: 0.0,
            y: 0.0,
            width: SIZE as f32,
            height: SIZE as f32,
        },
        Brush::solid(bg),
    );
    // 2. Static vignette: a radial-gradient disc covering most of the
    //    target, translucent toward the center so it composites against the
    //    background (and so the gradient's ordered dither is live in the
    //    cached bytes).
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
    // 2b. Optional third span shape: a smaller radial-gradient glow disc,
    //     so a test can invalidate the span's TAIL while the gradient-
    //     carrying prefix (background + vignette) stays capturable.
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
    // 3. Churn: orbiting solid dots whose positions move every frame,
    //    several overlapping the vignette so a z-order mistake in the
    //    span-split changes blended bytes, not just fringes.
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
    // 4. A translucent churner right over the vignette center.
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

/// Palette flips for BOTH leading shapes at `PALETTE_FLIP_FRAME` — the
/// palette-drain shape of invalidation: one whole-span miss, then a full
/// recapture of the new span.
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
    // Before the first frame: latched process-wide (see module docs).
    cranpose_render_wgpu::set_debug_toggle("CRANPOSE_DISABLE_DIRECT_SCENE_RANGE_CACHE", Some("1"));
    let mut renderer = match support::headless_renderer() {
        Ok(renderer) => renderer,
        Err(err) => {
            eprintln!("skipping static span parity: headless WGPU init failed: {err}");
            return;
        }
    };

    // Arm A: cache off, the live baseline.
    cranpose_render_wgpu::set_debug_toggle("CRANPOSE_STATIC_SPAN", Some("0"));
    let baseline = render_sequence(&mut renderer, 0..FRAMES, full_flip_graph);
    assert_eq!(
        renderer.static_span_stats(),
        (0, 0),
        "the kill switch must keep the cache fully idle"
    );

    // Arm B: cache on (default state).
    cranpose_render_wgpu::set_debug_toggle("CRANPOSE_STATIC_SPAN", None);
    let cached = render_sequence(&mut renderer, 0..FRAMES, full_flip_graph);
    let (hits, recaptures) = renderer.static_span_stats();

    // Engagement proof — the schedule is deterministic: frame 0 observes,
    // frame 1 captures, 2..6 hit; the flip frame misses whole (both leading
    // shapes changed), frame 7 recaptures, 8..12 hit.
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

/// Glow-only mutation: the background rect and the vignette stay
/// byte-identical, so the recapture after the flip covers only the 2-shape
/// gradient-carrying prefix, and the upgrade hysteresis must re-extend it
/// to the full 3-shape span after 30 consecutive stable frames.
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
    // Before the first frame: latched process-wide (see module docs).
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

    // Initial capture (frame 1), shrink recapture at the flip (frame 6,
    // span = the still-stable background + vignette prefix), upgrade
    // recapture once the re-stabilized glow outlasts the 30-frame
    // hysteresis.
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

/// The span capture pass shares its encoder with the culled fused pass but
/// carries no depth attachment, so the depth-variant pipeline flag must die
/// with the fused pass it described. This exact combination — a capturable
/// static leading span under an `InscribedCircle` visible region — aborted
/// the first round-display device build with a pipeline/pass depth-format
/// validation panic. The bar: the sequence renders at all, the cache still
/// recaptures mid-arm (the flip frame forces the capture pass to run while
/// the region is cullable), and every VISIBLE pixel matches the Full-region
/// cache-on arm byte-for-byte.
#[test]
fn span_capture_inside_a_culled_pass_neither_panics_nor_diverges() {
    use cranpose_render_wgpu::{DisplayVisibleRegion, display_clip_pixel_is_visible};

    // Before the first frame: latched process-wide (see module docs).
    cranpose_render_wgpu::set_debug_toggle("CRANPOSE_DISABLE_DIRECT_SCENE_RANGE_CACHE", Some("1"));
    let mut renderer = match support::headless_renderer() {
        Ok(renderer) => renderer,
        Err(err) => {
            eprintln!("skipping culled span capture: headless WGPU init failed: {err}");
            return;
        }
    };

    // Arm A: Full region, cache on.
    let flat = render_sequence(&mut renderer, 0..FRAMES, full_flip_graph);
    let (_, recaptures_flat) = renderer.static_span_stats();

    // Arm B: the platform reports a cullable region and the cull is opted
    // in (it is opt-in while the on-device abort is under investigation).
    // The mid-sequence palette flip forces a recapture INSIDE this arm, so
    // the capture pass provably encodes while the fused pass is culled.
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
