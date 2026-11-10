//! The patchwork in iced (`../README.md`): nine patches of the still wall's
//! tiles beside a chat. Two patches never change; in the others a frame
//! changes a small part.
//!
//! In iced's idiom `view` builds the screen from the frame, as the Elm
//! architecture does, and `lazy` keeps what the frame did not change: a row
//! of tiles is lazy on how often it changed, a card on its number, a block
//! of the chat on its place. iced builds those widgets once and only places
//! them again. The charts are one canvas whose cache holds their geometry.
//! The pile has no part that stays, so `view` builds it every frame.

use std::{cell::RefCell, collections::HashMap};

use iced::{
    Color, Element, Length, Point, Radians, Rectangle, Renderer, Rotation, Size, Subscription,
    Task, Theme, alignment,
    mouse::Cursor,
    widget::{
        Column, Row, Stack, button, canvas, column, container, image, lazy, operation, pin,
        rich_text, row, scrollable, span, svg, text,
    },
};
use perf_data::*;

use super::{
    ROBOTO, ROBOTO_BOLD, TOP_BAR, fill, measure, rgb, rounded,
    wall::{Message, tile, tile_on, tiles},
};

const BACKGROUND: Color = rgb(WALL_BACKGROUND_RGB);
const INK: Color = rgb(WALL_INK_RGB);
const CHAT_BACKGROUND: Color = rgb(CHAT_BACKGROUND_RGB);
const PANEL: Color = rgb(CHAT_PANEL_RGB);
const BUBBLE: Color = rgb(CHAT_USER_RGB);
const MONO: iced::Font = iced::Font::with_name("DejaVu Sans Mono");
// The turns whose blocks the chat's list holds: more than it shows.
const TURNS_HELD: usize = 2;

type View = Element<'static, Message>;

pub struct Patchwork {
    tier: WallTier,
    window: Size,
    frame: FrameIndex,
    // The charts' geometry, drawn once for a grid.
    charts: canvas::Cache,
    feed: Option<ChatFeed>,
    // The chat's list, which each frame scrolls to its end.
    list: iced::widget::Id,
    token: Option<ChatToken>,
    // The animated picture's frames.
    gif: Vec<image::Handle>,
    spinner: svg::Handle,
    // The width of each character of the status line, measured once.
    advances: RefCell<HashMap<char, f32>>,
}

// One patch: its tiles and its size.
#[derive(Clone, Copy)]
struct Grid {
    tiles: PatchTiles,
    patch: Size,
}

impl Grid {
    // `content` in a patch's box, which cuts what lies past it.
    fn patch<'a>(self, content: impl Into<Element<'a, Message>>) -> Element<'a, Message> {
        container(content)
            .width(self.patch.width)
            .height(self.patch.height)
            .clip(true)
            .into()
    }
}

// Every frame of the GIF; none when it does not decode.
fn gif_frames() -> Vec<image::Handle> {
    let mut options = gif::DecodeOptions::new();
    options.set_color_output(gif::ColorOutput::RGBA);
    let mut frames = Vec::new();
    match options.read_info(CHAT_GIF) {
        Ok(mut decoder) => {
            let (width, height) = (u32::from(decoder.width()), u32::from(decoder.height()));
            while let Ok(Some(frame)) = decoder.read_next_frame() {
                frames.push(image::Handle::from_rgba(
                    width,
                    height,
                    frame.buffer.to_vec(),
                ));
            }
        }
        Err(error) => log::warn!("chat picture: {error}"),
    }
    frames
}

fn line(content: String, size: f32, font: iced::Font) -> text::Text<'static> {
    text(content)
        .size(size)
        .font(font)
        .color(INK)
        .line_height(text::LineHeight::Relative(1.4))
}

