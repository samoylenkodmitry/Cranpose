// A frame kept by a host and redrawn only in the areas
// `PixelsRenderer::update_picture` reports stays equal to a full redraw
// while an update pushes again what it kept of nodes that did not change:
// boxes and rows that move, a parent that turns, nodes between and above
// the nodes that change, and children that come and go before children
// that change.

use cranpose_core::{key, rememberMutableStateOf};
use cranpose_macros::composable;
use cranpose_ui::{
    Box, BoxSpec, Column, ColumnSpec, LazyColumn, LazyColumnSpec, LazyItems, LazyListScope,
    MemoryHeadroom, Modifier, Text, TextStyle, rememberLazyListState,
};
use cranpose_ui_graphics::{Brush, Color, GraphicsLayer, Point, Rect};

#[path = "support/kept_frames.rs"]
mod kept_frames;

use kept_frames::{
    FRAMES, Step, color, kept_frames_match_full_redraws,
    kept_frames_match_full_redraws_after_unread, kept_frames_match_full_redraws_with,
};

// A box that moves each step, as the box above it grows, whose text changes
// every third step: it moves unchanged, then changes, then moves unchanged
// again.
#[composable]
fn MovedBoxChangingText(captured: Step) {
    let step = rememberMutableStateOf(|| 0u32);
    *captured.borrow_mut() = Some(step);
    Column(
        Modifier::empty().offset(4.0, 8.0),
        ColumnSpec::default(),
        move || {
            let n = step.value();
            Box(
                Modifier::empty()
                    .width(20.0)
                    .height(4.0 + (n % 7) as f32 * 3.0),
                BoxSpec::default(),
                || {},
            );
            Box(
                Modifier::empty()
                    .width(60.0)
                    .height(16.0)
                    .background(color(3, 8)),
                BoxSpec::default(),
                move || {
                    let label = step.value().div_ceil(3);
                    Text(format!("v{label}"), Modifier::empty(), TextStyle::default());
                },
            );
        },
    );
}

#[test]
fn a_child_that_changes_between_moves_draws_what_it_holds_now() {
    kept_frames_match_full_redraws(1.0, MovedBoxChangingText);
}

// A parent its layer turns a little each step, which draws through a
// surface of its own, over a child that never changes and a child whose
// colour changes every third step.
#[composable]
fn SpinningParent(captured: Step) {
    let step = rememberMutableStateOf(|| 0u32);
    *captured.borrow_mut() = Some(step);
    Box(
        Modifier::empty()
            .offset(70.0, 40.0)
            .width(100.0)
            .height(80.0)
            .graphics_layer(move || GraphicsLayer {
                rotation_z: 10.0 + step.value() as f32 * 7.0,
                ..GraphicsLayer::default()
            })
            .background(Color::from_rgb_u8(30, 40, 50)),
        BoxSpec::default(),
        move || {
            Box(
                Modifier::empty()
                    .offset(8.0, 8.0)
                    .width(40.0)
                    .height(20.0)
                    .background(color(2, 5)),
                BoxSpec::default(),
                || {
                    Text("still", Modifier::empty(), TextStyle::default());
                },
            );
            Box(
                Modifier::empty()
                    .offset(8.0, 44.0)
                    .width(60.0)
                    .height(20.0)
                    .background(color(step.value() / 3, 6)),
                BoxSpec::default(),
                || {},
            );
        },
    );
}

#[test]
fn children_under_a_turning_parent_draw_what_they_hold_now() {
    kept_frames_match_full_redraws(1.0, SpinningParent);
}

// A box whose shadow shows every fourth step, above a box whose colour
// changes each step. Each step changes the same two boxes: the steps between
// push them again with no walk to them, and a step that adds or drops the
// shadow splices the items after the first box.
#[composable]
fn ShadowAboveColour(captured: Step) {
    let step = rememberMutableStateOf(|| 0u32);
    *captured.borrow_mut() = Some(step);
    Column(
        Modifier::empty().offset(6.0, 6.0),
        ColumnSpec::default(),
        move || {
            let n = step.value();
            Box(
                Modifier::empty()
                    .width(40.0)
                    .height(20.0)
                    .graphics_layer(move || GraphicsLayer {
                        alpha: 0.9,
                        shadow_elevation: if n % 4 == 0 { 6.0 } else { 0.0 },
                        ..GraphicsLayer::default()
                    })
                    .background(color(1, 5)),
                BoxSpec::default(),
                || {},
            );
            Box(
                Modifier::empty()
                    .width(40.0)
                    .height(20.0)
                    .background(color(n, 6)),
                BoxSpec::default(),
                || {},
            );
        },
    );
}

