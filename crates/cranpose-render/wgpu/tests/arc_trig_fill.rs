use cranpose_render_common::{
    graph::{
        DrawCommandId, DrawRunNode, PrimitivePhase, ProjectiveTransform, RenderGraph, RenderNode,
    },
    style_shared::DrawPlacement,
};
use cranpose_ui_graphics::{
    Brush, Color, CompositingStrategy, CornerRadii, DrawScope, DrawScopeDefault, GraphicsLayer,
    Point, Rect, Size, Stroke, StrokeCap,
};

use crate::{shared_test_support, support};

const SIZE: f32 = support::SIZE as f32;
const STORED_NODE: cranpose_core::NodeId = 7_400;
const ROW_MISALIGNING_RECORDS: usize = 3;

type Frame = (usize, usize);

fn rect(x: f32, y: f32, width: f32, height: f32) -> Rect {
    Rect {
        x,
        y,
        width,
        height,
    }
}

fn shade(index: usize, tint: usize) -> Brush {
    Brush::solid(Color(
        0.25 + ((index + tint) % 4) as f32 * 0.2,
        0.85 - (index % 3) as f32 * 0.25,
        0.3 + (index % 5) as f32 * 0.15,
        if index.is_multiple_of(2) { 1.0 } else { 0.8 },
    ))
}

fn mixed_shapes(scope: &mut DrawScopeDefault, center: Point, count: usize, (phase, tint): Frame) {
    for index in 0..count {
        let turn = index as f32 * 0.37 + phase as f32 * 0.9;
        let reach = 18.0 + (index % 9) as f32 * 7.0;
        match (index + phase) % 5 {
            0 => scope.draw_annular_sector(
                shade(index, tint),
                center,
                reach,
                reach + 4.0,
                turn,
                0.05 + (index % 3) as f32 * 0.02,
            ),
            1 => scope.draw_arc(
                shade(index, tint),
                center,
                reach,
                turn,
                1.4 + (index % 4) as f32 * 0.6,
                Stroke::new(2.5).with_cap(StrokeCap::Round),
            ),
            2 => scope.draw_round_rect_at(
                rect(
                    center.x - 30.0 + (index % 7) as f32 * 8.0,
                    center.y - 30.0 + (index % 6) as f32 * 9.0,
                    14.0,
                    10.0,
                ),
                shade(index, tint),
                CornerRadii::uniform(4.0),
            ),
            3 => scope.draw_arc(
                shade(index, tint),
                Point::new(center.x + reach * turn.cos(), center.y + reach * turn.sin()),
                6.0,
                turn,
                2.0,
                Stroke::new(2.0).with_cap(StrokeCap::Butt),
            ),
            _ => {
                scope.draw_circle_stroked(
                    shade(index, tint),
                    center,
                    reach + 2.0,
                    Stroke::new(1.5),
                );
            }
        }
    }
}

fn run(center: Point, count: usize, frame: Frame, command: Option<DrawCommandId>) -> RenderNode {
    let mut scope = DrawScopeDefault::new(Size::new(SIZE, SIZE));
    mixed_shapes(&mut scope, center, count, frame);
    RenderNode::DrawRun(DrawRunNode::for_command(
        PrimitivePhase::BeforeChildren,
        command,
        scope.into_primitives(),
    ))
}

fn scene(frame: Frame) -> RenderGraph {
    let stored = run(
        Point::new(SIZE * 0.5, SIZE * 0.3),
        80,
        frame,
        Some(DrawCommandId {
            node_id: STORED_NODE,
            command_index: 0,
            placement: DrawPlacement::Behind,
        }),
    );
    let offscreen = RenderNode::Layer(Box::new(shared_test_support::layer_node(
        rect(0.0, SIZE * 0.5, SIZE * 0.5, SIZE * 0.5),
        ProjectiveTransform::identity(),
        GraphicsLayer {
            compositing_strategy: CompositingStrategy::Offscreen,
            ..GraphicsLayer::default()
        },
        vec![run(
            Point::new(SIZE * 0.25, SIZE * 0.25),
            ROW_MISALIGNING_RECORDS,
            frame,
            None,
        )],
    )));
    let in_frame = run(
        Point::new(SIZE * 0.75, SIZE * 0.75),
        ROW_MISALIGNING_RECORDS,
        frame,
        None,
    );
    RenderGraph::new(shared_test_support::layer_node(
        rect(0.0, 0.0, SIZE, SIZE),
        ProjectiveTransform::identity(),
        GraphicsLayer::default(),
        vec![stored, offscreen, in_frame],
    ))
}

fn frames_without(flags: wgpu::DownlevelFlags, frames: &[Frame]) -> Option<Vec<Vec<u8>>> {
    let mut renderer = match support::headless_renderer_without(flags) {
        Ok(renderer) => renderer,
        Err(err) => {
            eprintln!("skipping arc trig fill parity: headless WGPU init failed: {err}");
            return None;
        }
    };
    Some(
        frames
            .iter()
            .map(|&frame| support::settled_capture(&mut renderer, &scene(frame)))
            .collect(),
    )
}

fn assert_alike(frames: &[Frame]) {
    let Some(filled) = frames_without(wgpu::DownlevelFlags::empty(), frames) else {
        return;
    };
    let Some(derived) = frames_without(wgpu::DownlevelFlags::COMPUTE_SHADERS, frames) else {
        return;
    };
    let Some(uniform_floor) = frames_without(wgpu::DownlevelFlags::VERTEX_STORAGE, frames) else {
        return;
    };
    for (index, &frame) in frames.iter().enumerate() {
        assert!(
            support::distinct_colors(&filled[index]) > 8,
            "frame {index} must draw its shapes"
        );
        for (path, captured) in [
            ("vertex-derived", &derived),
            ("uniform floor", &uniform_floor),
        ] {
            let differing =
                support::differing_pixels(support::SIZE, &filled[index], &captured[index]);
            assert!(
                differing.is_empty(),
                "frame {index} {frame:?}: arcs filled on the GPU and the {path} path differ at {}",
                support::describe_differing(&differing)
            );
        }
    }
}

#[test]
fn arcs_draw_alike_whether_the_gpu_fills_their_trig_or_each_vertex_derives_it() {
    assert_alike(&[(0, 0)]);
}

#[test]
fn a_retained_run_draws_its_arcs_alike_as_its_recording_changes() {
    assert_alike(&[(0, 0), (1, 0), (1, 1), (2, 1), (2, 1), (0, 0)]);
}