impl Patchwork {
    pub fn new(load: Launch, window: Size) -> Self {
        Self {
            tier: wall_tier(load.tier),
            window,
            frame: FrameIndex::new(load.freeze),
            charts: canvas::Cache::new(),
            feed: ChatFeed::start()
                .map_err(|error| log::warn!("chat data thread: {error}"))
                .ok(),
            token: None,
            list: iced::widget::Id::unique(),
            gif: gif_frames(),
            spinner: svg::Handle::from_memory(CHAT_SVG.as_bytes()),
            advances: RefCell::new(HashMap::new()),
        }
    }

    pub fn update(&mut self, message: Message) -> Task<Message> {
        match message {
            Message::Resized(size) => {
                self.window = size;
                self.charts.clear();
            }
            Message::Frame => {
                if self.frame.advance()
                    && let Some(feed) = &self.feed
                {
                    for _ in 0..CHAT_TOKENS_PER_FRAME {
                        match feed.take() {
                            Some(token) => self.token = Some(token),
                            None => break,
                        }
                    }
                    return operation::snap_to_end(self.list.clone());
                }
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

    fn grid(&self) -> Grid {
        let patch = Size::new(
            ((self.window.width - CHAT_WIDTH) / PATCH_COLUMNS as f32).floor(),
            ((self.window.height - 56.0 - WALL_CLOCK_HEIGHT) / PATCH_ROWS as f32).floor(),
        );
        Grid {
            tiles: PatchTiles::new(self.tier, patch.width, patch.height),
            patch,
        }
    }

    pub fn view(&self) -> Element<'_, Message> {
        let title = text("Patchwork")
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
        let grid = self.grid();
        let frame = self.frame.get();
        let patches = Column::with_children((0..PATCH_ROWS).map(|across| {
            Row::with_children(
                Patch::ALL[across * PATCH_COLUMNS..(across + 1) * PATCH_COLUMNS]
                    .iter()
                    .map(|patch| grid.patch(self.patch(*patch, grid, frame))),
            )
            .into()
        }));
        container(column![top_bar, clock, row![patches, self.chat(frame)]])
            .width(Length::Fill)
            .height(Length::Fill)
            .style(|_| fill(BACKGROUND))
            .into()
    }

    fn patch(&self, patch: Patch, grid: Grid, frame: u32) -> Element<'_, Message> {
        match patch {
            Patch::Still => still(grid),
            Patch::Charts => canvas(Charts {
                cache: &self.charts,
                grid,
            })
            .width(grid.patch.width)
            .height(grid.patch.height)
            .into(),
            Patch::Recolor => recolor(grid, frame),
            Patch::Cards => cards(grid, frame),
            Patch::Reorder => reorder(grid, frame),
            Patch::Deep => deep(grid, frame),
            Patch::Scroll => scroll(grid, frame),
            Patch::Paragraph => container(
                line(
                    patchwork_paragraph(frame, patchwork_paragraph_words(grid.tiles.count())),
                    wall_text_size(grid.tiles.height).0,
                    ROBOTO,
                )
                .width(Length::Fill),
            )
            .padding(WALL_GAP)
            .into(),
            Patch::Pile => pile(grid, frame),
        }
    }

    fn chat(&self, frame: u32) -> Element<'_, Message> {
        let mut blocks = Column::new().spacing(8).padding(8).width(Length::Fill);
        if let Some(token) = self.token {
            let first = (token.block + 1).saturating_sub(TURNS_HELD * CHAT_TURN.len());
            for block in first..=token.block {
                blocks = blocks.push(self.block(block, token, frame));
            }
        }
        let list = scrollable(blocks)
            .id(self.list.clone())
            .direction(scrollable::Direction::Vertical(
                scrollable::Scrollbar::hidden(),
            ))
            .width(Length::Fill)
            .height(Length::Fill);
        container(column![list, self.status(frame)])
            .width(CHAT_WIDTH)
            .height(Length::Fill)
            .style(|_| fill(CHAT_BACKGROUND))
            .into()
    }

