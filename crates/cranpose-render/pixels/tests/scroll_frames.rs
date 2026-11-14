// A picture a host keeps while a column scrolls under it, in whole and in
// fractional pixels, draws the frames a full redraw draws, whether the
// scroll is all that changes, or a row inside the column, or a node outside
// it changes with it, or the host takes the picture every few updates.

use std::{
    cell::{Cell, RefCell},
    rc::Rc,
};

use cranpose_app_shell::AppShell;
use cranpose_core::{MutableState, location_key, rememberMutableStateOf};
use cranpose_macros::composable;
use cranpose_render_pixels::PixelsRenderer;
use cranpose_ui::{
    BasicText, Box, BoxSpec, Column, ColumnSpec, Modifier, Row, RowSpec, ScrollState, Text,
    TextStyle, text::TextOverflow,
};
use cranpose_ui_graphics::Color;

#[path = "support/kept_frames.rs"]
mod kept_frames;

use kept_frames::KeptFrames;

const WIDTH: u32 = 200;
const HEIGHT: u32 = 160;
const ROWS: usize = 24;
const ROW_HEIGHT: f32 = 32.0;
const VIEWPORT_HEIGHT: f32 = 96.0;

type Step = Rc<RefCell<Option<MutableState<u32>>>>;

thread_local! {
    static SCROLL: Cell<Option<ScrollState>> = const { Cell::new(None) };
}

// What changes with the step besides the scroll.
#[derive(Clone, Copy, PartialEq)]
enum Besides {
    Nothing,
    // A count in a row inside the column.
    ARowInside,
    // A mark drawn before the column, whose count of items changes.
    ANodeOutside,
    // A row whose label is too long for its width and is cut there.
    AClippedText,
    // The column's height, which cuts the rows, grows and shrinks.
    AResizedViewport,
}

#[composable]
fn Rows(step: MutableState<u32>, besides: Besides) {
    let count = match besides {
        Besides::ARowInside => step.value(),
        _ => 0,
    };
    for row in 0..ROWS {
        let shade = row as f32 / ROWS as f32;
        Row(
            Modifier::empty()
                .fill_max_width()
                .height(ROW_HEIGHT)
                .padding_vertical(1.0)
                .background(Color(0.2 + shade * 0.5, 0.3, 0.9 - shade * 0.5, 1.0))
                .rounded_corners(4.0)
                .padding_horizontal(4.0),
            RowSpec::default(),
            move || {
                Box(
                    Modifier::empty()
                        .width(10.0)
                        .height(10.0)
                        .background(Color(1.0 - shade, 0.8, 0.2, 1.0))
                        .rounded_corners(5.0),
                    BoxSpec::default(),
                    || {},
                );
                if let (Besides::AClippedText, 5) = (besides, row) {
                    BasicText(
                        "A label too long to fit the width it has",
                        Modifier::empty().width(50.0),
                        TextStyle::default(),
                        TextOverflow::Clip,
                        false,
                        1,
                        1,
                    );
                } else {
                    Text(
                        format!("Row {row}"),
                        Modifier::empty(),
                        TextStyle::default(),
                    );
                }
                if let (Besides::ARowInside, 7) = (besides, row) {
                    Text(format!("{count}"), Modifier::empty(), TextStyle::default());
                }
            },
        );
    }
}

// A mark before the column in the draw order: the boxes it holds come and go
// with the step, so the scene's items after it move.
#[composable]
fn Mark(step: MutableState<u32>) {
    let boxes = 1 + step.value() % 3;
    Box(
        Modifier::empty()
            .offset(150.0, 4.0)
            .width(40.0)
            .height(20.0),
        BoxSpec::default(),
        move || {
            for index in 0..boxes {
                Box(
                    Modifier::empty()
                        .offset(index as f32 * 12.0, 2.0)
                        .width(10.0)
                        .height(6.0 + index as f32 * 4.0)
                        .background(Color(0.9, 0.3, 0.2 + index as f32 * 0.2, 1.0)),
                    BoxSpec::default(),
                    || {},
                );
            }
        },
    );
}

