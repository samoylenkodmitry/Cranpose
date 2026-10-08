//! The gauntlet in iced: the screen `compose-app/.../Gauntlet.kt` and
//! `cranpose-app/src/screens/gauntlet.rs` draw, element for element, in iced's
//! own idiom. The benchmark's README describes it; `desktop.py` runs it:
//! `PERF_TIER=12 PERF_FONTS=../fonts cargo run --release`, and a page in a
//! browser runs it from `index.html`.
//!
//! Every frame advances a frame index and everything follows from it, never
//! from wall time. iced's frame subscription advances it once per frame, and
//! `view` builds the screen from it, as the Elm architecture does.

use std::{cell::OnceCell, collections::HashMap};

use iced::{
    Background, Border, Color, Element, Font, Length, Pixels, Point, Rectangle, Renderer, Shadow,
    Size, Subscription, Task, Theme, Vector,
    advanced::text::{self as core_text, Paragraph as _},
    alignment, font,
    mouse::Cursor,
    widget::{
        Column, Row, canvas, column, container, image, operation, progress_bar, rich_text, row,
        rule, scrollable, sensor, span, stack, text,
    },
};
use perf_data::*;

/// Blocks of five card rows and a cluster: no measurement window reaches the end.
const ROWS: usize = 2000 * (CARD_ROWS_PER_CLUSTER + 1);
/// How far the list scrolls each frame, in logical pixels.
const SCROLL_PER_FRAME: f32 = 3.0;
/// The most rows one frame builds below the anchor.
const MAX_ROWS: usize = 64;

const fn rgb(value: [u8; 3]) -> Color {
    Color::from_rgb8(value[0], value[1], value[2])
}

const INK: Color = rgb([0x11, 0x18, 0x27]);
const BODY: Color = rgb([0x37, 0x41, 0x51]);
const MUTED: Color = rgb([0x6B, 0x72, 0x80]);
const HAIRLINE: Color = rgb([0xE5, 0xE7, 0xEB]);
const PANEL: Color = rgb([0xE2, 0xE8, 0xF0]);
const BACKGROUND: Color = rgb([0xEE, 0xF0, 0xF5]);
const TOP_BAR: Color = rgb([0x1E, 0x2A, 0x4A]);
const UP: Color = rgb([0x16, 0xA3, 0x4A]);
const DOWN: Color = rgb([0xDC, 0x26, 0x26]);
const LEVEL_BACKGROUND: [Color; 2] = [rgb([0xF1, 0xF5, 0xF9]), rgb([0xCB, 0xD5, 0xE1])];
const LAYER_BACKGROUND: Color = Color::from_rgba8(0x1E, 0x29, 0x3B, 0xC0 as f32 / 255.0);
const NESTED_BACKGROUND: Color = Color::from_rgba8(0xFF, 0xFF, 0xFF, 0xE6 as f32 / 255.0);
const FOOTER_LABELS: [&str; 3] = ["likes", "replies", "shares"];

const ROBOTO: Font = Font::with_name("Roboto");
const ROBOTO_BOLD: Font = Font {
    weight: font::Weight::Bold,
    ..ROBOTO
};

fn palette(index: usize) -> Color {
    rgb(PALETTE_RGB[index % PALETTE_RGB.len()])
}

#[derive(Debug, Clone, Copy)]
enum Message {
    Frame,
    /// The window's size, which the screen and its list are laid out in.
    Resized(Size),
    /// A row's height, from the sensor around it.
    Measured(usize, f32),
}

struct Gauntlet {
    load: Launch,
    window: Size,
    tier: GauntletTier,
    frame: u32,
    frozen: bool,
    first_frame_logged: bool,
    posts: Vec<Post>,
    tickers: Vec<Ticker>,
    avatars: Vec<image::Handle>,
    /// The first row the list builds and its top in the list's content.
    anchor: (usize, f32),
    /// The heights rows reported, from the anchor down.
    heights: HashMap<usize, f32>,
    /// The badge's size, measured once.
    badge: OnceCell<Size>,
    /// The heights of a stacked panel's 13 px and 11 px lines, measured once.
    layer_lines: OnceCell<(f32, f32)>,
    list: iced::widget::Id,
}

