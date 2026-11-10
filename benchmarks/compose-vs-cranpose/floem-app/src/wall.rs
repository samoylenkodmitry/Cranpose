//! The still wall in Floem (`../README.md`): a window of tiles that never
//! change, under a clock line whose text changes every frame, with a cursor
//! outline that rests on another tile every frame.
//!
//! The tiles are one view that paints them from text shaped once. The clock is
//! a label on the frame signal. The cursor is a view whose style moves it by a
//! transform, so no frame lays anything out again but the clock's text.

use floem::{IntoView, peniko::Color, prelude::*};
use perf_data::*;

use crate::{
    frame::Frame,
    layer::layer,
    rgb,
    tiles::{TileGrid, TileKit},
    top_bar,
};

const BACKGROUND: Color = rgb(WALL_BACKGROUND_RGB);
const INK: Color = rgb(WALL_INK_RGB);

pub fn screen(launch: Launch, frame: Frame) -> impl IntoView {
    let tier = wall_tier(launch.tier);
    let (width, height) = DESKTOP_WINDOW;
    let grid = wall_grid(tier, width as f32, height as f32 - 56.0 - WALL_CLOCK_HEIGHT);
    let (columns, rows) = grid;
    let (tile_width, tile_height) = (tier.tile_width, tier.tile_height);
    let kit = TileKit::new(tile_height);
    let size = (
        columns as f32 * (tile_width + WALL_GAP) + WALL_GAP,
        rows as f32 * (tile_height + WALL_GAP) + WALL_GAP,
    );
    let tiles = TileGrid::new(
        &kit,
        grid,
        (tile_width, WALL_GAP),
        |index| index,
        |index| rgb(wall_fill_rgb(index)),
    );
    // The tiles never change, so they are drawn once and shown as a bitmap:
    // neither of Floem's renderers draws tens of thousands of them a frame.
    let tiles = layer(size, BACKGROUND, move |raster| tiles.draw(raster))
        .style(move |s| s.size(size.0, size.1).flex_shrink(0.0_f32));
    let cursor = empty().style(move |s| {
        let tile = wall_cursor(frame.get(), columns * rows);
        s.absolute()
            .inset_left(0.0)
            .inset_top(0.0)
            .size(tile_width, tile_height)
            .border(WALL_CURSOR_WIDTH)
            .border_color(INK)
            .border_radius(WALL_CORNER)
            .translate_x(WALL_GAP + (tile % columns) as f32 * (tile_width + WALL_GAP))
            .translate_y(WALL_GAP + (tile / columns) as f32 * (tile_height + WALL_GAP))
    });
    v_stack((
        top_bar("Still wall"),
        clock_line(frame),
        stack((tiles, cursor)).style(|s| s.flex_grow(1.0_f32).width_full()),
    ))
    .style(|s| s.size_full().background(BACKGROUND))
}

/// The clock line under the top bar.
pub fn clock_line(frame: Frame) -> impl IntoView {
    container(
        label(move || wall_clock(frame.get())).style(|s| s.font_size(WALL_CLOCK_TEXT).color(INK)),
    )
    .style(|s| {
        s.width_full()
            .height(WALL_CLOCK_HEIGHT)
            .flex_shrink(0.0_f32)
            .padding_horiz(8.0)
            .items_center()
    })
}