    // Block `block`: the last one as far as `token` shows it, the animated
    // picture at the frame's place, and any other from the widgets `lazy`
    // built for it once.
    fn block(&self, block: usize, token: ChatToken, frame: u32) -> Element<'_, Message> {
        let kind = CHAT_TURN[block % CHAT_TURN.len()];
        if let ChatBlock::Gif = kind {
            return match self.gif.get(chat_gif_frame(frame, self.gif.len())) {
                Some(picture) => image(picture.clone())
                    .width(CHAT_GIF_SIZE)
                    .height(CHAT_GIF_SIZE)
                    .into(),
                None => column![].into(),
            };
        }
        if block == token.block {
            written(block, kind, token.shown)
        } else {
            lazy(block, move |&block| written(block, kind, usize::MAX)).into()
        }
    }

    // The status line: the spinner turned to the frame's angle, and what the
    // agent does, each character in the gradient's color at its place. iced
    // has no gradient for text.
    fn status(&self, frame: u32) -> Element<'_, Message> {
        let spinner = svg(self.spinner.clone())
            .width(CHAT_SPINNER_SIZE)
            .height(CHAT_SPINNER_SIZE)
            .rotation(Rotation::Floating(Radians(
                (frame as f32 * CHAT_SPIN_DEGREES).to_radians(),
            )));
        let [dim, bright] = CHAT_SHIMMER_RGB;
        let mut advances = self.advances.borrow_mut();
        let mut at = 0.0;
        let said = self.token.map(chat_status).unwrap_or_default();
        let spans: Vec<text::Span<'static, (), iced::Font>> = said
            .chars()
            .map(|character| {
                let weight = chat_shimmer(at, frame);
                at += *advances.entry(character).or_insert_with(|| {
                    measure(character.encode_utf8(&mut [0; 4]), CHAT_TEXT, ROBOTO).width
                });
                let channel = |index: usize| {
                    dim[index] + ((bright[index] - dim[index]) as f32 * weight) as u8
                };
                span(String::from(character)).color(Color::from_rgb8(
                    channel(0),
                    channel(1),
                    channel(2),
                ))
            })
            .collect();
        let said: text::Rich<'_, (), Message> = rich_text(spans).size(CHAT_TEXT).font(ROBOTO);
        container(
            row![spinner, said]
                .spacing(6)
                .align_y(alignment::Vertical::Center),
        )
        .height(CHAT_STATUS_HEIGHT)
        .width(Length::Fill)
        .padding([0, 8])
        .align_y(alignment::Vertical::Center)
        .style(|_| fill(PANEL))
        .into()
    }
}

// Block `block` of kind `kind` as far as `shown` tokens show it.
fn written(block: usize, kind: ChatBlock, shown: usize) -> View {
    let turn = ChatToken::turn(block);
    let typed = |text: &'static str| &text[..chat_shown_chars(text, shown.min(text.len()))];
    match kind {
        ChatBlock::User(said) => container(
            container(line(said.to_owned(), CHAT_TEXT, ROBOTO))
                .padding([6, 10])
                .style(|_| rounded(BUBBLE, 10.0)),
        )
        .width(Length::Fill)
        .align_x(alignment::Horizontal::Right)
        .into(),
        ChatBlock::Heading(said) => line(
            format!("{} {}", chat_turn_label(turn), typed(said)),
            CHAT_HEADING_TEXT,
            ROBOTO_BOLD,
        )
        .width(Length::Fill)
        .into(),
        ChatBlock::Paragraph(said) => line(
            format!("{} {}", chat_turn_label(turn), typed(said)),
            CHAT_TEXT,
            ROBOTO,
        )
        .width(Length::Fill)
        .into(),
        ChatBlock::Code(code) => container(highlighted(turn, typed(code)))
            .padding(8)
            .width(Length::Fill)
            .style(|_| rounded(PANEL, 6.0))
            .into(),
        ChatBlock::Table(rows) => {
            Column::with_children(rows.iter().take(shown).enumerate().map(|(index, cells)| {
                let font = if index == 0 { ROBOTO_BOLD } else { ROBOTO };
                let shaded = index == 0 || index % 2 == 1;
                container(Row::with_children(cells.iter().map(|cell| {
                    line((*cell).to_owned(), CHAT_CODE_TEXT, font)
                        .width(Length::FillPortion(1))
                        .into()
                })))
                .padding([3, 6])
                .width(Length::Fill)
                .style(move |_| {
                    if shaded {
                        fill(PANEL)
                    } else {
                        container::Style::default()
                    }
                })
                .into()
            }))
            .into()
        }
        ChatBlock::Buttons(labels) => Row::with_children(labels.iter().map(|label| {
            button(line((*label).to_owned(), CHAT_TEXT, ROBOTO_BOLD))
                .padding([6, 10])
                .style(|_, _| button::Style {
                    background: Some(BUBBLE.into()),
                    text_color: INK,
                    border: iced::Border::default().rounded(6.0),
                    ..button::Style::default()
                })
                .into()
        }))
        .spacing(8)
        .into(),
        // `Patchwork::block` shows the picture.
        ChatBlock::Gif => column![].into(),
    }
}