impl Gauntlet {
    fn new(load: Launch, window: Size) -> Self {
        let tier = gauntlet_tier(load.tier);
        Self {
            load,
            window,
            tier,
            frame: 0,
            frozen: false,
            first_frame_logged: false,
            posts: posts(),
            tickers: tickers(tier.tickers),
            avatars: (0..AVATAR_COUNT)
                .map(|index| image::Handle::from_rgba(AVATAR_SIZE, AVATAR_SIZE, avatar_rgba(index)))
                .collect(),
            anchor: (0, 8.0 * tier.scale),
            heights: HashMap::new(),
            badge: OnceCell::new(),
            layer_lines: OnceCell::new(),
            list: iced::widget::Id::unique(),
        }
    }

    fn s(&self) -> f32 {
        self.tier.scale
    }

    fn offset(&self) -> f32 {
        self.frame as f32 * SCROLL_PER_FRAME
    }

    fn update(&mut self, message: Message) -> Task<Message> {
        match message {
            Message::Resized(size) => {
                self.window = size;
                Task::none()
            }
            Message::Measured(row, height) => {
                if row >= self.anchor.0 {
                    self.heights.insert(row, height);
                }
                Task::none()
            }
            Message::Frame => {
                if !self.first_frame_logged {
                    self.first_frame_logged = true;
                    log::info!("PERF first_frame");
                }
                if self.frozen {
                    return Task::none();
                }
                self.frame += 1;
                if self.load.freeze > 0 && self.frame >= self.load.freeze {
                    self.frozen = true;
                    log::info!("PERF frozen frame={}", self.frame);
                }
                // Rows that scrolled off above are built no more.
                let gap = 8.0 * self.s();
                let offset = self.offset();
                while let Some(&height) = self.heights.get(&self.anchor.0) {
                    let (row, top) = self.anchor;
                    if top + height + gap > offset {
                        break;
                    }
                    self.heights.remove(&row);
                    self.anchor = (row + 1, top + height + gap);
                }
                operation::scroll_to(
                    self.list.clone(),
                    scrollable::AbsoluteOffset {
                        x: 0.0,
                        y: offset - self.anchor.1,
                    },
                )
            }
        }
    }

    fn subscription(&self) -> Subscription<Message> {
        let resized = iced::window::resize_events().map(|(_, size)| Message::Resized(size));
        if self.frozen {
            resized
        } else {
            Subscription::batch([resized, iced::window::frames().map(|_| Message::Frame)])
        }
    }

