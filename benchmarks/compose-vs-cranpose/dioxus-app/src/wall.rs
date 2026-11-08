//! The still wall in Dioxus (`../README.md`): a window of tiles that never
//! change, under a clock line whose text changes every frame, with a cursor
//! outline that rests on another tile every frame.
//!
//! In Dioxus's idiom the tiles are a component that reads no signal, so it
//! renders once; the clock and the cursor are the components that read the
//! frame, so a frame renders those two and Dioxus sends the webview what they
//! changed. The page's CSS (`web-app/www/style.css`) lays the wall out in a
//! grid, as the web page does.

use dioxus::prelude::*;
use perf_data::*;

use super::{FRAME, hex, platform, use_frames};

/// Marks the page's body as the wall's, which its CSS colours.
const STILL_WALL: &str = "document.body.classList.add('still-wall')";

#[component]
pub(crate) fn WallApp() -> Element {
    use_frames(|_| {});
    use_hook(|| document::eval(STILL_WALL));
    let tier = wall_tier(perf_data::launch().tier);
    let (width, height) = platform::viewport();
    let (columns, rows) = wall_grid(tier, width as f32, height as f32 - 56.0 - WALL_CLOCK_HEIGHT);
    let (tile_width, tile_height) = (tier.tile_width, tier.tile_height);
    let (label, value) = wall_text_size(tile_height);
    let style = format!(
        "--width: {tile_width}px; --height: {tile_height}px; --label: {label}px; \
         --value: {value}px; --dot: {}px; --pad: {}px; --columns: {columns}",
        wall_dot_size(tile_height),
        wall_padding(tile_height),
    );
    let tiles = columns * rows;
    rsx! {
        header { class: "top-bar", "Still wall" }
        Clock {}
        div { class: "wall", style: "{style}",
            Tiles { tiles }
            Cursor { columns, tiles, width: tile_width, height: tile_height }
        }
    }
}

/// The line above the wall: the frame's number, new each frame.
#[component]
fn Clock() -> Element {
    rsx! {
        div { class: "wall-clock", "{wall_clock(FRAME())}" }
    }
}

/// The wall's `tiles`, which read no signal.
#[component]
fn Tiles(tiles: usize) -> Element {
    rsx! {
        for index in 0..tiles {
            div {
                key: "{index}",
                class: "wall-tile",
                background: "{hex(&wall_fill_rgb(index))}",
                div { class: "wall-label", "{wall_label(index)}" }
                div { class: "wall-row",
                    div { class: "wall-dot", background: "{hex(&wall_dot_rgb(index))}" }
                    div { class: "wall-value", "{wall_value(index)}" }
                }
            }
        }
    }
}

/// The cursor's outline over the tile `wall_cursor` names for the frame,
/// moved by its transform alone.
#[component]
fn Cursor(columns: usize, tiles: usize, width: f32, height: f32) -> Element {
    let tile = wall_cursor(FRAME(), tiles);
    let x = WALL_GAP + (tile % columns) as f32 * (width + WALL_GAP);
    let y = WALL_GAP + (tile / columns) as f32 * (height + WALL_GAP);
    rsx! {
        div {
            class: "wall-cursor",
            style: "border-width: {WALL_CURSOR_WIDTH}px; transform: translate({x}px, {y}px)",
        }
    }
}