#[test]
fn nodes_that_change_each_step_draw_right_across_a_splice_before_one() {
    kept_frames_match_full_redraws(1.0, ShadowAboveColour);
}

// Rows that move every third step and rest between, under a count that
// changes each step: their moves after a rest push what was kept of them
// while memory is ample, and lower them again once little is left.
#[composable]
fn RestingRows(captured: Step) {
    let step = rememberMutableStateOf(|| 0u32);
    *captured.borrow_mut() = Some(step);
    Column(
        Modifier::empty().offset(4.0, 4.0),
        ColumnSpec::default(),
        move || {
            let n = step.value();
            Text(
                format!("Count {n}"),
                Modifier::empty(),
                TextStyle::default(),
            );
            Column(
                Modifier::empty().offset((n / 3 * 5) as f32, 0.0),
                ColumnSpec::default(),
                || {
                    for row in 0..4 {
                        Text(
                            format!("Row {row}"),
                            Modifier::empty().background(color(row, 9)),
                            TextStyle::default(),
                        );
                    }
                },
            );
        },
    );
}

// A column whose colour changes each step, over a box that does not change
// and holds a box whose colour changes each step and whose shadow shows
// every fourth step, and a count after them. The column pushes again; the
// box between keeps its record and its draws while its child pushes again.
#[composable]
fn KeptBetween(captured: Step) {
    let step = rememberMutableStateOf(|| 0u32);
    *captured.borrow_mut() = Some(step);
    let n = step.value();
    Column(
        Modifier::empty().offset(4.0, 4.0).background(color(n, 10)),
        ColumnSpec::default(),
        move || {
            Box(
                Modifier::empty()
                    .width(60.0)
                    .height(40.0)
                    .background(color(2, 11))
                    .padding(6.0),
                BoxSpec::default(),
                move || {
                    let n = step.value();
                    Box(
                        Modifier::empty()
                            .width(30.0)
                            .height(20.0)
                            .graphics_layer(move || GraphicsLayer {
                                shadow_elevation: if n % 4 == 0 { 5.0 } else { 0.0 },
                                ..GraphicsLayer::default()
                            })
                            .background(color(n, 12)),
                        BoxSpec::default(),
                        || {},
                    );
                },
            );
            Text(
                format!("Count {}", step.value()),
                Modifier::empty(),
                TextStyle::default(),
            );
        },
    );
}

#[test]
fn a_still_box_between_changing_nodes_keeps_its_draws() {
    kept_frames_match_full_redraws(1.0, KeptBetween);
}

// A box that moves on odd steps with the column that holds it, and rests
// on even steps while the colour of the box it holds changes. A move pushes
// the box whole, and the next update keeps its record while its child
// changes: the move after that lowers it again rather than push what it held
// before.
#[composable]
fn MovesThenChangesAtRest(captured: Step) {
    let step = rememberMutableStateOf(|| 0u32);
    *captured.borrow_mut() = Some(step);
    let shift = step.value().div_ceil(2) as f32 * 3.0;
    Column(
        Modifier::empty().offset(4.0, 4.0 + shift),
        ColumnSpec::default(),
        move || {
            Box(
                Modifier::empty()
                    .width(40.0)
                    .height(24.0)
                    .background(color(3, 13))
                    .padding(4.0),
                BoxSpec::default(),
                move || {
                    let n = step.value();
                    Box(
                        Modifier::empty()
                            .width(20.0)
                            .height(12.0)
                            .background(color(n / 2, 14)),
                        BoxSpec::default(),
                        || {},
                    );
                },
            );
        },
    );
}

#[test]
fn a_box_whose_child_changed_at_rest_draws_it_after_its_next_move() {
    kept_frames_match_full_redraws(1.0, MovesThenChangesAtRest);
}

#[test]
fn rows_that_rest_between_moves_draw_right_with_ample_memory() {
    kept_frames_match_full_redraws_with(1.0, MemoryHeadroom::Ample, RestingRows);
}

#[test]
fn rows_that_rest_between_moves_draw_right_with_little_memory() {
    kept_frames_match_full_redraws_with(1.0, MemoryHeadroom::Tight, RestingRows);
}

// Rows of nested boxes below a box that grows two steps in three, as the
// rows of a list move while it scrolls, each row ending in a box whose
// colour changes each step, and the first row's own colour changing every
// fourth step. The rows and the boxes on the way to each changing box did
// not change themselves: their moves push the layers kept for them while
// they last, and lower them again when their own colour changed.
#[composable]
fn LiveRows(captured: Step) {
    let step = rememberMutableStateOf(|| 0u32);
    *captured.borrow_mut() = Some(step);
    Column(
        Modifier::empty().offset(4.0, 4.0),
        ColumnSpec::default(),
        move || {
            let n = step.value();
            Box(
                Modifier::empty().width(20.0).height((n - n / 3) as f32),
                BoxSpec::default(),
                || {},
            );
            for row in 0..3 {
                LiveRow(row, step);
            }
        },
    );
}