    fn view(&self) -> Element<'_, Message> {
        let title = text("Gauntlet")
            .size(20)
            .font(ROBOTO_BOLD)
            .color(Color::WHITE);
        let top_bar = container(title)
            .height(56)
            .width(Length::Fill)
            .padding([0, 16])
            .align_y(alignment::Vertical::Center)
            .style(|_| fill(TOP_BAR));
        // The panels stack over the list and show only over it, as every
        // app clips them.
        let layers = canvas(Layers {
            layers: self.tier.layers,
            rows: self.tier.layer_rows,
            frame: self.frame,
            lines: &self.layer_lines,
        })
        .width(Length::Fill)
        .height(Length::Fill);
        let content = column![
            self.ticker_panel(),
            container(stack![self.list(), layers]).clip(true)
        ]
        .width(self.window.width * width_fraction(self.frame));
        container(column![top_bar, content])
            .width(Length::Fill)
            .height(Length::Fill)
            .style(|_| fill(BACKGROUND))
            .into()
    }

    fn ticker_panel(&self) -> Element<'_, Message> {
        let s = self.s();
        let tiles = self
            .tickers
            .iter()
            .enumerate()
            .map(|(index, ticker)| self.ticker(index, ticker));
        container(
            Row::with_children(tiles)
                .spacing(4.0 * s)
                .wrap()
                .vertical_spacing(4.0 * s),
        )
        .padding(6.0 * s)
        .width(Length::Fill)
        .style(|_| fill(PANEL))
        .into()
    }

    fn ticker(&self, index: usize, ticker: &Ticker) -> Element<'_, Message> {
        let s = self.s();
        let cents = ticker_cents(ticker, self.frame);
        let change = if cents >= ticker.base_cents { UP } else { DOWN };
        let share = ((cents - ticker.base_cents + ticker.swing_cents) as f32
            / (2 * ticker.swing_cents) as f32)
            .clamp(0.0, 1.0);
        let label =
            |content: String, color, font| text(content).size(10.0 * s).color(color).font(font);
        let tile = row![
            label(ticker.symbol.clone(), INK, ROBOTO_BOLD),
            label(cents_text(cents), BODY, ROBOTO),
            label(change_text(ticker, cents), change, ROBOTO),
            bar(share, palette(index), 20.0 * s, 4.0 * s),
        ]
        .spacing(4.0 * s)
        .align_y(alignment::Vertical::Center);
        container(tile)
            .padding([3.0 * s, 6.0 * s])
            .style(move |_| rounded(Color::WHITE, 6.0 * s))
            .into()
    }

    /// The rows from the anchor down to the bottom of the window. Each row
    /// reports its height, which moves the anchor once the row scrolled off.
    fn list(&self) -> Element<'_, Message> {
        let s = self.s();
        let gap = 8.0 * s;
        let bottom = self.offset() - self.anchor.1 + self.window.height;
        let (mut row, mut top) = (self.anchor.0, 0.0);
        let mut rows = Column::new().spacing(gap).padding([0.0, gap]);
        while row < ROWS && top < bottom && row < self.anchor.0 + MAX_ROWS {
            let content = sensor(self.row(row))
                .key(row)
                .on_show(move |size| Message::Measured(row, size.height))
                .on_resize(move |size| Message::Measured(row, size.height));
            rows = rows.push(content);
            // A row not yet measured counts as a line of text, so the frame
            // builds enough of them.
            top += self.heights.get(&row).copied().unwrap_or(10.0 * s) + gap;
            row += 1;
        }
        scrollable(rows)
            .id(self.list.clone())
            .direction(scrollable::Direction::Vertical(
                scrollable::Scrollbar::hidden(),
            ))
            .height(Length::Fill)
            .into()
    }

    fn row(&self, row: usize) -> Element<'_, Message> {
        match gauntlet_row(row, self.tier.columns) {
            GauntletRow::Cards(first) => {
                Row::with_children((first..first + self.tier.columns).map(|card| {
                    container(self.card(card))
                        .width(Length::FillPortion(1))
                        .into()
                }))
                .spacing(8.0 * self.s())
                .into()
            }
            GauntletRow::Cluster(cluster) => self.level(cluster, self.tier.depth),
        }
    }

    fn card(&self, card: usize) -> Element<'_, Message> {
        let s = self.s();
        let post = &self.posts[card % POST_COUNT];
        let content = column![
            self.card_header(post, card),
            paragraph(post.body.clone(), 12.0 * s, BODY, ROBOTO, 4),
            self.progress(card),
            canvas(Sparkline {
                card,
                frame: self.frame,
                color: palette(post.color),
                stroke: 1.5 * s,
            })
            .width(Length::Fill)
            .height(36.0 * s),
            self.chips(post),
            self.footer(post),
        ]
        .spacing(6.0 * s);
        // Compose draws its border over the padding: the content is inset by
        // the padding alone, as an iced border is.
        let body = container(content)
            .padding(10.0 * s)
            .width(Length::Fill)
            .style(move |_| container::Style {
                background: Some(Background::Color(Color::WHITE)),
                border: Border {
                    color: HAIRLINE,
                    width: 1.0,
                    radius: (12.0 * s).into(),
                },
                shadow: Shadow {
                    color: Color::from_rgba8(0, 0, 0, 0.24),
                    offset: Vector::new(0.0, s),
                    blur_radius: 3.0 * s,
                },
                ..container::Style::default()
            });
        if !card.is_multiple_of(5) {
            return body.into();
        }
        // A translucent tag tilting with the frame: drawn rotated over the
        // card, never laid out again.
        let badge = canvas(Badge {
            degrees: badge_degrees(card, self.frame),
            s,
            size: &self.badge,
        })
        .width(Length::Fill)
        .height(Length::Fill);
        stack![body, badge].into()
    }

    fn card_header<'a>(&'a self, post: &'a Post, card: usize) -> Element<'a, Message> {
        let s = self.s();
        let avatar = image(self.avatars[card % AVATAR_COUNT].clone())
            .width(32.0 * s)
            .height(32.0 * s)
            .border_radius(16.0 * s);
        let (tag, tag_color) = &post.tags[0];
        // `by Ada Lovelace · #rust`: the author bold, the tag in its color.
        let subtitle: text::Rich<'_, (), Message> = rich_text![
            span("by "),
            span(&post.author).font(ROBOTO_BOLD),
            span(" · "),
            span(tag).color(rgb(GRADIENT_END_RGB[*tag_color])),
        ]
        .size(11.0 * s)
        .color(MUTED)
        .wrapping(text::Wrapping::None);
        let heading = column![
            paragraph(post.title.clone(), 13.0 * s, INK, ROBOTO_BOLD, 2),
            container(subtitle).clip(true),
        ];
        row![avatar, heading]
            .spacing(8.0 * s)
            .align_y(alignment::Vertical::Center)
            .into()
    }

    fn progress(&self, card: usize) -> Element<'_, Message> {
        let s = self.s();
        let permille = progress_permille(card, self.frame);
        row![
            container(bar(
                permille as f32 / 1000.0,
                palette(card),
                Length::Fill,
                6.0 * s
            ))
            .width(Length::Fill),
            text(format!("{}%", permille / 10))
                .size(10.0 * s)
                .color(MUTED),
        ]
        .spacing(6.0 * s)
        .align_y(alignment::Vertical::Center)
        .into()
    }

    fn chips<'a>(&'a self, post: &'a Post) -> Element<'a, Message> {
        let s = self.s();
        let chips = post.tags.iter().map(|(tag, color)| {
            let (background, foreground) = (
                rgb(CHIP_BACKGROUND_RGB[*color]),
                rgb(GRADIENT_END_RGB[*color]),
            );
            container(text(tag).size(10.0 * s).color(foreground))
                .padding([3.0 * s, 8.0 * s])
                .style(move |_| rounded(background, 10.0 * s))
                .into()
        });
        Row::with_children(chips)
            .spacing(4.0 * s)
            .wrap()
            .vertical_spacing(4.0 * s)
            .into()
    }

    /// Three counters split by dividers as tall as the row.
    fn footer<'a>(&'a self, post: &'a Post) -> Element<'a, Message> {
        let s = self.s();
        let mut footer = Row::new();
        for (index, label) in FOOTER_LABELS.iter().enumerate() {
            if index > 0 {
                footer = footer.push(rule::vertical(1).style(|_| rule::Style {
                    color: HAIRLINE,
                    radius: 0.0.into(),
                    fill_mode: rule::FillMode::Full,
                    snap: true,
                }));
            }
            let counter = column![
                text(&post.stats[index])
                    .size(12.0 * s)
                    .font(ROBOTO_BOLD)
                    .color(INK),
                text(*label).size(9.0 * s).color(MUTED),
            ]
            .align_x(alignment::Horizontal::Center)
            .width(Length::Fill);
            footer = footer.push(counter);
        }
        footer.height(Length::Shrink).into()
    }

    /// Levels nested inside one another, each a row of three labels above the
    /// next level.
    fn level(&self, cluster: usize, remaining: usize) -> Element<'_, Message> {
        let s = self.s();
        let chips = (0..3).map(|chip| {
            let color = palette(remaining + chip + cluster);
            container(
                text(format!("C{cluster}.L{remaining}.{chip}"))
                    .size(9.0 * s)
                    .color(Color::WHITE),
            )
            .width(Length::FillPortion(1))
            .style(move |_| rounded(color, 2.0 * s))
            .into()
        });
        let mut content = column![Row::with_children(chips).spacing(2.0 * s)].spacing(s);
        if remaining > 0 {
            content = content.push(self.level(cluster, remaining - 1));
        }
        let background = LEVEL_BACKGROUND[remaining % 2];
        container(content)
            .padding(iced::Padding {
                top: s,
                right: s,
                bottom: s,
                left: 3.0 * s,
            })
            .width(Length::Fill)
            .style(move |_| rounded(background, 4.0 * s))
            .into()
    }
}