// `code` under a comment that names its turn, each stretch in its color.
fn highlighted(turn: usize, code: &str) -> View {
    let colored = |code: &str, color: Color| -> text::Span<'static, (), iced::Font> {
        span(code.to_owned()).color(color)
    };
    let mut spans = vec![colored(
        &format!("// {}\n", chat_turn_label(turn)),
        rgb(CodeKind::Comment.rgb()),
    )];
    let mut at = 0;
    for (stretch, kind) in chat_code_spans(code) {
        spans.push(colored(&code[at..stretch.start], INK));
        spans.push(colored(&code[stretch.clone()], rgb(kind.rgb())));
        at = stretch.end;
    }
    spans.push(colored(&code[at..], INK));
    let code: text::Rich<'static, (), Message> = rich_text(spans)
        .size(CHAT_CODE_TEXT)
        .font(MONO)
        .line_height(text::LineHeight::Relative(1.4));
    code.into()
}

fn still(grid: Grid) -> View {
    let (width, height) = (grid.tiles.width, grid.tiles.height);
    lazy(
        (grid.tiles.columns, grid.tiles.rows),
        move |&(columns, rows)| tiles(columns, rows, width, height),
    )
    .into()
}

// Rows of a patch `WALL_GAP` apart and from its edges.
fn rows_of(rows: impl Iterator<Item = View>) -> View {
    Column::with_children(rows)
        .spacing(WALL_GAP)
        .padding(WALL_GAP)
        .into()
}

// The line charts, all of them one geometry the cache holds.
struct Charts<'a> {
    cache: &'a canvas::Cache,
    grid: Grid,
}

impl canvas::Program<Message> for Charts<'_> {
    type State = ();

    fn draw(
        &self,
        _: &(),
        renderer: &Renderer,
        _: &Theme,
        bounds: Rectangle,
        _: Cursor,
    ) -> Vec<canvas::Geometry> {
        let (columns, rows) = patchwork_pairs(self.grid.tiles.columns, self.grid.tiles.rows);
        let (width, height) = (
            patchwork_double(self.grid.tiles.width),
            patchwork_double(self.grid.tiles.height),
        );
        let step = width / (PATCHWORK_CHART_POINTS - 1) as f32;
        vec![self.cache.draw(renderer, bounds.size(), |frame| {
            for index in 0..columns * rows {
                let left = WALL_GAP + (index % columns) as f32 * (width + WALL_GAP);
                let top = WALL_GAP + (index / columns) as f32 * (height + WALL_GAP);
                let chart = canvas::Path::new(|path| {
                    for point in 0..PATCHWORK_CHART_POINTS {
                        let at = Point::new(
                            left + point as f32 * step,
                            top + height * (1.0 - patchwork_chart_value(index, point)),
                        );
                        if point == 0 {
                            path.move_to(at);
                        } else {
                            path.line_to(at);
                        }
                    }
                });
                frame.stroke(
                    &chart,
                    canvas::Stroke::default()
                        .with_width(PATCHWORK_CHART_STROKE)
                        .with_color(rgb(patchwork_chart_rgb(index))),
                );
            }
        })]
    }
}

