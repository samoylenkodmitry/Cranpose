//! The still wall in iced (`../README.md`): a window of tiles that never
//! change, under a clock line whose text changes every frame, with a cursor
//! outline that rests on another tile every frame.
//!
//! In iced's idiom `view` builds the screen from the frame, as the Elm
//! architecture does. The tiles never change, so the wall is `lazy` on its
//! grid and iced builds its widgets once; the cursor is a box pinned over
//! the tile the frame names.

use iced::{
    Border, Color, Element, Length, Size, Subscription, Task, Theme, alignment,
    widget::{Column, Row, column, container, lazy, pin, row, space, stack, text},
};
use perf_data::*;

use super::{ROBOTO, ROBOTO_BOLD, TOP_BAR, fill, rgb, rounded};

const BACKGROUND: Color = rgb(WALL_BACKGROUND_RGB);
const INK: Color = rgb(WALL_INK_RGB);

#[derive(Debug, Clone, Copy)]
pub enum Message {
    Frame,
    /// The window's size, which the wall's grid is worked out in.
    Resized(Size),
}

pub struct Wall {
    tier: WallTier,
    window: Size,
    frame: FrameIndex,
}

impl Wall {
    pub fn new(load: Launch, window: Size) -> Self {
        Self {
            tier: wall_tier(load.tier),
            window,
            frame: FrameIndex::new(load.freeze),
        }
    }

    pub fn update(&mut self, message: Message) -> Task<Message> {
        match message {
            Message::Resized(size) => self.window = size,
            Message::Frame => {
                self.frame.advance();
            }
        }
        Task::none()
    }

    pub fn subscription(&self) -> Subscription<Message> {
        let resized = iced::window::resize_events().map(|(_, size)| Message::Resized(size));
        if self.frame.frozen() {
            resized
        } else {
            Subscription::batch([resized, iced::window::frames().map(|_| Message::Frame)])
        }
    }

    pub fn view(&self) -> Element<'_, Message> {
        let title = text("Still wall")
            .size(20)
            .font(ROBOTO_BOLD)
            .color(Color::WHITE);
        let top_bar = container(title)
            .height(56)
            .width(Length::Fill)
            .padding([0, 16])
            .align_y(alignment::Vertical::Center)
            .style(|_| fill(TOP_BAR));
        let clock = container(
            text(wall_clock(self.frame.get()))
                .size(WALL_CLOCK_TEXT)
                .color(INK),
        )
        .height(WALL_CLOCK_HEIGHT)
        .width(Length::Fill)
        .padding([0, 8])
        .align_y(alignment::Vertical::Center);
        let grid = wall_grid(
            self.tier,
            self.window.width,
            self.window.height - 56.0 - WALL_CLOCK_HEIGHT,
        );
        let (width, height) = (self.tier.tile_width, self.tier.tile_height);
        let wall = lazy(grid, move |&(columns, rows)| {
            tiles(columns, rows, width, height)
        });
        let tile = wall_cursor(self.frame.get(), grid.0 * grid.1);
        let cursor =
            pin(container(space())
                .width(width)
                .height(height)
                .style(|_| container::Style {
                    border: Border {
                        color: INK,
                        width: WALL_CURSOR_WIDTH,
                        radius: WALL_CORNER.into(),
                    },
                    ..container::Style::default()
                }))
            .x(WALL_GAP + (tile % grid.0) as f32 * (width + WALL_GAP))
            .y(WALL_GAP + (tile / grid.0) as f32 * (height + WALL_GAP));
        container(column![top_bar, clock, stack![wall, cursor]])
            .width(Length::Fill)
            .height(Length::Fill)
            .style(|_| fill(BACKGROUND))
            .into()
    }
}

// The tiles, `columns` by `rows` of them `width` by `height` points each,
// `WALL_GAP` apart.
pub(super) fn tiles(
    columns: usize,
    rows: usize,
    width: f32,
    height: f32,
) -> Element<'static, Message> {
    Column::with_children((0..rows).map(|row| {
        Row::with_children((0..columns).map(|column| tile(row * columns + column, width, height)))
            .spacing(WALL_GAP)
            .into()
    }))
    .spacing(WALL_GAP)
    .padding(WALL_GAP)
    .into()
}

// One tile: its label over a dot and its value, on its fill.
pub(super) fn tile(index: usize, width: f32, height: f32) -> Element<'static, Message> {
    tile_on(
        index,
        Size::new(width, height),
        height,
        rgb(wall_fill_rgb(index)),
    )
}

// Tile `index` of `size` on `fill`, with the content of a tile `height`
// points tall.
pub(super) fn tile_on(
    index: usize,
    size: Size,
    height: f32,
    fill_color: Color,
) -> Element<'static, Message> {
    let (label_size, value_size) = wall_text_size(height);
    let dot = wall_dot_size(height);
    let dot_color = rgb(wall_dot_rgb(index));
    let line = |content: String, size: f32| {
        text(content)
            .size(size)
            .color(INK)
            .wrapping(text::Wrapping::None)
    };
    container(column![
        line(wall_label(index), label_size).font(ROBOTO_BOLD),
        row![
            container(space())
                .width(dot)
                .height(dot)
                .style(move |_| rounded(dot_color, dot / 2.0)),
            line(wall_value(index), value_size).font(ROBOTO),
        ]
        .spacing(dot * 0.5)
        .align_y(alignment::Vertical::Center),
    ])
    .width(size.width)
    .height(size.height)
    .padding(wall_padding(height))
    .clip(true)
    .style(move |_| rounded(fill_color, WALL_CORNER))
    .into()
}

/// Runs the still wall in a window of `size`.
pub fn run(load: Launch, size: Size) -> iced::Result {
    let mut application =
        iced::application(move || Wall::new(load, size), Wall::update, Wall::view)
            .title("Still wall")
            .subscription(Wall::subscription)
            .window_size(size)
            .resizable(false)
            .theme(|_: &Wall| Theme::Dark)
            .default_font(ROBOTO);
    for file in ["Roboto-Regular.ttf", "Roboto-Bold.ttf"] {
        match perf_data::font_bytes(file) {
            Ok(bytes) => application = application.font(bytes),
            Err(error) => log::warn!("no {file}: {error}"),
        }
    }
    application.run()
}