fn fill(color: Color) -> container::Style {
    container::Style::default().background(color)
}

fn rounded(color: Color, radius: f32) -> container::Style {
    container::Style::default()
        .background(color)
        .border(Border::default().rounded(radius))
}

/// A rounded track filled to `share` in `color`.
fn bar<'a>(
    share: f32,
    color: Color,
    length: impl Into<Length>,
    girth: f32,
) -> Element<'a, Message> {
    progress_bar(0.0..=1.0, share)
        .length(length)
        .girth(girth)
        .style(move |_| progress_bar::Style {
            background: Background::Color(HAIRLINE),
            bar: Background::Color(color),
            border: Border::default().rounded(girth / 2.0),
        })
        .into()
}

/// Lines 1.4 em apart, cut after `max_lines`: iced text has no ellipsis, so a
/// box as tall as those lines clips the rest.
fn paragraph<'a>(
    content: String,
    size: f32,
    color: Color,
    font: Font,
    max_lines: usize,
) -> Element<'a, Message> {
    let line = size * 1.4;
    container(
        text(content)
            .size(size)
            .line_height(text::LineHeight::Absolute(Pixels(line)))
            .color(color)
            .font(font),
    )
    .max_height(line * max_lines as f32)
    .clip(true)
    .into()
}

/// A card's 48-point line over a fading fill, drawn from the frame.
struct Sparkline {
    card: usize,
    frame: u32,
    color: Color,
    stroke: f32,
}

