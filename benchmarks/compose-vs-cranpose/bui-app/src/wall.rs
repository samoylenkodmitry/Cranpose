//! The still wall in BUI (`../README.md`): a window of tiles that never
//! change, under a clock line whose text changes every frame, with a cursor
//! outline that rests on another tile every frame.

use blenderui::prelude::*;
use perf_data::*;

use super::{BOLD, REGULAR, color, line, text, top_bar};

pub(crate) struct Wall {
    frame: FrameIndex,
}

impl Wall {
    pub(crate) fn new() -> Self {
        Self {
            frame: FrameIndex::new(perf_data::launch().freeze),
        }
    }
}

impl Render for Wall {
    fn render(&mut self, window: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        self.frame.advance();
        if !self.frame.frozen() {
            window.request_animation_frame();
        }
        let frame = self.frame.get();
        let tier = wall_tier(perf_data::launch().tier);
        let (width, height) = DESKTOP_WINDOW;
        let (columns, rows) =
            wall_grid(tier, width as f32, height as f32 - 56.0 - WALL_CLOCK_HEIGHT);
        let tile = wall_cursor(frame, columns * rows);
        let ink = color(WALL_INK_RGB);
        let cursor = div()
            .absolute()
            .left(px(
                WALL_GAP + (tile % columns) as f32 * (tier.tile_width + WALL_GAP)
            ))
            .top(px(
                WALL_GAP + (tile / columns) as f32 * (tier.tile_height + WALL_GAP)
            ))
            .w(px(tier.tile_width))
            .h(px(tier.tile_height))
            .box_border()
            .border_2()
            .border_color(ink)
            .rounded(px(WALL_CORNER));
        html().size_full().child(
            body()
                .m_0()
                .size_full()
                .flex()
                .flex_col()
                .bg(color(WALL_BACKGROUND_RGB))
                .child(top_bar("Still wall"))
                .child(
                    div()
                        .w_full()
                        .h(px(WALL_CLOCK_HEIGHT))
                        .flex_none()
                        .px(px(8.0))
                        .flex()
                        .items_center()
                        .child(text(wall_clock(frame), WALL_CLOCK_TEXT, REGULAR, ink)),
                )
                .child(
                    div()
                        .relative()
                        .flex_1()
                        .child(tiles(columns, rows, tier))
                        .child(cursor),
                ),
        )
    }
}

fn tiles(columns: usize, rows: usize, tier: WallTier) -> impl IntoElement {
    div()
        .p(px(WALL_GAP))
        .flex()
        .flex_col()
        .gap(px(WALL_GAP))
        .children((0..rows).map(move |row| {
            div()
                .flex()
                .gap(px(WALL_GAP))
                .children((0..columns).map(move |column| tile(row * columns + column, tier)))
        }))
}

/// One tile: its label over a dot and its value, on its fill, each part at
/// its own offset.
fn tile(index: usize, tier: WallTier) -> impl IntoElement {
    let (label_size, value_size) = wall_text_size(tier.tile_height);
    let dot = wall_dot_size(tier.tile_height);
    let padding = wall_padding(tier.tile_height);
    let ink = color(WALL_INK_RGB);
    let (label_line, value_line) = (label_size * 1.4, value_size * 1.4);
    div()
        .relative()
        .flex_none()
        .w(px(tier.tile_width))
        .h(px(tier.tile_height))
        .rounded(px(WALL_CORNER))
        .bg(color(wall_fill_rgb(index)))
        .child(line(
            wall_label(index),
            label_size,
            BOLD,
            ink,
            padding,
            padding,
        ))
        .child(
            div()
                .absolute()
                .left(px(padding))
                .top(px(padding + label_line + (value_line - dot) / 2.0))
                .w(px(dot))
                .h(px(dot))
                .rounded(px(dot / 2.0))
                .bg(color(wall_dot_rgb(index))),
        )
        .child(line(
            wall_value(index),
            value_size,
            REGULAR,
            ink,
            padding + dot * 1.5,
            padding + label_line,
        ))
}
