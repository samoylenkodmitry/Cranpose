mod support;

use std::rc::Rc;

use cranpose_render_common::{
    graph::{DrawCommandId, DrawRunNode, LayerNode, PrimitivePhase, RenderGraph, RenderNode},
    style_shared::DrawPlacement,
};
use cranpose_ui_graphics::{
    ArcRecordArgs, BlendMode, Brush, Color, CommandRecording, DrawScope, DrawScopeDefault, Point,
    Rect, Size, normalized_band,
};

const SIDE: u32 = 256;
const COLUMNS: usize = 16;
const RECORDS: usize = COLUMNS * COLUMNS;

fn rotating_arcs(angle: f32, color: Color) -> RenderGraph {
    let mut scope = DrawScopeDefault::new(Size::new(SIDE as f32, SIDE as f32));
    let cell = SIDE as f32 / COLUMNS as f32;
    for index in 0..RECORDS {
        scope.draw_annular_sector(
            Brush::solid(color),
            Point::new(
                (index % COLUMNS) as f32 * cell + cell * 0.5,
                (index / COLUMNS) as f32 * cell + cell * 0.5,
            ),
            cell * 0.2,
            cell * 0.4,
            angle,
            1.5,
        );
    }
    recording_graph(scope.finish(), SIDE)
}

fn recording_graph(recording: CommandRecording, side: u32) -> RenderGraph {
    let recording = Rc::new(recording);
    let segments = recording.all_segments();
    RenderGraph::new(LayerNode {
        node_id: Some(support::STORED_RUN_NODE),
        local_bounds: Rect {
            x: 0.0,
            y: 0.0,
            width: side as f32,
            height: side as f32,
        },
        children: vec![RenderNode::DrawRun(DrawRunNode::for_command_shared(
            PrimitivePhase::BeforeChildren,
            Some(DrawCommandId {
                node_id: support::STORED_RUN_NODE,
                command_index: 0,
                placement: DrawPlacement::Behind,
            }),
            recording,
            segments,
        ))],
        ..LayerNode::default()
    })
}

fn large_arc_run(count: usize, phase: usize, scope_recorded: bool) -> RenderGraph {
    const COLUMNS: usize = 104;
    const CELL: f32 = 5.0;
    let mut scope = DrawScopeDefault::new(Size::new(520.0, 520.0));
    let mut serial = CommandRecording::default();
    for index in 0..count {
        let brush = Brush::solid(if (index + phase).is_multiple_of(7) {
            Color(0.1, 0.7, 0.3, 1.0)
        } else {
            Color(0.8, 0.3, 0.1, 1.0)
        });
        let center = Point::new(
            (index % COLUMNS) as f32 * CELL + CELL * 0.5,
            (index / COLUMNS) as f32 * CELL + CELL * 0.5,
        );
        let start = phase as f32 * 0.375 + index as f32 * 0.125;
        let sweep = if (index + phase).is_multiple_of(29) {
            0.0
        } else {
            1.5
        };
        if scope_recorded {
            scope.draw_annular_sector(brush, center, 1.0, 2.0, start, sweep);
        } else {
            let args = ArcRecordArgs {
                brush: &brush,
                center,
                radius: 2.0,
                start_angle: start,
                sweep_angle: sweep,
                stroke: None,
                inner_radius: 1.0,
                blend_mode: BlendMode::SrcOver,
            };
            let geometry = normalized_band(&args);
            if !geometry.is_degenerate() {
                serial.push_scope_arc(&args, &geometry);
            }
        }
    }
    recording_graph(
        if scope_recorded {
            scope.finish()
        } else {
            serial
        },
        520,
    )
}

#[test]
fn large_rotating_runs_preserve_pixels_when_lengths_change() {
    let mut renderer = support::headless_renderer().expect("headless renderer");
    let frames = [(5_003, 0), (10_007, 1), (4_201, 2), (123, 3), (5_017, 4)];
    let actual: Vec<_> = frames
        .iter()
        .map(|&(count, phase)| {
            support::present_and_read(&mut renderer, 520, 520, large_arc_run(count, phase, true))
        })
        .collect();
    drop(renderer);
    for ((count, phase), pixels) in frames.into_iter().zip(actual) {
        let mut fresh = support::headless_renderer().expect("fresh renderer");
        let expected =
            support::present_and_read(&mut fresh, 520, 520, large_arc_run(count, phase, false));
        assert!(
            pixels == expected,
            "draw-scope frame {phase} with {count} inputs differs from fresh serial pixels"
        );
    }
}

#[test]
fn rotating_arcs_upload_motion_and_preserve_changed_pixels() {
    let mut renderer = support::headless_renderer().expect("headless renderer");
    let color = Color(0.8, 0.3, 0.1, 1.0);
    let before = support::present_and_read(&mut renderer, SIDE, SIDE, rotating_arcs(0.0, color));
    let after = support::present_and_read(&mut renderer, SIDE, SIDE, rotating_arcs(0.8, color));
    let stats = renderer.last_frame_stats().expect("frame stats");
    assert!(
        before != after,
        "a changed arc angle must change the picture"
    );
    assert!(
        stats.upload_bytes <= (RECORDS * 32 + 4096) as u64,
        "rotation must upload motion without resending unchanged geometry and paint: {} bytes",
        stats.upload_bytes,
    );
    drop(renderer);

    let mut fresh = support::headless_renderer().expect("fresh headless renderer");
    let expected = support::present_and_read(&mut fresh, SIDE, SIDE, rotating_arcs(0.8, color));
    assert!(
        after == expected,
        "the updated table must match a fresh render"
    );
    let recolored = support::present_and_read(
        &mut fresh,
        SIDE,
        SIDE,
        rotating_arcs(0.8, Color(0.1, 0.7, 0.3, 1.0)),
    );
    assert!(
        expected != recolored,
        "paint changes must invalidate the geometry and paint column"
    );
}