impl canvas::Program<Message> for Sparkline {
    type State = ();

    fn draw(
        &self,
        _: &(),
        renderer: &Renderer,
        _: &Theme,
        bounds: Rectangle,
        _: Cursor,
    ) -> Vec<canvas::Geometry> {
        let mut frame = canvas::Frame::new(renderer, bounds.size());
        let (width, height) = (bounds.width, bounds.height);
        let step = width / (GAUNTLET_SPARK_POINTS - 1) as f32;
        let point = |index: usize| {
            Point::new(
                index as f32 * step,
                height * (1.0 - spark_value(self.card, index, self.frame)),
            )
        };
        let area = canvas::Path::new(|path| {
            path.move_to(Point::new(0.0, height));
            for index in 0..GAUNTLET_SPARK_POINTS {
                path.line_to(point(index));
            }
            path.line_to(Point::new(width, height));
            path.close();
        });
        let line = canvas::Path::new(|path| {
            path.move_to(point(0));
            for index in 1..GAUNTLET_SPARK_POINTS {
                path.line_to(point(index));
            }
        });
        let fade = canvas::gradient::Linear::new(Point::ORIGIN, Point::new(0.0, height))
            .add_stop(0.0, self.color.scale_alpha(0.25))
            .add_stop(1.0, Color::TRANSPARENT);
        frame.fill(&area, fade);
        frame.stroke(
            &line,
            canvas::Stroke::default()
                .with_color(self.color)
                .with_width(self.stroke),
        );
        vec![frame.into_geometry()]
    }
}

