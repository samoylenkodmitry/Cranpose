//! The still wall in Rune (`../README.md`): the tiles are built once and
//! cached as one layer, the clock's text and the cursor's offset are live
//! values the frame sets without running the view again.

use perf_data::*;
use rune::{Color, Cx, El, Live, LiveText, Tx, col, el, row, text};

use super::{rgb, roboto};

pub(crate) struct Wall {
    frame: FrameIndex,
    clock: LiveText,
    cursor: Live<[f32; 2]>,
    columns: usize,
    rows: usize,
}

impl Wall {
    pub(crate) fn new() -> Self {
        let tier = wall_tier(perf_data::launch().tier);
        let (width, height) = DESKTOP_WINDOW;
        let (columns, rows) =
            wall_grid(tier, width as f32, height as f32 - 56.0 - WALL_CLOCK_HEIGHT);
        Self {
            frame: FrameIndex::new(perf_data::launch().freeze),
            clock: LiveText::new(wall_clock(0)),
            cursor: Live::new([0.0; 2]),
            columns,
            rows,
        }
    }
}

/// Moves the clock and the cursor to the next frame.
pub(crate) fn poll(wall: &mut Wall, _seconds: f64, tx: &mut Tx) -> bool {
    let before = wall.frame.get();
    wall.frame.advance();
    let frame = wall.frame.get();
    if frame != before {
        let tier = wall_tier(perf_data::launch().tier);
        let tile = wall_cursor(frame, wall.columns * wall.rows);
        wall.clock.set(wall_clock(frame));
        wall.cursor.set([
            WALL_GAP + (tile % wall.columns) as f32 * (tier.tile_width + WALL_GAP),
            WALL_GAP + (tile / wall.columns) as f32 * (tier.tile_height + WALL_GAP),
        ]);
    }
    if !wall.frame.frozen() {
        tx.wake_after(std::time::Duration::from_millis(1));
    }
    false
}

pub(crate) fn view(wall: &Wall, _: &mut Cx<Wall>) -> El {
    let tier = wall_tier(perf_data::launch().tier);
    let ink = rgb(WALL_INK_RGB);
    let top_bar = el()
        .h(56.0)
        .w_full()
        .px(16.0)
        .fill(rgb([0x1E, 0x2A, 0x4A]))
        .center_y()
        .child(
            text("Still wall")
                .font(roboto(20.0, true))
                .color(Color::WHITE),
        );
    let clock = el().h(WALL_CLOCK_HEIGHT).w_full().px(8.0).center_y().child(
        text(wall_clock(0))
            .live_text(&wall.clock)
            .font(roboto(WALL_CLOCK_TEXT, false))
            .color(ink),
    );
    let columns = wall.columns;
    let tiles = col()
        .gap(WALL_GAP)
        .padding(WALL_GAP)
        .cache()
        .children((0..wall.rows).map(move |line| {
            row()
                .gap(WALL_GAP)
                .children((0..columns).map(move |column| {
                    let index = line * columns + column;
                    tile(
                        index,
                        [tier.tile_width, tier.tile_height],
                        tier.tile_height,
                        rgb(wall_fill_rgb(index)),
                    )
                }))
        }));
    let cursor = el()
        .size(tier.tile_width, tier.tile_height)
        .radius(WALL_CORNER)
        .border(WALL_CURSOR_WIDTH, ink)
        .pin(Some(0.0), Some(0.0), None, None)
        .live_offset(&wall.cursor);
    col()
        .w_full()
        .h_full()
        .fill(rgb(WALL_BACKGROUND_RGB))
        .child(top_bar)
        .child(clock)
        .child(el().w_full().grow(1.0).child(tiles).child(cursor))
}

/// One tile of `size` points on `fill`: its parts, which follow `unit_height`,
/// the height of a single tile.
pub(crate) fn tile(index: usize, size: [f32; 2], unit_height: f32, fill: Color) -> El {
    el().size(size[0], size[1])
        .radius(WALL_CORNER)
        .fill(fill)
        .children(tile_parts(index, unit_height))
}

/// A tile's label over a dot and its value, each at its own offset.
pub(crate) fn tile_parts(index: usize, unit_height: f32) -> [El; 3] {
    let (label_size, value_size) = wall_text_size(unit_height);
    let dot = wall_dot_size(unit_height);
    let padding = wall_padding(unit_height);
    let ink = rgb(WALL_INK_RGB);
    let label_line = label_size * 1.4;
    let value_line = value_size * 1.4;
    [
        text(wall_label(index))
            .font(roboto(label_size, true))
            .color(ink)
            .absolute(padding, padding),
        el().size(dot, dot)
            .radius(dot / 2.0)
            .fill(rgb(wall_dot_rgb(index)))
            .absolute(padding, padding + label_line + (value_line - dot) / 2.0),
        text(wall_value(index))
            .font(roboto(value_size, false))
            .color(ink)
            .absolute(padding + dot * 1.5, padding + label_line),
    ]
}