#[composable]
fn LiveRow(row: u32, step: cranpose_core::MutableState<u32>) {
    let own = if row == 0 { step.value() / 4 } else { 0 };
    Box(
        Modifier::empty()
            .width(120.0)
            .height(24.0)
            .background(color(own + row, 13))
            .padding(3.0),
        BoxSpec::default(),
        move || {
            Box(
                Modifier::empty()
                    .width(100.0)
                    .height(18.0)
                    .background(color(row, 14))
                    .padding(2.0),
                BoxSpec::default(),
                move || {
                    Box(
                        Modifier::empty()
                            .width(10.0 + (step.value() * 7 + row * 11) as f32 % 80.0)
                            .height(14.0)
                            .background(color(step.value() + row, 15)),
                        BoxSpec::default(),
                        || {},
                    );
                },
            );
        },
    );
}

// A box whose alpha changes each step over rows like `LiveRows`'s, which
// stay where they are: the rows pass the new alpha down from the layers
// kept for them.
#[composable]
fn FadingLiveRows(captured: Step) {
    let step = rememberMutableStateOf(|| 0u32);
    *captured.borrow_mut() = Some(step);
    Column(
        Modifier::empty()
            .offset(6.0, 6.0)
            .graphics_layer(move || GraphicsLayer {
                alpha: 0.3 + (step.value() % 7) as f32 * 0.1,
                ..GraphicsLayer::default()
            })
            .background(Color::from_rgb_u8(30, 40, 50)),
        ColumnSpec::default(),
        move || {
            for row in 0..3 {
                LiveRow(row, step);
            }
        },
    );
}

#[test]
fn rows_that_move_and_hold_changing_boxes_draw_right_with_ample_memory() {
    kept_frames_match_full_redraws_with(1.0, MemoryHeadroom::Ample, LiveRows);
}

#[test]
fn rows_that_move_and_hold_changing_boxes_draw_right_with_little_memory() {
    kept_frames_match_full_redraws_with(1.0, MemoryHeadroom::Tight, LiveRows);
}

#[test]
fn rows_under_a_changing_alpha_draw_right_with_ample_memory() {
    kept_frames_match_full_redraws_with(1.0, MemoryHeadroom::Ample, FadingLiveRows);
}

#[test]
fn rows_under_a_changing_alpha_draw_right_with_little_memory() {
    kept_frames_match_full_redraws_with(1.0, MemoryHeadroom::Tight, FadingLiveRows);
}

// A list that scrolls 7 pixels a step over rows like `LiveRows`'s: a row
// leaves at its top and another comes in at its bottom every few steps,
// while the rows between move and their boxes change.
#[composable]
fn ScrollingLiveRows(captured: Step) {
    let step = rememberMutableStateOf(|| 0u32);
    *captured.borrow_mut() = Some(step);
    let list = rememberLazyListState();
    let scroll = step.value() * 7;
    list.scroll_to_item((scroll / 24) as usize, (scroll % 24) as f32);
    LazyColumn(
        Modifier::empty()
            .offset(4.0, 4.0)
            .width(130.0)
            .height(120.0),
        list,
        LazyColumnSpec::default(),
        move |scope| {
            scope.items(LazyItems::new(40), move |row| {
                LiveRow(u32::try_from(row).expect("forty rows"), step);
            });
        },
    );
}

// Rows like `LiveRows`'s in a column, each named by a key: the third leaves
// on odd steps and comes back on even ones, and the first two leave together
// every fourth step.
#[composable]
fn ComingAndGoingRows(captured: Step) {
    let step = rememberMutableStateOf(|| 0u32);
    *captured.borrow_mut() = Some(step);
    Column(
        Modifier::empty().offset(4.0, 4.0),
        ColumnSpec::default(),
        move || {
            let n = step.value();
            for row in 0..6 {
                let gone = (row == 2 && n % 2 == 1) || (row < 2 && n % 4 == 3);
                if !gone {
                    key(row, || LiveRow(row, step));
                }
            }
        },
    );
}

#[test]
fn rows_that_scroll_in_and_out_of_a_list_draw_right() {
    kept_frames_match_full_redraws_with(1.0, MemoryHeadroom::Ample, ScrollingLiveRows);
}

#[test]
fn rows_that_come_and_go_before_changing_rows_draw_right() {
    kept_frames_match_full_redraws_with(1.0, MemoryHeadroom::Ample, ComingAndGoingRows);
}

