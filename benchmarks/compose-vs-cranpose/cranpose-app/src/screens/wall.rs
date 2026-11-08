//! The still wall (`../../../README.md`): a window of tiles that never
//! change, under a clock line whose text changes every frame, with a cursor
//! outline that rests on another tile every frame.
//!
//! In Cranpose's idiom the wall is one call whose arguments never change, so
//! no tile composes again; the clock is the one composable that reads the
//! frame while composing; the cursor moves in a graphics layer whose block
//! reads the frame, so its frame records nothing again.

use cranpose::prelude::*;

use super::text_style;
use crate::data;

/// The wall's tier and the frame it freezes on; 0 runs on.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct WallLoad {
    pub tier: usize,
    pub freeze: u32,
}

const fn rgb(rgb: [u8; 3]) -> Color {
    Color::from_rgb_u8(rgb[0], rgb[1], rgb[2])
}

const BACKGROUND: Color = rgb(data::WALL_BACKGROUND_RGB);
// A tile's text stays on one line and is cut at the tile's edge.
const ONE_LINE: TextOptions = TextOptions {
    overflow: TextOverflow::Clip,
    soft_wrap: false,
    max_lines: Some(1),
    min_lines: 1,
};
const INK: Color = rgb(data::WALL_INK_RGB);

#[composable]
pub fn WallScreen(load: WallLoad) {
    let tier = data::wall_tier(load.tier);
    let frame = rememberMutableStateOf(|| 0u32);
    let freeze = load.freeze;
    LaunchedEffect(freeze, async move |scope| {
        let clock = scope.runtime().frame_clock();
        let mut index = 0u32;
        loop {
            clock.next_frame().await;
            index += 1;
            frame.set(index);
            if freeze > 0 && index >= freeze {
                log::info!("PERF frozen frame={index}");
                break;
            }
        }
    });
    Column(
        Modifier::empty().fill_max_size().background(BACKGROUND),
        ColumnSpec::default(),
        move || {
            Clock(frame);
            BoxWithConstraints(
                Modifier::empty().fill_max_width().weight(1.0),
                move |scope| {
                    let (columns, rows) =
                        data::wall_grid(tier, scope.max_width().0, scope.max_height().0);
                    let (width, height) = (tier.tile_width, tier.tile_height);
                    Wall(columns, rows, width, height);
                    Cursor(frame, columns, columns * rows, width, height);
                },
            );
        },
    );
}

// The line above the wall: the frame's number, new each frame.
#[composable]
fn Clock(frame: MutableState<u32>) {
    Box(
        Modifier::empty()
            .fill_max_width()
            .height(data::WALL_CLOCK_HEIGHT)
            .padding_horizontal(8.0),
        BoxSpec::default().content_alignment(Alignment::CENTER_START),
        move || {
            Text(
                data::wall_clock(frame.get()),
                Modifier::empty(),
                text_style(data::WALL_CLOCK_TEXT, INK, false),
            );
        },
    );
}

// The tiles, `columns` by `rows` of them `width` by `height` points each,
// `WALL_GAP` apart.
#[composable]
fn Wall(columns: usize, rows: usize, width: f32, height: f32) {
    Column(
        Modifier::empty().padding(data::WALL_GAP),
        ColumnSpec::new().vertical_arrangement(LinearArrangement::spaced_by(data::WALL_GAP)),
        move || {
            for row in 0..rows {
                Row(
                    Modifier::empty(),
                    RowSpec::new()
                        .horizontal_arrangement(LinearArrangement::spaced_by(data::WALL_GAP)),
                    move || {
                        for column in 0..columns {
                            Tile(row * columns + column, width, height);
                        }
                    },
                );
            }
        },
    );
}

// One tile: its label over a dot and its value, on its fill.
#[composable]
fn Tile(index: usize, width: f32, height: f32) {
    let (label_size, value_size) = data::wall_text_size(height);
    let dot = data::wall_dot_size(height);
    Column(
        Modifier::empty()
            .size_points(width, height)
            .background(rgb(data::wall_fill_rgb(index)))
            .rounded_corners(data::WALL_CORNER)
            .padding(data::wall_padding(height)),
        ColumnSpec::default(),
        move || {
            TextWithOptions(
                data::wall_label(index),
                Modifier::empty(),
                text_style(label_size, INK, true),
                ONE_LINE,
            );
            Row(
                Modifier::empty(),
                RowSpec::new()
                    .horizontal_arrangement(LinearArrangement::spaced_by(dot * 0.5))
                    .vertical_alignment(VerticalAlignment::CenterVertically),
                move || {
                    Box(
                        Modifier::empty()
                            .size_points(dot, dot)
                            .background(rgb(data::wall_dot_rgb(index)))
                            .rounded_corners(dot * 0.5),
                        BoxSpec::default(),
                        || {},
                    );
                    TextWithOptions(
                        data::wall_value(index),
                        Modifier::empty(),
                        text_style(value_size, INK, false),
                        ONE_LINE,
                    );
                },
            );
        },
    );
}

// The cursor's outline, over the tile `wall_cursor` names for the frame: its
// layer block reads the frame, so the frame moves it with no composition,
// layout or drawing of its own.
#[composable]
fn Cursor(frame: MutableState<u32>, columns: usize, tiles: usize, width: f32, height: f32) {
    Box(
        Modifier::empty()
            .size_points(width, height)
            .graphics_layer(move || {
                let tile = data::wall_cursor(frame.get(), tiles);
                GraphicsLayer {
                    translation_x: data::WALL_GAP
                        + (tile % columns) as f32 * (width + data::WALL_GAP),
                    translation_y: data::WALL_GAP
                        + (tile / columns) as f32 * (height + data::WALL_GAP),
                    ..Default::default()
                }
            })
            .border(
                data::WALL_CURSOR_WIDTH,
                INK,
                RoundedCornerShape::uniform(data::WALL_CORNER),
            ),
        BoxSpec::default(),
        || {},
    );
}
