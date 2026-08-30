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

fn record_frame(
    frame: usize,
    rotate: bool,
    breathe: bool,
    twinkle_churn: bool,
    recolor_at: Option<usize>,
    fractional_grid: bool,
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
        Brush::solid(if fractional_grid {
            Color(0.0, 0.0, 0.0, 1.0)
        } else {
            Color(0.02, 0.02, 0.05, 1.0)
        }),
    );
    if fractional_grid {
        for y in 0..23 {
            for x in 0..23 {
                scope.draw_circle(
                    Brush::solid(Color(0.5, 0.5, 1.0, 0.4)),
                    Point::new(9.0 + x as f32 * 17.0, 9.0 + y as f32 * 17.0),
                    9.5,
                );
            }
        }
    }
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
    fractional_grid: bool,
) -> Vec<RenderGraph> {
    let mut state = CommandReplayState::default();
    let command = DrawCommandId {
        node_id: 7,
        command_index: 0,
        placement: DrawPlacement::Behind,
    };
    (0..frames)
        .map(|frame| {
            let scope = record_frame(
                frame,
                rotate,
                breathe,
                twinkle_churn,
                recolor_at,
                fractional_grid,
            );
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
                has_origin_sinks: false,
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
    differing: usize,
    worst: u8,
    outside_footprint: usize,
}

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
    cranpose_render_wgpu::set_debug_toggle("CRANPOSE_COMMAND_FEED", Some("1"));
    cranpose_render_wgpu::set_debug_toggle("CRANPOSE_ARC_MESH", Some("0"));
    cranpose_render_wgpu::set_debug_toggle("CRANPOSE_SEGMENT_SURFACE_COST_RATIO", Some("6.0"));
    cranpose_render_wgpu::set_debug_toggle("CRANPOSE_SEGMENT_SURFACE", Some("0"));
}

fn clear_env() {
    cranpose_render_wgpu::set_debug_toggle("CRANPOSE_COMMAND_FEED", None);
    cranpose_render_wgpu::set_debug_toggle("CRANPOSE_ARC_MESH", None);
    cranpose_render_wgpu::set_debug_toggle("CRANPOSE_SEGMENT_SURFACE", Some("0"));
    cranpose_render_wgpu::set_debug_toggle("CRANPOSE_SEGMENT_SURFACE_COST_RATIO", None);
}

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
    let graphs = build_sequence(FRAMES, false, false, false, Some(RECOLOR_AT), false);
    let _warmup = render_sequence(&mut renderer, &graphs);
    let baseline = render_sequence(&mut renderer, &graphs);
    let stats_before = renderer.segment_surface_stats();
    assert_eq!(
        stats_before,
        (0, 0, 0, 0, 0),
        "the cache must stay silent while the kill switch is held shut"
    );

    cranpose_render_wgpu::set_debug_toggle("CRANPOSE_SEGMENT_SURFACE", Some("1"));
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

#[test]
fn fractional_alpha_retained_surface_matches_float_blending() {
    let mut renderer = match support::headless_renderer_unencoded() {
        Ok(renderer) => renderer,
        Err(err) => {
            eprintln!(
                "skipping fractional retained surface probe: headless WGPU init failed: {err}"
            );
            return;
        }
    };
    set_common_env();
    cranpose_render_wgpu::set_debug_toggle("CRANPOSE_SEGMENT_SURFACE_COST_RATIO", Some("6.0"));
    let graphs = build_sequence(16, false, false, false, None, true);
    let _ = render_sequence(&mut renderer, &graphs);
    let baseline = render_sequence(&mut renderer, &graphs);
    cranpose_render_wgpu::set_debug_toggle("CRANPOSE_SEGMENT_SURFACE", Some("1"));
    let cached = render_sequence(&mut renderer, &graphs);
    let (captures, composites, _, _, _) = renderer.segment_surface_stats();
    clear_env();
    assert!(captures > 0 && composites > 0);
    let center = ((94 * SIZE + 94) * 4) as usize;
    assert_eq!(
        &cached.last().expect("cached frame")[center..center + 3],
        &baseline.last().expect("baseline frame")[center..center + 3],
        "retaining a fractional-alpha fill must preserve the direct float blend"
    );
    let edge = ((9 * SIZE + 18) * 4) as usize;
    assert_eq!(
        &cached.last().expect("cached frame")[edge..edge + 3],
        &baseline.last().expect("baseline frame")[edge..edge + 3],
        "retaining an antialiased fractional-alpha edge must preserve the direct float blend"
    );
}

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
    let graphs = build_sequence(FRAMES, true, false, true, None, false);
    let _warmup = render_sequence(&mut renderer, &graphs);
    let baseline = render_sequence(&mut renderer, &graphs);

    cranpose_render_wgpu::set_debug_toggle("CRANPOSE_SEGMENT_SURFACE", Some("1"));
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
    assert!(
        worst <= SEGMENT_ROTATION_WORST_BOUND,
        "rotation resampling worst delta {worst} exceeded the measured envelope"
    );
    assert!(
        worst_differing <= SEGMENT_ROTATION_DIFFERING_BOUND,
        "rotation resampling differing count {worst_differing} exceeded the measured envelope"
    );
}

const SEGMENT_ROTATION_WORST_BOUND: u8 = 200;
const SEGMENT_ROTATION_DIFFERING_BOUND: usize = 110_000;

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
    cranpose_render_wgpu::set_debug_toggle("CRANPOSE_SEGMENT_SURFACE_SCALE_EPS", Some("0.004"));
    cranpose_render_wgpu::set_debug_toggle("CRANPOSE_SEGMENT_SURFACE", Some("1"));
    let graphs = build_sequence(FRAMES, false, true, false, None, false);
    let _ = render_sequence(&mut renderer, &graphs);
    let stats = renderer.segment_surface_stats();
    cranpose_render_wgpu::set_debug_toggle("CRANPOSE_SEGMENT_SURFACE_SCALE_EPS", None);
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