#[test]
fn rows_that_come_and_go_while_no_host_takes_the_changes_draw_right() {
    kept_frames_match_full_redraws_after_unread(1.0, FRAMES / 2, ComingAndGoingRows);
}

// A bar whose width and colour a draw reads from `step`: an update draws it
// again with no composition.
fn drawn_bar(row: u32, salt: u32, step: cranpose_core::MutableState<u32>) -> Modifier {
    Modifier::empty()
        .fill_max_width()
        .height(8.0)
        .draw_behind(move |scope| {
            let size = scope.size();
            let n = step.value();
            let part = ((n * 7 + row * 11 + salt) % 10 + 1) as f32 / 11.0;
            scope.draw_rect_at(
                Rect {
                    x: 0.0,
                    y: 0.0,
                    width: size.width * part,
                    height: size.height,
                },
                Brush::solid(color(n + row, salt)),
            );
        })
}

// A row that draws its background before its graphics layer and pads its
// content inside the layer: a bar, and a padded box around another bar.
// The wrapper of the row's outer draws holds the layer that places the
// row's children.
#[composable]
fn WrappedBarRow(row: u32, step: cranpose_core::MutableState<u32>) {
    Column(
        Modifier::empty()
            .width(120.0)
            .background(color(row, 21))
            .graphics_layer(|| GraphicsLayer {
                alpha: 0.8,
                ..GraphicsLayer::default()
            })
            .padding(5.0),
        ColumnSpec::default(),
        move || {
            Box(drawn_bar(row, 22, step), BoxSpec::default(), || {});
            Box(
                Modifier::empty()
                    .fill_max_width()
                    .background(color(row, 23))
                    .padding(2.0),
                BoxSpec::default(),
                move || {
                    Box(drawn_bar(row, 24, step), BoxSpec::default(), || {});
                },
            );
        },
    );
}

// Rows like `WrappedBarRow` in a column that an offset its placement reads
// moves each step: the column's layer moves, each row pushes again from
// what its record keeps, and its bar draws again.
#[composable]
fn MovingWrappedRows(captured: Step) {
    let step = rememberMutableStateOf(|| 0u32);
    *captured.borrow_mut() = Some(step);
    Column(
        Modifier::empty()
            .offset_with(move || Point::new(4.0 + (step.value() % 4) as f32 * 3.0, 4.0)),
        ColumnSpec::default(),
        move || {
            for row in 0..3 {
                WrappedBarRow(row, step);
            }
        },
    );
}

// A card whose alpha a block reads from `step` over rows like
// `WrappedBarRow`, with `outside` drawn outside its layer: the card's layer
// changes each step, each row pushes again from what its record keeps, and
// its bar draws again.
#[composable]
fn FadingCard(step: cranpose_core::MutableState<u32>, outside: Modifier) {
    Column(
        Modifier::empty()
            .offset(6.0, 6.0)
            .then(outside)
            .graphics_layer_block(move |layer| {
                layer.alpha = 0.3 + (step.value() % 7) as f32 * 0.1;
            })
            .background(Color::from_rgb_u8(30, 40, 50))
            .padding(4.0),
        ColumnSpec::default(),
        move || {
            for row in 0..3 {
                WrappedBarRow(row, step);
            }
        },
    );
}

#[composable]
fn FadingWrappedRows(captured: Step) {
    let step = rememberMutableStateOf(|| 0u32);
    *captured.borrow_mut() = Some(step);
    FadingCard(step, Modifier::empty());
}

// The card in a border it draws outside its layer: the layer that changes
// lies in the wrapper of the card's outer draws.
#[composable]
fn FadingCardInBorder(captured: Step) {
    let step = rememberMutableStateOf(|| 0u32);
    *captured.borrow_mut() = Some(step);
    FadingCard(
        step,
        Modifier::empty()
            .background(Color::from_rgb_u8(90, 30, 60))
            .padding(3.0),
    );
}

#[test]
fn wrapped_rows_in_a_moving_column_draw_their_changing_bars_right() {
    kept_frames_match_full_redraws_with(1.0, MemoryHeadroom::Ample, MovingWrappedRows);
}

#[test]
fn wrapped_rows_under_a_fading_card_draw_their_changing_bars_right() {
    kept_frames_match_full_redraws_with(1.0, MemoryHeadroom::Ample, FadingWrappedRows);
}

#[test]
fn rows_under_a_fading_card_in_a_border_draw_their_changing_bars_right() {
    kept_frames_match_full_redraws_with(1.0, MemoryHeadroom::Ample, FadingCardInBorder);
}
