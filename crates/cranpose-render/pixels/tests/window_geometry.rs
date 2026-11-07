// A node deep inside a row of a scrolled column reports where it lies in
// its window. The rows change nothing while the column scrolls, so an
// update pushes their subtrees again from what it kept of them. The
// reported rect after each scroll step must equal the one a window built
// anew reports, also when the node starts to report while the column
// scrolls: its new modifiers lie beneath rows that only moved.

use std::{
    cell::{Cell, RefCell},
    rc::Rc,
};

use cranpose_app_shell::AppShell;
use cranpose_core::{MutableState, location_key, remember, rememberMutableStateOf};
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
type Reports = Rc<RefCell<Option<MutableState<bool>>>>;

// A column that scrolls over rows of nested boxes. The row at `REPORTING`
// holds, two boxes deep, a box that reports its window rect into `sink`
// while the state `reports` captures is true.
const REPORTING: usize = 6;

#[composable]
fn ScrolledRows(captured: (Scroll, Reports), sink: Rc<Cell<Rect>>) {
    let scroll = remember(|| ScrollState::new(0.0)).with(|state| *state);
    let reports = rememberMutableStateOf(|| false);
    *captured.0.borrow_mut() = Some(scroll);
    *captured.1.borrow_mut() = Some(reports);
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
                                let reporter = if row == REPORTING && reports.value() {
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

fn shell(sink: &Rc<Cell<Rect>>) -> (AppShell<PixelsRenderer>, (Scroll, Reports)) {
    let captured: (Scroll, Reports) = Default::default();
    let content = (Rc::clone(&captured.0), Rc::clone(&captured.1));
    let sink = Rc::clone(sink);
    let shell = AppShell::new_with_size_and_density(
        PixelsRenderer::new(),
        location_key(file!(), line!(), column!()),
        move || {
            ScrolledRows(
                (Rc::clone(&content.0), Rc::clone(&content.1)),
                Rc::clone(&sink),
            )
        },
        (WIDTH, HEIGHT),
        (WIDTH as f32, HEIGHT as f32),
        1.0,
    );
    (shell, captured)
}

// Scrolls a window kept across updates and one built anew at each step, the
// node reporting from step `reports_from` on, and checks after each step
// that both report the same window rect. Returns the rects reported.
fn scroll_both(reports_from: u32) -> Vec<Rect> {
    let (kept_sink, fresh_sink) = (Rc::default(), Rc::default());
    let (mut kept, kept_states) = shell(&kept_sink);
    let (mut fresh, fresh_states) = shell(&fresh_sink);
    let mut pixels = vec![0u8; (WIDTH * HEIGHT * 4) as usize];
    let step_to = |(scroll, reports): &(Scroll, Reports), step: u32| {
        let scroll = (*scroll.borrow()).expect("the composition remembered the scroll state");
        scroll.scroll_to(step as f32 * 3.0);
        let reports = (*reports.borrow()).expect("the composition remembered the reports state");
        reports.set(step >= reports_from);
    };
    let mut reported = Vec::new();
    for step in 0..STEPS {
        step_to(&kept_states, step);
        kept.update();
        kept.primary()
            .with_renderer(|renderer| renderer.draw(&mut pixels, WIDTH, HEIGHT));
        step_to(&fresh_states, step);
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
        reported.push(fresh_sink.get());
    }
    reported
}

#[test]
fn a_node_in_a_scrolled_row_that_holds_still_reports_where_it_lies_now() {
    let reported = scroll_both(0);
    let scrolled = reported[0].y - reported[reported.len() - 1].y;
    assert_eq!(
        scrolled,
        (STEPS - 1) as f32 * 3.0,
        "the reported rect moves up by the whole scroll"
    );
}

#[test]
fn a_node_that_starts_to_report_while_its_row_scrolls_reports_where_it_lies() {
    let reports_from = STEPS / 2;
    let reported = scroll_both(reports_from);
    assert_eq!(
        reported[0],
        Rect::default(),
        "the node reports nothing at first"
    );
    assert_ne!(
        reported[reports_from as usize],
        Rect::default(),
        "the node reports from the step its modifier came in"
    );
}
