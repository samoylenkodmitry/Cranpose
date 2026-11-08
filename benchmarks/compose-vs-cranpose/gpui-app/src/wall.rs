//! The still wall in GPUI (`../README.md`): a window of tiles that never
//! change, under a clock line whose text changes every frame, with a cursor
//! outline that rests on another tile every frame.
//!
//! In GPUI's idiom the window's view renders once per frame and asks for the
//! next frame. The tiles are a view of their own that the window embeds
//! cached: GPUI reuses its layout and painting from the frame before until
//! it is notified, which it never is. Only the clock and the cursor render
//! again each frame.

use gpui::{
    App, AppContext, Bounds, Context, Entity, FontWeight, Hsla, IntoElement, ParentElement, Render,
    StyleRefinement, Styled, TitlebarOptions, Window, WindowBounds, WindowOptions, div, px,
    relative, size,
};
use perf_data::*;

use super::{hex, rgb};

/// The tiles, `columns` by `rows` of them: a view that never changes.
struct Tiles {
    tier: WallTier,
    columns: usize,
    rows: usize,
}

impl Render for Tiles {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        let (width, height) = (self.tier.tile_width, self.tier.tile_height);
        let columns = self.columns;
        div()
            .flex()
            .flex_col()
            .gap(px(WALL_GAP))
            .p(px(WALL_GAP))
            .children((0..self.rows).map(move |row| {
                div().flex().gap(px(WALL_GAP)).children(
                    (0..columns).map(move |column| tile(row * columns + column, width, height)),
                )
            }))
    }
}

// One tile: its label over a dot and its value, on its fill.
fn tile(index: usize, width: f32, height: f32) -> impl IntoElement {
    let (label_size, value_size) = wall_text_size(height);
    let dot = wall_dot_size(height);
    div()
        .flex_none()
        .w(px(width))
        .h(px(height))
        .p(px(wall_padding(height)))
        .overflow_hidden()
        .whitespace_nowrap()
        .rounded(px(WALL_CORNER))
        .bg(rgb(wall_fill_rgb(index)))
        .flex()
        .flex_col()
        .child(
            div()
                .text_size(px(label_size))
                .font_weight(FontWeight::BOLD)
                .child(wall_label(index)),
        )
        .child(
            div()
                .flex()
                .items_center()
                .gap(px(dot * 0.5))
                .child(
                    div()
                        .flex_none()
                        .size(px(dot))
                        .rounded(px(dot / 2.0))
                        .bg(rgb(wall_dot_rgb(index))),
                )
                .child(div().text_size(px(value_size)).child(wall_value(index))),
        )
}

/// The window's view: the top bar, the clock, the cached tiles and the cursor.
struct Wall {
    tier: WallTier,
    frame: FrameIndex,
    tiles: Entity<Tiles>,
}

impl Render for Wall {
    fn render(&mut self, window: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        if self.frame.advance() {
            window.request_animation_frame();
        }
        let ink: Hsla = rgb(WALL_INK_RGB);
        let (width, height) = (self.tier.tile_width, self.tier.tile_height);
        let (columns, rows) = wall_grid_of_window(self.tier);
        let tile = wall_cursor(self.frame.get(), columns * rows);
        div()
            .size_full()
            .flex()
            .flex_col()
            .font_family("Roboto")
            .line_height(relative(1.4))
            .text_color(ink)
            .bg(rgb(WALL_BACKGROUND_RGB))
            .child(
                div()
                    .flex_none()
                    .h(px(56.0))
                    .px(px(16.0))
                    .flex()
                    .items_center()
                    .bg(hex(0x1E2A4A))
                    .text_size(px(20.0))
                    .font_weight(FontWeight::BOLD)
                    .text_color(gpui::white())
                    .child("Still wall"),
            )
            .child(
                div()
                    .flex_none()
                    .h(px(WALL_CLOCK_HEIGHT))
                    .px(px(8.0))
                    .flex()
                    .items_center()
                    .text_size(px(WALL_CLOCK_TEXT))
                    .child(wall_clock(self.frame.get())),
            )
            .child(
                div()
                    .flex_1()
                    .min_h(px(0.0))
                    .relative()
                    .child(
                        self.tiles
                            .clone()
                            .cached(StyleRefinement::default().size_full()),
                    )
                    .child(
                        div()
                            .absolute()
                            .left(px(WALL_GAP + (tile % columns) as f32 * (width + WALL_GAP)))
                            .top(px(WALL_GAP + (tile / columns) as f32 * (height + WALL_GAP)))
                            .w(px(width))
                            .h(px(height))
                            .border(px(WALL_CURSOR_WIDTH))
                            .border_color(ink)
                            .rounded(px(WALL_CORNER)),
                    ),
            )
    }
}

// The grid of the wall in the desktop window, under the top bar and the
// clock line.
fn wall_grid_of_window(tier: WallTier) -> (usize, usize) {
    let (width, height) = DESKTOP_WINDOW;
    wall_grid(tier, width as f32, height as f32 - 56.0 - WALL_CLOCK_HEIGHT)
}

/// Opens the still wall's window.
pub fn open(load: Launch, cx: &mut App) {
    let tier = wall_tier(load.tier);
    let (width, height) = DESKTOP_WINDOW;
    let bounds = Bounds::centered(None, size(px(width as f32), px(height as f32)), cx);
    let options = WindowOptions {
        window_bounds: Some(WindowBounds::Windowed(bounds)),
        titlebar: Some(TitlebarOptions {
            title: Some("Still wall".into()),
            ..Default::default()
        }),
        is_resizable: false,
        // The window draws every frame even when another window is in
        // front, as the other frameworks' windows do.
        inactive_frame_interval: None,
        ..Default::default()
    };
    if let Err(error) = cx.open_window(options, |_, cx| {
        let (columns, rows) = wall_grid_of_window(tier);
        let tiles = cx.new(|_| Tiles {
            tier,
            columns,
            rows,
        });
        cx.new(|_| Wall {
            tier,
            frame: FrameIndex::new(load.freeze),
            tiles,
        })
    }) {
        log::error!("no window: {error}");
    }
    cx.activate(true);
}
