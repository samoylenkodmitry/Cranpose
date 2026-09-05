mod support;

use std::rc::Rc;

use cranpose_render_common::{
    graph::{DrawCommandId, DrawRunNode, LayerNode, PrimitivePhase, RenderGraph, RenderNode},
    style_shared::DrawPlacement,
};
use cranpose_ui_graphics::{Brush, Color, DrawScope, DrawScopeDefault, Point, Rect, Size};

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
    let recording = Rc::new(scope.finish());
    let segments = recording.all_segments();
    RenderGraph::new(LayerNode {
        node_id: Some(support::STORED_RUN_NODE),
        local_bounds: Rect {
            x: 0.0,
            y: 0.0,
            width: SIDE as f32,
            height: SIDE as f32,
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
