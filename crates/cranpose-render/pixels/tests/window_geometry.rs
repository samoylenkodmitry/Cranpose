// A node deep inside a row of a scrolled column reports where it lies in
// its window. The rows change nothing while the column scrolls, so an
// update pushes their subtrees again from what it kept of them. The
// reported rect after each scroll step must equal the one a window built
// anew reports.

use std::{
    cell::{Cell, RefCell},
    rc::Rc,
};

use cranpose_app_shell::AppShell;
use cranpose_core::{location_key, remember};
use cranpose_macros::composable;
use cranpose_render_common::{RenderScene, Renderer};
use cranpose_render_pixels::PixelsRenderer;
use cranpose_ui::{Box, BoxSpec, Column, ColumnSpec, Modifier, ScrollState, Text, TextStyle};
use cranpose_ui_graphics::{Color, Rect};

const WIDTH: u32 = 120;
const HEIGHT: u32 = 100;
const ROWS: usize = 12;
const STEPS: u32 = 16;

type Scroll = Rc<RefCell<Option<ScrollState>>>;

// A column that scrolls over rows of nested boxes. The row at `REPORTING`
// holds, two boxes deep, a box that reports its window rect into `sink`.
const REPORTING: usize = 6;

#[composable]
fn ScrolledRows(captured: Scroll, sink: Rc<Cell<Rect>>) {
    let scroll = remember(|| ScrollState::new(0.0)).with(|state| *state);
    *captured.borrow_mut() = Some(scroll);
    Column(
        Modifier::empty()
            .padding(5.0)
            .height(60.0)
            .vertical_scroll(scroll, false),
        ColumnSpec::default(),
        move || {
            for row in 0..ROWS {
                let sink = Rc::clone(&sink);
                Box(
                    Modifier::empty()
                        .height(14.0)
                        .padding(2.0)
                        .background(Color::from_rgb_u8(30, 40 + row as u8 * 9, 60)),
                    BoxSpec::default(),
                    move || {
                        let sink = Rc::clone(&sink);
                        Box(
                            Modifier::empty().padding(1.0),
                            BoxSpec::default(),
                            move || {
                                let reporter = if row == REPORTING {
                                    Modifier::empty().report_window_rect(Rc::clone(&sink))
                                } else {
                                    Modifier::empty()
                                };
                                Text(format!("row {row}"), reporter, TextStyle::default());
                            },
                        );
                    },
                );
            }
        },
    );
}

fn shell(sink: &Rc<Cell<Rect>>) -> (AppShell<PixelsRenderer>, Scroll) {
    let captured: Scroll = Rc::default();
    let content = Rc::clone(&captured);
    let sink = Rc::clone(sink);
    let shell = AppShell::new_with_size_and_density(
        PixelsRenderer::new(),
        location_key(file!(), line!(), column!()),
        move || ScrolledRows(Rc::clone(&content), Rc::clone(&sink)),
        (WIDTH, HEIGHT),
        (WIDTH as f32, HEIGHT as f32),
        1.0,
    );
    (shell, captured)
}

#[test]
fn a_node_in_a_scrolled_row_that_holds_still_reports_where_it_lies_now() {
    let (kept_sink, fresh_sink) = (Rc::default(), Rc::default());
    let (mut kept, kept_scroll) = shell(&kept_sink);
    let (mut fresh, fresh_scroll) = shell(&fresh_sink);
    let mut pixels = vec![0u8; (WIDTH * HEIGHT * 4) as usize];
    let scroll_to = |captured: &Scroll, step: u32| {
        let scroll = (*captured.borrow()).expect("the composition remembered the scroll state");
        scroll.scroll_to(step as f32 * 3.0);
    };
    let mut first = None;
    for step in 0..STEPS {
        scroll_to(&kept_scroll, step);
        kept.update();
        kept.primary()
            .with_renderer(|renderer| renderer.draw(&mut pixels, WIDTH, HEIGHT));
        scroll_to(&fresh_scroll, step);
        fresh
            .primary()
            .with_renderer(|renderer| renderer.scene_mut().clear());
        fresh.request_root_render();
        fresh.update();
        assert_eq!(
            kept_sink.get(),
            fresh_sink.get(),
            "step {step}: the node reports another window rect than a window built anew"
        );
        first.get_or_insert_with(|| fresh_sink.get());
    }
    let scrolled = first.map(|first| first.y - fresh_sink.get().y);
    assert_eq!(
        scrolled,
        Some((STEPS - 1) as f32 * 3.0),
        "the reported rect moves up by the whole scroll"
    );
}
