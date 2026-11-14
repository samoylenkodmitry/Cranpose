// Tiles whose dots share pixels with their labels and whose values share
// pixels with their dots draw in a number of calls that does not grow with
// the tiles, and shapes and texts that cover each other keep their order,
// also across the draws a long page makes of what is held.

use cranpose_render_wgpu::CapturedFrame;
use cranpose_ui::text::{SpanStyle, TextUnit};
use support::page::*;

use crate::support;

const TILE: [u32; 2] = [64, 40];
const GAP: u32 = 8;
const COLUMNS: u32 = 6;
const ROWS: u32 = 4;
// Enough tiles, three held draws each, to fill what a pass holds back
// several times.
const MANY_COLUMNS: u32 = 12;
const MANY_ROWS: u32 = 10;
const PAGE: Color = Color(0.08, 0.09, 0.15, 1.0);
const FILL: Color = Color(0.9, 0.9, 0.8, 1.0);
const INK: Color = Color(0.05, 0.05, 0.1, 1.0);
const DOT: Color = Color(1.0, 0.0, 0.0, 1.0);
const DOT_SIZE: f32 = 8.0;
// The draws of the page, the tiles' fills, labels, dots and values, and the
// pass that lays the fills' interiors down.
const MOST_DRAWS: u32 = 8;

fn frame_size(columns: u32, rows: u32) -> [u32; 2] {
    [
        columns * (TILE[0] + GAP) + GAP,
        rows * (TILE[1] + GAP) + GAP,
    ]
}

fn tile_origin(index: u32, columns: u32) -> [u32; 2] {
    [
        GAP + index % columns * (TILE[0] + GAP),
        GAP + index / columns * (TILE[1] + GAP),
    ]
}

fn style(color: Color, size: f32) -> TextStyle {
    TextStyle::from_span_style(SpanStyle {
        color: Some(color),
        font_size: TextUnit::Sp(size),
        ..Default::default()
    })
}

// A tile as the wall draws it: its fill, then its label, a dot and a value,
// `number` digits in each. The dot lies over the lower rows of the label's
// glyphs and the value starts over the dot's right edge.
#[composable]
fn Tile(index: u32, columns: u32, number: u32) {
    let [x, y] = tile_origin(index, columns).map(|edge| edge as f32);
    Box(
        rect_modifier([x, y, TILE[0] as f32, TILE[1] as f32])
            .background(FILL)
            .rounded_corners(4.0),
        BoxSpec::default(),
        move || {
            Text(
                format!("tile {number}"),
                rect_modifier([4.0, 4.0, 56.0, 12.0]),
                style(INK, 10.0),
            );
            Box(
                rect_modifier([4.0, 12.0, DOT_SIZE, DOT_SIZE]).background(DOT),
                BoxSpec::default(),
                || {},
            );
            Text(
                format!("{number}"),
                rect_modifier([4.0 + DOT_SIZE - 1.0, 12.0, 40.0, 12.0]),
                style(INK, 10.0),
            );
        },
    );
}

// The tiles of a `columns` by `rows` page, each numbered by its index or all
// the same.
#[composable]
fn TilePage(columns: u32, rows: u32, numbered: bool) {
    let [width, height] = frame_size(columns, rows);
    FramePage(width, height, PAGE, move || {
        // One held draw ahead of the tiles puts the draw that fills the held
        // draws at a text, not at a shape.
        Text(
            "page",
            rect_modifier([0.0, 0.0, 30.0, 6.0]),
            style(Color::WHITE, 5.0),
        );
        for index in 0..columns * rows {
            Tile(index, columns, if numbered { index } else { 0 });
        }
    });
}

#[composable]
fn Tiles() {
    TilePage(COLUMNS, ROWS, true);
}

#[composable]
fn SameTiles() {
    TilePage(MANY_COLUMNS, MANY_ROWS, false);
}

fn pixel(frame: &CapturedFrame, x: u32, y: u32) -> [u8; 4] {
    let at = ((y * frame.width + x) * 4) as usize;
    [
        frame.pixels[at],
        frame.pixels[at + 1],
        frame.pixels[at + 2],
        frame.pixels[at + 3],
    ]
}

// How many pixels of `x` by `y` ranges satisfy `test`.
fn count(
    frame: &CapturedFrame,
    x: std::ops::Range<u32>,
    y: std::ops::Range<u32>,
    test: impl Fn([u8; 4]) -> bool,
) -> usize {
    y.flat_map(|y| x.clone().map(move |x| (x, y)))
        .filter(|&(x, y)| test(pixel(frame, x, y)))
        .count()
}

fn is_dark(pixel: [u8; 4]) -> bool {
    pixel[..3].iter().all(|channel| *channel < 150)
}