/// The `HOT` tag in a card's top right corner, rotated about its centre.
struct Badge<'a> {
    degrees: f32,
    s: f32,
    size: &'a OnceCell<Size>,
}

impl canvas::Program<Message> for Badge<'_> {
    type State = ();

    fn draw(
        &self,
        _: &(),
        renderer: &Renderer,
        _: &Theme,
        bounds: Rectangle,
        _: Cursor,
    ) -> Vec<canvas::Geometry> {
        let s = self.s;
        let label = *self
            .size
            .get_or_init(|| measure("HOT", 9.0 * s, ROBOTO_BOLD));
        let size = Size::new(label.width + 12.0 * s, label.height + 4.0 * s);
        let center = Point::new(
            bounds.width - 6.0 * s - size.width / 2.0,
            6.0 * s + size.height / 2.0,
        );
        let mut frame = canvas::Frame::new(renderer, bounds.size());
        frame.translate(Vector::new(center.x, center.y));
        frame.rotate(self.degrees.to_radians());
        let corner = Point::new(-size.width / 2.0, -size.height / 2.0);
        let radius = (8.0 * s).min(size.height / 2.0);
        frame.fill(
            &canvas::Path::rounded_rectangle(corner, size, radius.into()),
            palette(0).scale_alpha(0.9),
        );
        frame.fill_text(canvas::Text {
            content: "HOT".to_owned(),
            position: Point::new(corner.x + 6.0 * s, corner.y + 2.0 * s),
            color: Color::WHITE.scale_alpha(0.9),
            size: Pixels(9.0 * s),
            font: ROBOTO_BOLD,
            ..canvas::Text::default()
        });
        vec![frame.into_geometry()]
    }
}

/// The size of one line of `content`.
fn measure(content: &str, size: f32, font: Font) -> Size {
    <Renderer as core_text::Renderer>::Paragraph::with_text(core_text::Text {
        content,
        bounds: Size::INFINITE,
        size: Pixels(size),
        line_height: text::LineHeight::default(),
        font,
        align_x: core_text::Alignment::Left,
        align_y: alignment::Vertical::Top,
        shaping: core_text::Shaping::Basic,
        wrapping: core_text::Wrapping::None,
    })
    .min_bounds()
}

/// The translucent panels stacked over the list, drawn again each frame as
/// the view is built again: each panel turned about its centre, and its
/// nested card turned back about its own.
struct Layers<'a> {
    layers: usize,
    rows: usize,
    frame: u32,
    lines: &'a OnceCell<(f32, f32)>,
}

