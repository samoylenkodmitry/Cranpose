//! The still wall in FerroUI (`../README.md`): a window of tiles that never
//! change, under a clock line whose text changes every frame, with a cursor
//! outline that rests on another tile every frame. The tiles are made once;
//! the clock's text and the cursor's offset follow the frame signal.

use std::rc::Rc;

use ferroui_base::{
    Ref,
    layout::{HorizontalAlignment, Orientation, VerticalAlignment},
    media::{ITransform, TranslateTransform},
};
use ferroui_controls::Border;
use ferroui_view::prelude::*;
use perf_data::*;

use super::{hex, txt};

pub(crate) fn wall(frame: Signal<u32>) -> Element {
    let tier = wall_tier(perf_data::launch().tier);
    let (width, height) = DESKTOP_WINDOW;
    let (columns, rows) = wall_grid(tier, width as f32, height as f32 - 56.0 - WALL_CLOCK_HEIGHT);
    let ink = hex(WALL_INK_RGB);
    let tiles = stack_panel()
        .spacing(f64::from(WALL_GAP))
        .margin(f64::from(WALL_GAP))
        .children(
            (0..rows)
                .map(|line| {
                    stack_panel()
                        .orientation(Orientation::Horizontal)
                        .spacing(f64::from(WALL_GAP))
                        .children(
                            (0..columns)
                                .map(|column| tile(line * columns + column, tier))
                                .collect::<Vec<_>>(),
                        )
                })
                .collect::<Vec<_>>(),
        );
    let cursor = border()
        .width(f64::from(tier.tile_width))
        .height(f64::from(tier.tile_height))
        .horizontal_alignment(HorizontalAlignment::Left)
        .vertical_alignment(VerticalAlignment::Top)
        .border_thickness(f64::from(WALL_CURSOR_WIDTH))
        .border_brush(ink.as_str())
        .corner_radius(f64::from(WALL_CORNER))
        .with(move |control: &Ref<Border>| {
            let translate = TranslateTransform::new();
            let transform: Rc<dyn ITransform> = translate.clone().into();
            control.set_render_transform(Some(transform));
            effect(move || {
                let tile = wall_cursor(frame.get(), columns * rows);
                translate.set_x(f64::from(
                    WALL_GAP + (tile % columns) as f32 * (tier.tile_width + WALL_GAP),
                ));
                translate.set_y(f64::from(
                    WALL_GAP + (tile / columns) as f32 * (tier.tile_height + WALL_GAP),
                ));
            });
        });
    grid()
        .row_definitions(&format!("56,{WALL_CLOCK_HEIGHT},*"))
        .background(hex(WALL_BACKGROUND_RGB).as_str())
        .child(
            border()
                .background("#1E2A4A")
                .padding((16.0, 0.0, 16.0, 0.0))
                .child(
                    txt("Still wall", 20.0, "#FFFFFF", true)
                        .vertical_alignment(VerticalAlignment::Center),
                ),
        )
        .child(
            border()
                .padding((8.0, 0.0, 8.0, 0.0))
                .child(
                    txt(wall_clock(0), f64::from(WALL_CLOCK_TEXT), &ink, false)
                        .text(move || wall_clock(frame.get()))
                        .vertical_alignment(VerticalAlignment::Center),
                )
                .grid_row(1),
        )
        .child(grid().child(tiles).child(cursor).grid_row(2))
        .into()
}

/// One tile: its label over a dot and its value, on its fill, each part at
/// its own offset.
fn tile(index: usize, tier: WallTier) -> Element {
    let (label_size, value_size) = wall_text_size(tier.tile_height);
    let dot = f64::from(wall_dot_size(tier.tile_height));
    let padding = f64::from(wall_padding(tier.tile_height));
    let ink = hex(WALL_INK_RGB);
    let (label_line, value_line) = (f64::from(label_size) * 1.4, f64::from(value_size) * 1.4);
    let at = |x: f64, y: f64| (x, y, 0.0, 0.0);
    border()
        .width(f64::from(tier.tile_width))
        .height(f64::from(tier.tile_height))
        .corner_radius(f64::from(WALL_CORNER))
        .background(hex(wall_fill_rgb(index)).as_str())
        .child(
            grid()
                .child(
                    txt(wall_label(index), f64::from(label_size), &ink, true)
                        .horizontal_alignment(HorizontalAlignment::Left)
                        .vertical_alignment(VerticalAlignment::Top)
                        .margin(at(padding, padding)),
                )
                .child(
                    border()
                        .width(dot)
                        .height(dot)
                        .corner_radius(dot / 2.0)
                        .background(hex(wall_dot_rgb(index)).as_str())
                        .horizontal_alignment(HorizontalAlignment::Left)
                        .vertical_alignment(VerticalAlignment::Top)
                        .margin(at(padding, padding + label_line + (value_line - dot) / 2.0)),
                )
                .child(
                    txt(wall_value(index), f64::from(value_size), &ink, false)
                        .horizontal_alignment(HorizontalAlignment::Left)
                        .vertical_alignment(VerticalAlignment::Top)
                        .margin(at(padding + dot * 1.5, padding + label_line)),
                ),
        )
        .into()
}