fn is_red(pixel: [u8; 4]) -> bool {
    pixel[0] > 200 && pixel[1] < 60 && pixel[2] < 60
}

#[test]
fn tiles_with_overlapping_labels_dots_and_values_draw_in_a_few_calls() {
    let [width, height] = frame_size(COLUMNS, ROWS);
    let Some((frame, stats)) = support::warm_app_frame(Tiles, width, height) else {
        return;
    };
    for index in 0..COLUMNS * ROWS {
        let [x, y] = tile_origin(index, COLUMNS);
        let fill = pixel(&frame, x + 56, y + 32);
        assert!(
            fill[0] > 200 && fill[1] > 200 && fill[2] > 150,
            "tile {index}: the fill shows, {fill:?}"
        );
        assert!(
            is_red(pixel(&frame, x + 8, y + 16)),
            "tile {index}: the dot shows over the label's lower rows"
        );
        let label = count(&frame, x + 4..x + 60, y + 4..y + 12, is_dark);
        assert!(label > 10, "tile {index}: the label shows, {label} pixels");
        let value = count(&frame, x + 12..x + 40, y + 12..y + 24, is_dark);
        assert!(value > 3, "tile {index}: the value shows, {value} pixels");
        let dots = count(&frame, x..x + 64, y..y + 40, is_red);
        assert!(
            dots > 30,
            "tile {index}: the dot keeps its pixels, {dots} of them"
        );
    }
    assert!(
        stats.draw_calls <= MOST_DRAWS,
        "{} tiles drew in {} calls",
        COLUMNS * ROWS,
        stats.draw_calls
    );
}

// Tiles that differ in nothing draw the same pixels wherever they lie: a
// draw of what is held that repeats or skips a text shows in the tiles after
// it.
#[test]
fn tiles_drawn_after_the_held_draws_flush_look_as_the_first_does() {
    let [width, height] = frame_size(MANY_COLUMNS, MANY_ROWS);
    let Some((frame, _)) = support::warm_app_frame(SameTiles, width, height) else {
        return;
    };
    let [first_x, first_y] = tile_origin(0, MANY_COLUMNS);
    for index in 1..MANY_COLUMNS * MANY_ROWS {
        let [x, y] = tile_origin(index, MANY_COLUMNS);
        for (dx, dy) in (0..TILE[0]).flat_map(|dx| (0..TILE[1]).map(move |dy| (dx, dy))) {
            assert_eq!(
                pixel(&frame, x + dx, y + dy),
                pixel(&frame, first_x + dx, first_y + dy),
                "tile {index} differs from the first at {dx},{dy}"
            );
        }
    }
}

const STACK: [u32; 2] = [200, 100];
const BACK: Color = Color(1.0, 1.0, 1.0, 1.0);
const SHEET: Color = Color(0.0, 0.0, 1.0, 1.0);

// A text, a shape over its right half, a second text over the shape and a
// second shape over the second text's right end: each draws over the one
// before it.
#[composable]
fn CoveredTexts() {
    FramePage(STACK[0], STACK[1], PAGE, || {
        Text(
            "MMMM",
            rect_modifier([8.0, 8.0, 120.0, 40.0]),
            style(BACK, 30.0),
        );
        Box(
            rect_modifier([60.0, 4.0, 60.0, 50.0]).background(DOT),
            BoxSpec::default(),
            || {},
        );
        Text(
            "WWWW",
            rect_modifier([70.0, 24.0, 120.0, 40.0]),
            style(BACK, 30.0),
        );
        Box(
            rect_modifier([122.0, 20.0, 40.0, 50.0]).background(SHEET),
            BoxSpec::default(),
            || {},
        );
    });
}

fn is_white(pixel: [u8; 4]) -> bool {
    pixel[..3].iter().all(|channel| *channel > 200)
}

#[test]
fn shapes_and_texts_that_cover_each_other_keep_their_order() {
    let Some((frame, _)) = support::warm_app_frame(CoveredTexts, STACK[0], STACK[1]) else {
        return;
    };
    assert!(
        count(&frame, 8..58, 8..44, is_white) > 20,
        "the first text shows where nothing covers it"
    );
    assert_eq!(
        count(&frame, 62..68, 6..52, is_white),
        0,
        "the shape hides the first text"
    );
    assert!(
        count(&frame, 70..120, 24..52, is_white) > 20,
        "the second text shows over the shape"
    );
    assert_eq!(
        count(&frame, 126..160, 22..68, is_white),
        0,
        "the second shape hides the second text"
    );
    assert!(
        count(&frame, 126..160, 22..68, |pixel| pixel[2] > 200
            && pixel[0] < 60)
            > 1_000,
        "the second shape shows"
    );
}
