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
    App, AppContext, Bounds, Context, Div, Entity, FontWeight, Hsla, IntoElement, ParentElement,
    Render, StyleRefinement, Styled, TitlebarOptions, Window, WindowBounds, WindowOptions, div,
    prelude::FluentBuilder, px, relative, size,
};
use perf_data::*;

use super::{hex, rgb};

/// The tiles, `columns` by `rows` of them: a view that never changes. Without
/// `filled` the tiles show their text and dots over what lies beneath.
pub(super) struct Tiles {
    pub(super) tier: WallTier,
    pub(super) columns: usize,
    pub(super) rows: usize,
    pub(super) filled: bool,
}

impl Render for Tiles {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        let (width, height) = (self.tier.tile_width, self.tier.tile_height);
        let (columns, filled) = (self.columns, self.filled);
        div()
            .flex()
            .flex_col()
            .gap(px(WALL_GAP))
            .p(px(WALL_GAP))
            .children((0..self.rows).map(move |row| {
                div()
                    .flex()
                    .gap(px(WALL_GAP))
                    .children((0..columns).map(move |column| {
                        let index = row * columns + column;
                        tile_box(
                            index,
                            width,
                            height,
                            height,
                            filled.then(|| rgb(wall_fill_rgb(index))),
                        )
                    }))
            }))
    }
}

/// One tile on its own wall fill.
pub(super) fn tile(index: usize, width: f32, height: f32) -> Div {
    tile_box(
        index,
        width,
        height,
        height,
        Some(rgb(wall_fill_rgb(index))),
    )
}

/// The line height of the screens' text, in text sizes.
pub(super) const LINE_HEIGHT: f32 = 1.4;

/// Where the parts of a tile lie, in points from the tile's corner, for tiles
/// `tile_height` points tall: the label over a dot and the value. The element
/// tile puts its parts there, and so does a tile painted on a canvas.
#[derive(Clone, Copy)]
pub(super) struct TileGeometry {
    pub(super) label_size: f32,
    pub(super) value_size: f32,
    pub(super) label_line: f32,
    pub(super) value_line: f32,
    pub(super) dot: f32,
    pub(super) label: (f32, f32),
    pub(super) dot_at: (f32, f32),
    pub(super) value: (f32, f32),
}

impl TileGeometry {
    pub(super) fn new(tile_height: f32) -> Self {
        let (label_size, value_size) = wall_text_size(tile_height);
        let (label_line, value_line) = (label_size * LINE_HEIGHT, value_size * LINE_HEIGHT);
        let (padding, dot) = (wall_padding(tile_height), wall_dot_size(tile_height));
        // The dot and the value share a band, centered on one another.
        let band = dot.max(value_line);
        let top = padding + label_line;
        Self {
            label_size,
            value_size,
            label_line,
            value_line,
            dot,
            label: (padding, padding),
            dot_at: (padding, top + (band - dot) / 2.0),
            value: (padding + dot * 1.5, top + (band - value_line) / 2.0),
        }
    }
}

/// A tile's label over a dot and its value, in a box of `width` by `height`
/// on `fill`; the parts follow tiles `tile_height` points tall.
pub(super) fn tile_box(
    index: usize,
    width: f32,
    height: f32,
    tile_height: f32,
    fill: Option<Hsla>,
) -> Div {
    let geometry = TileGeometry::new(tile_height);
    let part = |(left, top): (f32, f32)| div().absolute().left(px(left)).top(px(top));
    div()
        .flex_none()
        .relative()
        .w(px(width))
        .h(px(height))
        .overflow_hidden()
        .whitespace_nowrap()
        .line_height(relative(LINE_HEIGHT))
        .rounded(px(WALL_CORNER))
        .when_some(fill, |tile, fill| tile.bg(fill))
        .child(
            part(geometry.label)
                .text_size(px(geometry.label_size))
                .font_weight(FontWeight::BOLD)
                .child(wall_label(index)),
        )
        .child(
            part(geometry.dot_at)
                .size(px(geometry.dot))
                .rounded(px(geometry.dot / 2.0))
                .bg(rgb(wall_dot_rgb(index))),
        )
        .child(
            part(geometry.value)
                .text_size(px(geometry.value_size))
                .child(wall_value(index)),
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
            .line_height(relative(LINE_HEIGHT))
            .text_color(ink)
            .bg(rgb(WALL_BACKGROUND_RGB))
            .child(top_bar("Still wall"))
            .child(clock(self.frame.get()))
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

/// The bar with the screen's title.
pub(super) fn top_bar(title: &'static str) -> Div {
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
        .child(title)
}

/// The clock line under the top bar.
pub(super) fn clock(frame: u32) -> Div {
    div()
        .flex_none()
        .h(px(WALL_CLOCK_HEIGHT))
        .px(px(8.0))
        .flex()
        .items_center()
        .text_size(px(WALL_CLOCK_TEXT))
        .child(wall_clock(frame))
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
            filled: true,
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