#[composable]
fn Scrolled(captured: Step, besides: Besides) {
    let step = rememberMutableStateOf(|| 0u32);
    *captured.borrow_mut() = Some(step);
    let scroll = cranpose_core::remember(|| ScrollState::new(0.0)).with(|state| *state);
    SCROLL.with(|slot| slot.set(Some(scroll)));
    Box(
        Modifier::empty().fill_max_size(),
        BoxSpec::default(),
        move || {
            if besides == Besides::ANodeOutside {
                Mark(step);
            }
            Column(
                Modifier::empty()
                    .fill_max_size()
                    .background(Color(0.05, 0.06, 0.1, 1.0))
                    .padding(8.0),
                ColumnSpec::default(),
                move || {
                    Text("Title", Modifier::empty(), TextStyle::default());
                    let height = match besides {
                        Besides::AResizedViewport => {
                            VIEWPORT_HEIGHT + (step.value() % 4) as f32 * 10.0
                        }
                        _ => VIEWPORT_HEIGHT,
                    };
                    Column(
                        Modifier::empty()
                            .fill_max_width()
                            .height(height)
                            .vertical_scroll(scroll, false),
                        ColumnSpec::default(),
                        move || Rows(step, besides),
                    );
                },
            );
        },
    );
}

fn shell(scale: f32, besides: Besides) -> (AppShell<PixelsRenderer>, Step) {
    let captured: Step = Rc::default();
    let content = Rc::clone(&captured);
    let shell = AppShell::new_with_size_and_density(
        PixelsRenderer::new(),
        location_key(file!(), line!(), column!()),
        move || Scrolled(Rc::clone(&content), besides),
        (WIDTH, HEIGHT),
        (WIDTH as f32 / scale, HEIGHT as f32 / scale),
        scale,
    );
    (shell, captured)
}

// Scrolls the column by `deltas`, one step each, and checks the kept frame
// after each `every`-th update against a full redraw.
fn kept_frames_match_full_redraws(scale: f32, besides: Besides, deltas: &[f32], every: u32) {
    let (mut shell, captured) = shell(scale, besides);
    shell.update();
    let scroll = SCROLL
        .with(Cell::get)
        .expect("the column composed its scroll state");
    let steps = u32::try_from(deltas.len() + 1).expect("a few steps");
    KeptFrames::new((WIDTH, HEIGHT), scale).check(&mut shell, (steps, every), |step| {
        scroll.dispatch_raw_delta(deltas[step as usize - 1]);
        (*captured.borrow())
            .expect("the composition remembered the step")
            .set(step);
    });
}

// Steps that move the content by less than a pixel, by whole pixels, by
// fractions that cross a pixel, down far enough to bring rows that lay below
// the viewport into it, and back up.
const STEPS: [f32; 20] = [
    0.4, 0.4, 0.4, 1.0, 2.0, 0.7, 3.3, 0.25, 0.25, 0.25, 0.25, -1.3, -0.4, 5.5, -9.0, 0.6, 40.0,
    35.5, 27.0, -80.0,
];

#[test]
fn a_column_that_only_scrolls_keeps_its_frames() {
    kept_frames_match_full_redraws(1.0, Besides::Nothing, &STEPS, 1);
}

#[test]
fn a_column_whose_row_changes_while_it_scrolls_keeps_its_frames() {
    kept_frames_match_full_redraws(1.0, Besides::ARowInside, &STEPS, 1);
}

#[test]
fn a_mark_before_a_scrolling_column_changes_its_items_and_the_column_keeps_its_frames() {
    kept_frames_match_full_redraws(1.0, Besides::ANodeOutside, &STEPS, 1);
}

#[test]
fn a_column_with_a_clipped_text_keeps_its_frames_while_it_scrolls() {
    kept_frames_match_full_redraws(1.0, Besides::AClippedText, &STEPS, 1);
}

#[test]
fn a_column_resized_while_it_scrolls_keeps_its_frames() {
    kept_frames_match_full_redraws(1.0, Besides::AResizedViewport, &STEPS, 1);
}

#[test]
fn a_host_that_takes_every_third_picture_keeps_its_frames_while_a_column_scrolls() {
    kept_frames_match_full_redraws(1.0, Besides::Nothing, &STEPS, 3);
    kept_frames_match_full_redraws(1.0, Besides::ARowInside, &STEPS, 3);
}

#[test]
fn a_column_scrolls_at_other_densities_and_keeps_its_frames() {
    kept_frames_match_full_redraws(2.0, Besides::Nothing, &STEPS, 1);
    kept_frames_match_full_redraws(1.5, Besides::ARowInside, &STEPS, 1);
}
