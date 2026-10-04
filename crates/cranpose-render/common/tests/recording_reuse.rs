use std::{cell::Cell, rc::Rc};

use cranpose_core::NodeId;
use cranpose_render_common::{
    graph::{LayerNode, RenderGraph, RenderNode},
    scene_builder::{
        build_graph_from_applier, clear_command_recordings_for_tests, rebuild_graph_from_applier,
    },
};
use cranpose_ui::{
    Brush, Canvas, Color, LayoutEngine, Modifier, Size, TestComposition, run_test_composition,
};
use cranpose_ui_graphics::{CommandRecording, DrawPrimitive};

fn canvas_graph(
    composition: &mut TestComposition,
    root: NodeId,
    previous: Option<RenderGraph>,
) -> RenderGraph {
    let handle = composition.runtime_handle();
    let mut applier = composition.applier_mut();
    applier.set_runtime_handle(handle);
    applier
        .compute_layout(root, Size::new(64.0, 64.0))
        .expect("layout");
    let graph = match previous {
        Some(previous) => rebuild_graph_from_applier(&applier, root, 1.0, Some(previous)),
        None => build_graph_from_applier(&applier, root, 1.0),
    }
    .expect("render graph");
    applier.clear_runtime_handle();
    graph
}

fn retained_recording(layer: &LayerNode) -> Option<CommandRecording> {
    for child in &layer.children {
        match child {
            RenderNode::DrawRun(run) => return Some(run.recording.as_ref().clone()),
            RenderNode::Layer(child) => {
                if let Some(recording) = retained_recording(child) {
                    return Some(recording);
                }
            }
            RenderNode::Primitive(_) => {}
        }
    }
    None
}

fn recording_colors(recording: &CommandRecording) -> Vec<Color> {
    recording
        .primitives_with_markers()
        .filter_map(|primitive| match primitive {
            DrawPrimitive::Rect {
                brush: Brush::Solid(color),
                ..
            } => Some(color),
            _ => None,
        })
        .collect()
}

#[test]
fn canvas_rebuilds_keep_each_retained_frame_and_paint_the_new_state() {
    clear_command_recordings_for_tests();
    let value = Rc::new(Cell::new(0usize));
    let draw_value = Rc::clone(&value);
    let mut composition = run_test_composition(move || {
        let draw_value = Rc::clone(&draw_value);
        Canvas(
            Modifier::empty().size(Size::new(64.0, 64.0)),
            move |scope| {
                let color = match draw_value.get() {
                    0 => Color::RED,
                    1 => Color::GREEN,
                    2 => Color::BLUE,
                    _ => Color::WHITE,
                };
                scope.draw_rect(Brush::solid(color));
            },
        );
    });
    let root = composition.root().expect("canvas root");
    let mut graph = canvas_graph(&mut composition, root, None);
    let expected = [Color::RED, Color::GREEN, Color::BLUE, Color::WHITE];
    let mut retained = Vec::new();

    for (index, expected_color) in expected.into_iter().enumerate() {
        if index > 0 {
            value.set(index);
            graph = canvas_graph(&mut composition, root, Some(graph));
        }
        let mut colors = Vec::new();
        crate::scene_probe::painted_solid_rect_colors(&graph.root, &mut colors);
        assert_eq!(
            colors,
            vec![expected_color],
            "frame {index} paints its state"
        );
        let recording = retained_recording(&graph.root).expect("canvas recording");
        retained.push((recording, expected_color));
    }

    for (index, (recording, expected_color)) in retained.iter().enumerate() {
        assert_eq!(
            recording_colors(recording),
            vec![*expected_color],
            "retained frame {index} keeps its original primitives"
        );
    }

    value.set(0);
    let final_graph = canvas_graph(&mut composition, root, Some(graph));
    let mut colors = Vec::new();
    crate::scene_probe::painted_solid_rect_colors(&final_graph.root, &mut colors);
    assert_eq!(colors, vec![Color::RED]);
}