// Tiles whose fill changes in turn: a row is lazy on how often it changed.
fn recolor(grid: Grid, frame: u32) -> View {
    let turn = grid.tiles.in_turn();
    rows_of((0..grid.tiles.rows).map(|row| {
        let first = row * grid.tiles.columns;
        lazy((first, turn.step(first, frame)), move |_| -> View {
            Row::with_children((first..first + grid.tiles.columns).map(|index| {
                tile_on(
                    index,
                    Size::new(grid.tiles.width, grid.tiles.height),
                    grid.tiles.height,
                    rgb(patchwork_recolor_rgb(index, frame, turn)),
                )
            }))
            .spacing(WALL_GAP)
            .into()
        })
        .into()
    }))
}

// Cards pinned at the frame's places, each lazy on its number.
fn cards(grid: Grid, frame: u32) -> View {
    let cards = Cards::new(
        grid.tiles.columns,
        grid.tiles.rows,
        grid.tiles.height,
        grid.patch.height,
    );
    let header = patchwork_reorder_width(cards.columns, grid.tiles.width);
    Stack::with_children(
        (0..cards.places())
            .filter_map(|place| cards.place(place, frame))
            .map(|card| {
                let id = card.id;
                pin(lazy(id, move |_| -> View {
                    let title = container(
                        line(
                            patchwork_card_title(id),
                            wall_text_size(grid.tiles.height).0,
                            ROBOTO_BOLD,
                        )
                        .wrapping(text::Wrapping::None),
                    )
                    .width(header)
                    .height(grid.tiles.height)
                    .padding(wall_padding(grid.tiles.height))
                    .clip(true)
                    .style(move |_| rounded(rgb(patchwork_card_rgb(id)), WALL_CORNER));
                    let rows = (0..cards.rows).map(|row| {
                        Row::with_children((0..cards.columns).map(|column| {
                            let place = row * cards.columns + column;
                            tile(
                                patchwork_card_tile(id, place, cards.tiles()),
                                grid.tiles.width,
                                grid.tiles.height,
                            )
                        }))
                        .spacing(WALL_GAP)
                        .into()
                    });
                    Column::with_children(std::iter::once(title.into()).chain(rows))
                        .spacing(WALL_GAP)
                        .into()
                }))
                .x(WALL_GAP)
                .y(card.top)
                .into()
            }),
    )
    .width(grid.patch.width)
    .height(grid.patch.height)
    .into()
}

// A wrapping row of items in the frame's order, each lazy on its number.
fn reorder(grid: Grid, frame: u32) -> View {
    let count = patchwork_reorder_count(grid.tiles.count());
    container(
        Row::with_children((0..count).map(|position| {
            let item = patchwork_reorder_item(position, frame, count);
            lazy(item, move |&item| {
                let width =
                    patchwork_reorder_width(patchwork_reorder_units(item), grid.tiles.width);
                tile_on(
                    item,
                    Size::new(width, grid.tiles.height),
                    grid.tiles.height,
                    rgb(wall_fill_rgb(item)),
                )
            })
            .into()
        }))
        .spacing(WALL_GAP)
        .wrap()
        .vertical_spacing(WALL_GAP),
    )
    .padding(WALL_GAP)
    .width(Length::Fill)
    .into()
}

