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
        assert_eq!(
            differing, 0,
            "frame {frame}: {differing} bytes diverged ({beyond_one} beyond ±1, \
             worst {worst}) — instanced quads must replay byte-identical pixels"
        );
    }
}
