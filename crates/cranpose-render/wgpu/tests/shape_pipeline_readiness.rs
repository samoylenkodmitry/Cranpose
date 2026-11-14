#![cfg(any(target_os = "linux", target_os = "android"))]

use std::rc::Rc;

use cranpose_render_common::{
    Renderer,
    graph::{CachePolicy, DrawCommandId, DrawRunNode, LayerNode, PrimitivePhase, RenderNode},
};
use cranpose_render_wgpu::pipelines_created;
use cranpose_ui_graphics::{
    BlendMode, Brush, Color, DrawPlacement, DrawScope, DrawScopeDefault, Point, Rect, Size,
};
use support::SIZE;

use crate::support;

fn graph(stored: bool, phase: f32) -> LayerNode {
    let mut scope_recording = cranpose_ui_graphics::CommandRecording::default();
    let mut scope =
        DrawScopeDefault::new(Size::new(SIZE as f32, SIZE as f32), &mut scope_recording);
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
    scope.finish();
    let recording = Rc::new(scope_recording);
    let segments = recording.all_segments();
    let command = stored.then_some(DrawCommandId {
        node_id: support::STORED_RUN_NODE,
        command_index: 0,
        placement: DrawPlacement::Behind,
    });
    support::moved_to(
        Point::new(11.25, 7.5),
        LayerNode {
            cache_policy: CachePolicy::None,
            size: Size::new(223.0, 231.0),
            clip_to_bounds: true,
            children: vec![RenderNode::DrawRun(DrawRunNode::for_command_shared(
                PrimitivePhase::BeforeChildren,
                command,
                recording,
                segments,
            ))],
            ..LayerNode::default()
        },
    )
}

fn capture(renderer: &mut support::LockedRenderer, graph: &LayerNode) -> Vec<u8> {
    renderer.scene_mut().describe(graph);
    renderer
        .capture_frame_with_scale(SIZE, SIZE, 1.25)
        .expect("capture")
        .pixels
}

#[test]
fn changing_shape_storage_compiles_nothing_on_the_frame_thread() {
    let _lock = support::gpu_test_lock();
    let mut renderer =
        support::LockedRenderer::compiling_in_background_beside_locked().expect("Vulkan renderer");
    // The fallback pipelines build while an app composes its first screen.
    support::wait_for_background_compiler_idle();
    let first = capture(&mut renderer, &graph(false, 0.0));
    assert!(
        support::distinct_colors(&first) > 8,
        "the first frame must contain visible draws"
    );
    let prepared = pipelines_created();
    let storages = [true, false, true];
    let frames = storages.map(|stored| capture(&mut renderer, &graph(stored, 0.0)));
    let created = pipelines_created();
    drop(renderer);
    assert_eq!(
        created, prepared,
        "while the compiler's threads build shape pipelines, no frame builds one"
    );
    for (stored, frame) in storages.iter().zip(&frames) {
        assert!(
            support::distinct_colors(frame) > 8,
            "stored={stored}: the frame must contain visible draws"
        );
    }
}

// A shape drawn as triangles while its pipeline compiles feathers its edges
// over a pixel: an edge pixel differs from the pipeline's by its coverage,
// not by a whole pixel of the shape.
const EDGE: u8 = 160;

// The frames of `scenes` on a renderer of `limits` that compiles each
// pipeline where a frame first needs it, so no frame draws a stand-in.
fn inline_frames<const N: usize>(limits: &wgpu::Limits, scenes: [&LayerNode; N]) -> [Vec<u8>; N] {
    let mut renderer = support::headless_renderer_configured(
        limits.clone(),
        wgpu::Backends::VULKAN,
        support::device::Pipelines::Inline,
    )
    .expect("Vulkan renderer");
    scenes.map(|scene| capture(&mut renderer, scene))
}

fn check_transition(limits: &wgpu::Limits) {
    for stored in [false, true] {
        let scene = graph(stored, 0.0);
        let changed_scene = graph(stored, 3.75);
        let [reference, changed_reference] = inline_frames(limits, [&scene, &changed_scene]);
        let mut renderer = support::headless_renderer_configured(
            limits.clone(),
            wgpu::Backends::VULKAN,
            support::device::Pipelines::Background,
        )
        .expect("Vulkan renderer");
        support::wait_for_background_compiler_idle();
        let first = capture(&mut renderer, &scene);
        let first_stats = renderer.last_frame_stats().expect("first frame statistics");
        assert!(
            first_stats.shape_pipeline_fallback_draws > 0,
            "first frame must use pending specializations"
        );
        support::assert_bytes_within(
            &format!("stored={stored}: pending pipelines against pipelines compiled inline"),
            SIZE,
            &first,
            &reference,
            EDGE,
        );
        let (stats, ready) = support::settle(|| {
            let current = capture(&mut renderer, &scene);
            let stats = renderer.last_frame_stats().expect("completion statistics");
            (stats, (stats, current))
        });
        assert!(
            stats.shape_specialized_draws > 0,
            "completed frame must use specialization"
        );
        support::assert_same_bytes(
            &format!("stored={stored}: ready pipelines against pipelines compiled inline"),
            SIZE,
            &ready,
            &reference,
        );
        let changed = capture(&mut renderer, &changed_scene);
        drop(renderer);
        assert_ne!(ready, changed, "ready pipelines must draw changed records");
        support::assert_same_bytes(
            &format!("stored={stored}: changed records against pipelines compiled inline"),
            SIZE,
            &changed,
            &changed_reference,
        );
    }
}

#[test]
fn pending_and_ready_pipelines_preserve_order_blends_clipping_and_changed_records() {
    check_transition(&wgpu::Limits::default());
}

#[test]
fn uniform_tables_preserve_pixels_when_specializations_become_ready() {
    check_transition(&wgpu::Limits {
        max_storage_buffers_per_shader_stage: 0,
        ..wgpu::Limits::default()
    });
}
