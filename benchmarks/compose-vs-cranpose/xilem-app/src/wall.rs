//! The still wall in Xilem (`../README.md`): a window of tiles that never
//! change, under a clock line whose text changes every frame, with a cursor
//! outline that rests on another tile every frame.
//!
//! In Xilem's idiom the app logic builds the tree of views from the frame.
//! The tiles are memoized on the grid, so Xilem skips them; the clock's label
//! and the cursor's holder, which moves by its transform alone, are the views
//! a frame changes.

use perf_data::*;
use xilem::{
    Color, Vec2, WidgetView,
    core::memoize,
    masonry::properties::types::UnitPoint,
    style::{Padding, Style as _},
    view::{
        CrossAxisAlignment, FlexExt as _, ZStackExt as _, flex_col, flex_row, label, sized_box,
        zstack,
    },
};

use super::{
    px, rgb, text, top_bar,
    widgets::{Hold, frame_clock, hold, ignore_height},
};

const BACKGROUND: Color = rgb(WALL_BACKGROUND_RGB);
const INK: Color = rgb(WALL_INK_RGB);

/// What the wall reads, which each frame advances.
pub(crate) struct Wall {
    frame: FrameIndex,
}

impl Wall {
    pub(crate) fn new() -> Self {
        Self {
            frame: FrameIndex::new(perf_data::launch().freeze),
        }
    }

    // Advances the frame unless the wall holds still; answers whether it did.
    fn advance(&mut self) -> bool {
        let shown = self.frame.get();
        self.frame.advance();
        self.frame.get() != shown
    }
}

pub(crate) fn wall_logic(wall: &mut Wall) -> impl WidgetView<Wall> + use<> {
    let tier = wall_tier(perf_data::launch().tier);
    let (width, height) = DESKTOP_WINDOW;
    let (columns, rows) = wall_grid(tier, width as f32, height as f32 - 56.0 - WALL_CLOCK_HEIGHT);
    let (tile_width, tile_height) = (tier.tile_width, tier.tile_height);
    let tile = wall_cursor(wall.frame.get(), columns * rows);
    let cursor = sized_box(label(""))
        .width(px(tile_width))
        .height(px(tile_height))
        .border(INK, f64::from(WALL_CURSOR_WIDTH))
        .corner_radius(f64::from(WALL_CORNER));
    flex_col((
        frame_clock(Wall::advance),
        top_bar("Still wall"),
        sized_box(hold(
            text(wall_clock(wall.frame.get()), WALL_CLOCK_TEXT, INK, false),
            Hold::Align(UnitPoint::LEFT),
            ignore_height,
        ))
        .expand_width()
        .height(px(WALL_CLOCK_HEIGHT))
        .padding(Padding::horizontal(8.0)),
        zstack((
            memoize(
                (columns, rows, tile_width, tile_height),
                |&(columns, rows, width, height)| tiles(columns, rows, width, height),
            ),
            hold(
                cursor,
                Hold::Turn {
                    offset: Vec2::new(
                        f64::from(WALL_GAP + (tile % columns) as f32 * (tile_width + WALL_GAP)),
                        f64::from(WALL_GAP + (tile / columns) as f32 * (tile_height + WALL_GAP)),
                    ),
                    radians: 0.0,
                },
                ignore_height,
            )
            .alignment(UnitPoint::TOP_LEFT),
        ))
        .alignment(UnitPoint::TOP_LEFT)
        .flex(1.0),
    ))
    .gap(px(0.0))
    .cross_axis_alignment(CrossAxisAlignment::Start)
    .background_color(BACKGROUND)
}

/// The tiles, `columns` by `rows` of them `width` by `height` points each,
/// `WALL_GAP` apart.
fn tiles(columns: usize, rows: usize, width: f32, height: f32) -> impl WidgetView<Wall> + use<> {
    flex_col(
        (0..rows)
            .map(|row| {
                flex_row(
                    (0..columns)
                        .map(|column| tile_view(row * columns + column, width, height))
                        .collect::<Vec<_>>(),
                )
                .gap(px(WALL_GAP))
            })
            .collect::<Vec<_>>(),
    )
    .gap(px(WALL_GAP))
    .cross_axis_alignment(CrossAxisAlignment::Start)
    .padding(f64::from(WALL_GAP))
}

/// One tile: its label over a dot and its value, on its fill.
fn tile_view(index: usize, width: f32, height: f32) -> impl WidgetView<Wall> + use<> {
    let (label_size, value_size) = wall_text_size(height);
    let dot = wall_dot_size(height);
    hold(
        sized_box(
            flex_col((
                text(wall_label(index), label_size, INK, true),
                flex_row((
                    sized_box(label(""))
                        .width(px(dot))
                        .height(px(dot))
                        .corner_radius(f64::from(dot / 2.0))
                        .background_color(rgb(wall_dot_rgb(index))),
                    text(wall_value(index), value_size, INK, false),
                ))
                .gap(px(dot * 0.5)),
            ))
            .gap(px(0.0))
            .cross_axis_alignment(CrossAxisAlignment::Start),
        )
        .width(px(width))
        .height(px(height))
        .padding(f64::from(wall_padding(height)))
        .corner_radius(f64::from(WALL_CORNER))
        .background_color(rgb(wall_fill_rgb(index))),
        Hold::Clamp {
            max: f64::from(height),
        },
        ignore_height,
    )
}
