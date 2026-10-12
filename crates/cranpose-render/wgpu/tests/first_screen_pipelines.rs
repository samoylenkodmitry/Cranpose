//! A launch builds the pipelines an earlier launch's first screen drew with
//! before its own first frame, which then builds none and draws what the
//! earlier launch drew. A web page does this with the records its build
//! embeds; records another backend or another layout of pipeline keys wrote
//! build nothing.

use std::time::Duration;

use cranpose_render_common::graph::{RenderGraph, RenderNode};
use cranpose_render_wgpu::pipelines_created;
use cranpose_ui_graphics::{
    Brush, Color, CornerRadii, DrawPrimitive, Rect, Stroke, StrokeCap, StrokeJoin,
};

use crate::support;

const WIDTH: u32 = 160;
const HEIGHT: u32 = 64;

/// Past the two seconds a renderer's first screen lasts after its first
/// frame, when it starts reporting what that screen drew with.
const FIRST_SCREEN_ENDED: Duration = Duration::from_millis(2100);

/// The bytes after a set's tag, which name the layout of its pipeline keys,
/// and the byte after them, which names the backend that drew it.
const KEY_LAYOUT: std::ops::Range<usize> = 4..12;
const BACKEND: usize = 12;

fn rect(x: f32, y: f32, width: f32, height: f32) -> Rect {
    Rect {
        x,
        y,
        width,
        height,
    }
}

/// A rounded card with a border and a label beside a plain bar.
fn screen() -> RenderGraph {
    let card = |stroke: Option<Stroke>, color: Color| {
        support::draw_node(
            DrawPrimitive::RoundRect {
                rect: rect(6.0, 6.0, 52.0, 52.0),
                brush: Brush::solid(color),
                radii: CornerRadii::uniform(10.0),
                stroke,
            },
            None,
        )
    };
    let children: Vec<RenderNode> = vec![
        support::solid_rect(rect(0.0, 0.0, WIDTH as f32, HEIGHT as f32), Color::BLACK),
        card(None, Color(0.2, 0.5, 0.9, 1.0)),
        card(
            Some(Stroke {
                width: 2.0,
                cap: StrokeCap::Butt,
                join: StrokeJoin::Miter,
            }),
            Color::WHITE,
        ),
        support::solid_rect(rect(66.0, 44.0, 88.0, 6.0), Color(0.9, 0.3, 0.2, 1.0)),
        support::text_node(3, "Cranpose", rect(66.0, 10.0, 90.0, 24.0), 16.0),
    ];
    support::page_graph(WIDTH, HEIGHT, children)
}

/// The pipelines a fresh renderer, given `records` before its first frame,
/// builds while it draws the screen, and what it draws. An empty page drawn
/// first builds what every capture draws with.
fn first_frame_after(records: &[u8]) -> (u64, Vec<u8>) {
    let mut renderer = support::LockedRenderer::beside_locked().expect("a later renderer");
    renderer.build_first_screen_pipelines(records);
    support::capture_graph(
        &mut renderer,
        support::page_graph(WIDTH, HEIGHT, Vec::new()),
        WIDTH,
        HEIGHT,
    );
    let built = pipelines_created();
    let frame = support::capture_graph(&mut renderer, screen(), WIDTH, HEIGHT);
    (pipelines_created() - built, frame.pixels)
}

#[test]
fn a_launch_given_an_earlier_first_screen_draws_its_first_frame_with_pipelines_built_before_it() {
    let Ok(mut earlier) = support::headless_renderer() else {
        return;
    };
    let drawn = support::capture_graph(&mut earlier, screen(), WIDTH, HEIGHT);
    assert!(
        earlier.first_screen_pipelines().is_none(),
        "the first screen lasts past the first frame"
    );
    std::thread::sleep(FIRST_SCREEN_ENDED);
    let records = earlier
        .first_screen_pipelines()
        .expect("the first screen ended");

    let (unrecorded, _) = first_frame_after(&[]);
    assert!(
        unrecorded > 0,
        "without records the first frame builds its pipelines"
    );

    let (built, pixels) = first_frame_after(&records);
    assert_eq!(built, 0, "with them it builds none");
    assert!(
        pixels == drawn.pixels,
        "and draws what the earlier launch drew"
    );

    let mut other_layout = records.clone();
    other_layout[KEY_LAYOUT].copy_from_slice(b"CPKEY000");
    let mut other_backend = records;
    other_backend[BACKEND] ^= 1;
    for (stale, what) in [
        (other_layout, "another layout of pipeline keys"),
        (other_backend, "another backend"),
    ] {
        assert_eq!(
            first_frame_after(&stale).0,
            unrecorded,
            "records written by {what} build nothing"
        );
    }
}

#[test]
fn bytes_that_open_no_first_screen_build_nothing() {
    let Ok(mut renderer) = support::headless_renderer() else {
        return;
    };
    let built = pipelines_created();
    renderer.build_first_screen_pipelines(b"no first screen");
    renderer.build_first_screen_pipelines(b"CPFS");
    assert_eq!(pipelines_created(), built);
}
