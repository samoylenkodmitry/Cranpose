#![cfg(any(target_os = "linux", target_os = "android"))]

mod support;

use std::{
    rc::Rc,
    time::{Duration, Instant},
};

use cranpose_render_common::{
    Renderer,
    graph::{
        CachePolicy, DrawCommandId, DrawRunNode, LayerNode, PrimitivePhase, RenderGraph, RenderNode,
    },
    style_shared::DrawPlacement,
};
use cranpose_ui_graphics::{BlendMode, Brush, Color, DrawScope, DrawScopeDefault, Rect, Size};
use support::SIZE;

fn graph(stored: bool, phase: f32) -> RenderGraph {
    let bounds = Rect {
        x: 11.25,
        y: 7.5,
        width: 223.0,
        height: 231.0,
    };
    let mut scope = DrawScopeDefault::new(Size::new(SIZE as f32, SIZE as f32));
    support::record_mixed_scene(&mut scope);
    for (index, blend) in [BlendMode::DstOut, BlendMode::Plus, BlendMode::SrcOver]
        .into_iter()
        .enumerate()
    {
        scope.draw_rect_at_blend(
            Rect {
                x: 39.25 + index as f32 * 31.0 + phase,
                y: 83.5,
                width: 56.75,
                height: 97.25,
            },
            Brush::solid(Color(0.3, 0.7, 0.4, 0.37)),
            blend,
        );
    }
    let recording = Rc::new(scope.finish());
    let segments = recording.all_segments();
    let command = stored.then_some(DrawCommandId {
        node_id: support::STORED_RUN_NODE,
        command_index: 0,
        placement: DrawPlacement::Behind,
    });
    RenderGraph::new(LayerNode {
        cache_policy: CachePolicy::None,
        local_bounds: bounds,
        clip_to_bounds: true,
        children: vec![RenderNode::DrawRun(DrawRunNode::for_command_shared(
            PrimitivePhase::BeforeChildren,
            command,
            recording,
            segments,
        ))],
        ..LayerNode::default()
    })
}

fn capture(renderer: &mut support::LockedRenderer, graph: &RenderGraph) -> Vec<u8> {
    renderer.scene_mut().graph = Some(graph.clone());
    renderer
        .capture_frame_with_scale(SIZE, SIZE, 1.25)
        .expect("capture")
        .pixels
}

fn check_transition(limits: wgpu::Limits) {
    for stored in [false, true] {
        let mut renderer =
            support::headless_renderer_configured(limits.clone(), wgpu::Backends::VULKAN)
                .expect("Vulkan renderer");
        let scene = graph(stored, 0.0);
        let first = capture(&mut renderer, &scene);
        let first_stats = renderer.last_frame_stats().expect("first frame statistics");
        assert!(
            first_stats.shape_pipeline_fallback_draws > 0,
            "first frame must use pending specializations"
        );
        assert!(
            support::distinct_colors(&first) > 8,
            "scene must contain visible draws"
        );
        let deadline = Instant::now() + Duration::from_secs(30);
        loop {
            let current = capture(&mut renderer, &scene);
            let differing = first.iter().zip(&current).filter(|(a, b)| a != b).count();
            assert_eq!(
                differing, 0,
                "stored={stored}: publishing a pipeline changed the picture"
            );
            let stats = renderer.last_frame_stats().expect("completion statistics");
            if stats.shape_pipeline_fallback_draws == 0 {
                assert!(
                    stats.shape_specialized_draws > 0,
                    "completed frame must use specialization"
                );
                break;
            }
            assert!(
                Instant::now() < deadline,
                "specialization never became ready"
            );
            std::thread::sleep(Duration::from_millis(5));
        }
        let changed_scene = graph(stored, 3.75);
        let changed = capture(&mut renderer, &changed_scene);
        assert_ne!(first, changed, "ready pipelines must draw changed records");
        assert_eq!(changed, capture(&mut renderer, &changed_scene));
    }
}

#[test]
fn pending_and_ready_pipelines_preserve_order_blends_clipping_and_changed_records() {
    check_transition(wgpu::Limits::default());
}

#[test]
fn uniform_tables_preserve_pixels_when_specializations_become_ready() {
    check_transition(wgpu::Limits {
        max_storage_buffers_per_shader_stage: 0,
        ..wgpu::Limits::default()
    });
}
