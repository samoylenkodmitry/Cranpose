//! The still wall in zgui (`../README.md`): the tiles are built once and never
//! change; the clock's text and the cursor's offset follow the frame signal.

use std::cell::RefCell;

use perf_data::*;
use zgui::{
    elements::{self as e, Box_, Column, Element},
    prelude::*,
    view::AnyView,
};

use crate::{run_frames, sheet::hex};

pub(crate) fn screen() -> AnyView {
    let tier = wall_tier(perf_data::launch().tier);
    let (width, height) = DESKTOP_WINDOW;
    let (columns, rows) = wall_grid(tier, width as f32, height as f32 - 56.0 - WALL_CLOCK_HEIGHT);
    let frame = RwSignal::new(0_u32);
    let index = RefCell::new(FrameIndex::new(perf_data::launch().freeze));
    run_frames(move || {
        let (current, frozen) = {
            let mut index = index.borrow_mut();
            index.advance();
            (index.get(), index.frozen())
        };
        if current != frame.get_untracked() {
            frame.set(current);
        }
        !frozen
    });
    let tiles = tiles(columns, rows, tier);
    let cursor = e::r#box()
        .class("cursor")
        .style_property("transform", move || {
            let tile = wall_cursor(frame.get(), columns * rows);
            Some(format!(
                "translate({}px, {}px)",
                WALL_GAP + (tile % columns) as f32 * (tier.tile_width + WALL_GAP),
                WALL_GAP + (tile / columns) as f32 * (tier.tile_height + WALL_GAP),
            ))
        });
    AnyView::new(
        page("Still wall", frame).child(e::r#box().class("board").child(tiles).child(cursor)),
    )
}

/// The page of a wall screen: the top bar titled `title` and the clock line
/// that follows `frame`, to which the caller adds the body.
pub(crate) fn page(title: &'static str, frame: RwSignal<u32>) -> Element<Column> {
    e::column()
        .class("page")
        .child(
            e::column()
                .class("top")
                .child(e::text().class("top-title").child(title)),
        )
        .child(
            e::column().class("clock").child(
                e::text()
                    .class("clock-text")
                    .child(move || wall_clock(frame.get())),
            ),
        )
}

/// The tiles of a wall `columns` by `rows`, one line of tiles to a row.
pub(crate) fn tiles(columns: usize, rows: usize, tier: WallTier) -> Element<Column> {
    e::column().class("tiles").children((0..rows).map(|line| {
        AnyView::new(e::row().class("line").children(
            (0..columns).map(move |column| AnyView::new(tile(line * columns + column, tier))),
        ))
    }))
}

/// One tile on its own fill.
pub(crate) fn tile(index: usize, tier: WallTier) -> Element<Box_> {
    tile_content(
        e::r#box().style_property("background-color", Some(hex(wall_fill_rgb(index)))),
        index,
        tier,
    )
}

/// The parts of tile `index` in `base`: its label over a dot and its value,
/// each at its own offset.
pub(crate) fn tile_content(base: Element<Box_>, index: usize, tier: WallTier) -> Element<Box_> {
    let (label_size, value_size) = wall_text_size(tier.tile_height);
    let dot = wall_dot_size(tier.tile_height);
    let padding = wall_padding(tier.tile_height);
    let (label_line, value_line) = (label_size * 1.4, value_size * 1.4);
    base.class("tile")
        .child(
            e::text()
                .class("tile-label")
                .style_text(Some(format!("left: {padding}px; top: {padding}px")))
                .child(wall_label(index)),
        )
        .child(e::r#box().class("tile-dot").style_text(Some(format!(
            "background-color: {}; left: {padding}px; top: {}px",
            hex(wall_dot_rgb(index)),
            padding + label_line + (value_line - dot) / 2.0,
        ))))
        .child(
            e::text()
                .class("tile-value")
                .style_text(Some(format!(
                    "left: {}px; top: {}px",
                    padding + dot * 1.5,
                    padding + label_line,
                )))
                .child(wall_value(index)),
        )
}