impl canvas::Program<Message> for Layers<'_> {
    type State = ();

    fn draw(
        &self,
        _: &(),
        renderer: &Renderer,
        _: &Theme,
        bounds: Rectangle,
        _: Cursor,
    ) -> Vec<canvas::Geometry> {
        let (title, line) = *self.lines.get_or_init(|| {
            (
                measure("Layer", 13.0, ROBOTO_BOLD).height,
                measure("Nested", 11.0, ROBOTO).height,
            )
        });
        let label =
            |content: String, position: Point, color: Color, size: f32, font: Font| canvas::Text {
                content,
                position,
                color,
                size: Pixels(size),
                font,
                ..canvas::Text::default()
            };
        let cells_top = 10.0 + title + 8.0;
        let nested_top = cells_top + self.rows as f32 * 25.0 - 3.0 + 8.0;
        let nested = Size::new(200.0, 8.0 + line + 2.0 + line + 8.0);
        let panel = Size::new(220.0, nested_top + nested.height + 10.0);
        let mut frame = canvas::Frame::new(renderer, bounds.size());
        for layer in 0..self.layers {
            let angle = layer_degrees(layer, self.frame).to_radians();
            frame.with_save(|frame| {
                frame.translate(Vector::new(
                    layer_x(layer, self.frame) + panel.width / 2.0,
                    layer_y(layer, self.frame) + panel.height / 2.0,
                ));
                frame.rotate(angle);
                frame.translate(Vector::new(-panel.width / 2.0, -panel.height / 2.0));
                frame.fill(
                    &canvas::Path::rounded_rectangle(Point::ORIGIN, panel, 12.0.into()),
                    LAYER_BACKGROUND,
                );
                frame.fill_text(label(
                    format!("Layer {}", layer + 1),
                    Point::new(10.0, 10.0),
                    Color::WHITE,
                    13.0,
                    ROBOTO_BOLD,
                ));
                for cell in 0..self.rows * LAYER_COLUMNS {
                    let corner = Point::new(
                        10.0 + (cell % LAYER_COLUMNS) as f32 * 25.0,
                        cells_top + (cell / LAYER_COLUMNS) as f32 * 25.0,
                    );
                    frame.fill(
                        &canvas::Path::rounded_rectangle(corner, Size::new(22.0, 22.0), 4.0.into()),
                        palette(layer_cell_color(layer, cell)),
                    );
                    frame.fill_text(canvas::Text {
                        align_x: core_text::Alignment::Center,
                        align_y: alignment::Vertical::Center,
                        ..label(
                            (cell + 1).to_string(),
                            corner + Vector::new(11.0, 11.0),
                            Color::WHITE,
                            9.0,
                            ROBOTO,
                        )
                    });
                }
                frame.translate(Vector::new(
                    10.0 + nested.width / 2.0,
                    nested_top + nested.height / 2.0,
                ));
                frame.rotate(-angle);
                frame.translate(Vector::new(-nested.width / 2.0, -nested.height / 2.0));
                frame.fill(
                    &canvas::Path::rounded_rectangle(Point::ORIGIN, nested, 8.0.into()),
                    NESTED_BACKGROUND,
                );
                frame.fill_text(label(
                    format!("Nested in layer {}", layer + 1),
                    Point::new(8.0, 8.0),
                    INK,
                    11.0,
                    ROBOTO_BOLD,
                ));
                frame.fill_text(label(
                    "Tilts against its panel".to_owned(),
                    Point::new(8.0, 8.0 + line + 2.0),
                    BODY,
                    11.0,
                    ROBOTO,
                ));
            });
        }
        vec![frame.into_geometry()]
    }
}

/// Runs the gauntlet in a window of `size` that the page or the desktop
/// gives it.
pub fn run(load: Launch, size: Size) -> iced::Result {
    let mut application = iced::application(
        move || Gauntlet::new(load, size),
        Gauntlet::update,
        Gauntlet::view,
    )
    .title("Gauntlet")
    .subscription(Gauntlet::subscription)
    .window_size(size)
    .resizable(false)
    .theme(|_: &Gauntlet| Theme::Light)
    .default_font(ROBOTO);
    for file in ["Roboto-Regular.ttf", "Roboto-Bold.ttf"] {
        match perf_data::font_bytes(file) {
            Ok(bytes) => application = application.font(bytes),
            Err(error) => log::warn!("no {file}: {error}"),
        }
    }
    application.run()
}

/// Draws the gauntlet in the page: iced adds its canvas to the body, and the
/// query string is the launch.
#[cfg(target_arch = "wasm32")]
#[wasm_bindgen::prelude::wasm_bindgen(start)]
pub fn run_web() {
    wasm_logger::init(wasm_logger::Config::new(log::Level::Info));
    console_error_panic_hook::set_once();
    let page = web_sys::window();
    let query = page
        .as_ref()
        .and_then(|window| window.location().search().ok())
        .unwrap_or_default();
    let side = |value: Result<wasm_bindgen::JsValue, _>| value.ok()?.as_f64();
    let size = page
        .as_ref()
        .and_then(|window| {
            Some(Size::new(
                side(window.inner_width())? as f32,
                side(window.inner_height())? as f32,
            ))
        })
        .unwrap_or(Size::new(1280.0, 820.0));
    if let Err(error) = run(Launch::from_query(&query), size) {
        log::error!("iced stopped: {error}");
    }
}