// Nests of boxes around digits that change in turn: a row of nests is lazy
// on how often it changed.
fn deep(grid: Grid, frame: u32) -> View {
    let (columns, rows) = patchwork_pairs(grid.tiles.columns, grid.tiles.rows);
    let turn = InTurn { columns, rows };
    let padding = wall_padding(grid.tiles.height);
    rows_of((0..rows).map(|row| {
        let first = row * columns;
        lazy((first, turn.step(first, frame)), move |_| -> View {
            Row::with_children((first..first + columns).map(|index| {
                let digits = line(
                    patchwork_deep_value(index, frame, turn),
                    wall_text_size(grid.tiles.height).0,
                    ROBOTO_BOLD,
                )
                .wrapping(text::Wrapping::None);
                let mut nest: View = digits.into();
                for level in (0..PATCHWORK_DEPTH).rev() {
                    nest = container(nest)
                        .width(Length::Fill)
                        .height(Length::Fill)
                        .padding(padding)
                        .clip(true)
                        .style(move |_| fill(rgb(patchwork_deep_rgb(index, level))))
                        .into();
                }
                container(nest)
                    .width(patchwork_double(grid.tiles.width))
                    .height(patchwork_double(grid.tiles.height))
                    .into()
            }))
            .spacing(WALL_GAP)
            .into()
        })
        .into()
    }))
}

// The rows the frame shows and the one the patch's edge cuts, each lazy on
// its place in the list.
fn scroll(grid: Grid, frame: u32) -> View {
    let first = patchwork_scroll_first(
        frame,
        patchwork_scroll_rows(grid.tiles.columns),
        grid.tiles.rows,
    );
    rows_of((first..=first + grid.tiles.rows).map(|row| {
        lazy(row, move |&row| -> View {
            Row::with_children((0..grid.tiles.columns).map(|column| {
                tile(
                    row * grid.tiles.columns + column,
                    grid.tiles.width,
                    grid.tiles.height,
                )
            }))
            .spacing(WALL_GAP)
            .into()
        })
        .into()
    }))
}

// Translucent tiles pinned over one another.
fn pile(grid: Grid, frame: u32) -> View {
    let (columns, rows) = patchwork_pile_grid(grid.tiles.columns, grid.tiles.rows);
    let opaque = patchwork_pile_opaque(frame, columns * rows);
    let size = Size::new(
        patchwork_double(grid.tiles.width),
        patchwork_double(grid.tiles.height),
    );
    Stack::with_children((0..columns * rows).map(|index| {
        let fill = if index == opaque {
            rgb(PATCHWORK_PILE_OPAQUE_RGB)
        } else {
            let [red, green, blue] = wall_fill_rgb(index);
            Color::from_rgba8(red, green, blue, f32::from(PATCHWORK_PILE_ALPHA) / 255.0)
        };
        pin(tile_on(index, size, grid.tiles.height, fill))
            .x(WALL_GAP + (index % columns) as f32 * (grid.tiles.width + WALL_GAP))
            .y(WALL_GAP + (index / columns) as f32 * (grid.tiles.height + WALL_GAP))
            .into()
    }))
    .width(grid.patch.width)
    .height(grid.patch.height)
    .into()
}

/// Runs the patchwork in a window of `size`.
pub fn run(load: Launch, size: Size) -> iced::Result {
    let mut application = iced::application(
        move || Patchwork::new(load, size),
        Patchwork::update,
        Patchwork::view,
    )
    .title("Patchwork")
    .subscription(Patchwork::subscription)
    .window_size(size)
    .resizable(false)
    .theme(|_: &Patchwork| Theme::Dark)
    .default_font(ROBOTO);
    for file in [
        "Roboto-Regular.ttf",
        "Roboto-Bold.ttf",
        "DejaVuSansMono.ttf",
    ] {
        match perf_data::font_bytes(file) {
            Ok(bytes) => application = application.font(bytes),
            Err(error) => log::warn!("no {file}: {error}"),
        }
    }
    application.run()
}
