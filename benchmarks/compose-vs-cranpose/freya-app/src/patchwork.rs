use freya::{
    engine::prelude::{Paint, PaintStyle, PathBuilder},
    prelude::*,
};
use perf_data::*;

use crate::{
    chat,
    gauntlet::{rgb, text},
    grid::HEADER,
};

pub(super) fn app() -> impl IntoElement {
    let mut token = use_state(|| None);
    let mut scroll = use_scroll_controller(ScrollConfig::default);
    let feed = use_hook(|| std::rc::Rc::new(ChatFeed::start().expect("chat producer")));
    let frame = crate::frames(move |_| {
        if let Some(next) = (0..CHAT_TOKENS_PER_FRAME).map_while(|_| feed.take()).last() {
            token.set(Some(next));
            scroll.scroll_to(ScrollPosition::End, Direction::Vertical);
        }
    });
    let frame = *frame.read();
    let (width, height) = DESKTOP_WINDOW;
    let w = ((width as f32 - CHAT_WIDTH) / PATCH_COLUMNS as f32).floor();
    let h = ((height as f32 - HEADER - WALL_CLOCK_HEIGHT) / PATCH_ROWS as f32).floor();
    let tiles = PatchTiles::new(wall_tier(perf_data::launch().tier), w, h);
    let grid = rect().children(Patch::ALL.chunks(PATCH_COLUMNS).map(|row| {
        rect().horizontal().children(row.iter().map(|&patch| {
            let frame = if matches!(patch, Patch::Still | Patch::Charts) {
                0
            } else {
                frame
            };
            rect()
                .width(Size::px(w))
                .height(Size::px(h))
                .overflow(Overflow::Clip)
                .child(Panel {
                    patch,
                    tiles,
                    height: h,
                    frame,
                })
        }))
    }));
    crate::screen(
        "Patchwork",
        frame,
        rect()
            .expanded()
            .horizontal()
            .child(grid)
            .child(chat::sidebar(*token.read(), frame, scroll)),
    )
}

fn cells(columns: usize, rows: usize, cell: impl Fn(usize) -> Element) -> Rect {
    rect()
        .padding(Gaps::new_all(WALL_GAP))
        .spacing(WALL_GAP)
        .children((0..rows).map(|row| {
            rect()
                .horizontal()
                .spacing(WALL_GAP)
                .children((row * columns..(row + 1) * columns).map(&cell))
        }))
}

#[derive(PartialEq)]
struct Tile {
    index: usize,
    width: f32,
    height: f32,
    unit: f32,
    fill: Color,
}

impl Tile {
    fn new(index: usize, tiles: PatchTiles) -> Self {
        Self {
            index,
            width: tiles.width,
            height: tiles.height,
            unit: tiles.height,
            fill: rgb(wall_fill_rgb(index)),
        }
    }
}

impl Component for Tile {
    fn render_key(&self) -> DiffKey {
        DiffKey::from(&self.index)
    }
    fn render(&self) -> impl IntoElement {
        let (ls, vs) = wall_text_size(self.unit);
        let dot = wall_dot_size(self.unit);
        rect()
            .width(Size::px(self.width))
            .height(Size::px(self.height))
            .overflow(Overflow::Clip)
            .corner_radius(WALL_CORNER)
            .background(self.fill)
            .padding(Gaps::new_all(wall_padding(self.unit)))
            .child(text(wall_label(self.index), ls, Color::WHITE, true).max_lines(1))
            .child(
                rect()
                    .horizontal()
                    .cross_align(Alignment::Center)
                    .spacing(dot * 0.5)
                    .child(
                        rect()
                            .width(Size::px(dot))
                            .height(Size::px(dot))
                            .corner_radius(dot / 2.)
                            .background(rgb(wall_dot_rgb(self.index))),
                    )
                    .child(text(wall_value(self.index), vs, Color::WHITE, false).max_lines(1)),
            )
    }
}

#[derive(PartialEq)]
struct Panel {
    patch: Patch,
    tiles: PatchTiles,
    height: f32,
    frame: u32,
}

impl Panel {
    fn cards(&self) -> Rect {
        let (t, k) = (self.tiles, self.frame);
        let cards = Cards::new(t.columns, t.rows, t.height, self.height);
        rect().expanded().children(
            (0..cards.places())
                .filter_map(|place| cards.place(place, k))
                .map(|card| {
                    rect()
                        .key(card.id)
                        .position(Position::new_absolute().left(WALL_GAP).top(card.top))
                        .child(Card {
                            id: card.id,
                            tiles: t,
                            cards,
                        })
                }),
        )
    }
}

impl Component for Panel {
    fn render(&self) -> impl IntoElement {
        let (t, k) = (self.tiles, self.frame);
        let (pw, ph) = (patchwork_double(t.width), patchwork_double(t.height));
        let (cols, rows) = patchwork_pairs(t.columns, t.rows);
        match self.patch {
            Patch::Still | Patch::Recolor => cells(t.columns, t.rows, |i| {
                let mut tile = Tile::new(i, t);
                if self.patch == Patch::Recolor {
                    tile.fill = rgb(patchwork_recolor_rgb(i, k, t.in_turn()));
                }
                tile.into_element()
            }),
            Patch::Charts => cells(cols, rows, |i| chart(i, pw, ph).into_element()),
            Patch::Cards => self.cards(),
            Patch::Reorder => {
                let count = patchwork_reorder_count(t.count());
                rect()
                    .width(Size::fill())
                    .horizontal()
                    .content(Content::Wrap {
                        wrap_spacing: Some(WALL_GAP),
                    })
                    .padding(Gaps::new_all(WALL_GAP))
                    .spacing(WALL_GAP)
                    .children((0..count).map(|pos| {
                        let i = patchwork_reorder_item(pos, k, count);
                        Tile {
                            width: patchwork_reorder_width(patchwork_reorder_units(i), t.width),
                            ..Tile::new(i, t)
                        }
                    }))
            }
            Patch::Deep => cells(cols, rows, |i| {
                let number = patchwork_deep_number(
                    i,
                    k,
                    InTurn {
                        columns: cols,
                        rows,
                    },
                );
                Nest {
                    index: i,
                    width: pw,
                    height: ph,
                    unit: t.height,
                    number,
                }
                .into_element()
            }),
            Patch::Scroll => {
                let total = patchwork_scroll_rows(t.columns);
                let first = patchwork_scroll_first(k, total, t.rows);
                let visible = ((self.height - WALL_GAP) / (t.height + WALL_GAP)).ceil() as usize;
                rect()
                    .padding(Gaps::new_all(WALL_GAP))
                    .spacing(WALL_GAP)
                    .children((first..(first + visible).min(total)).map(|row| {
                        rect().key(row).horizontal().spacing(WALL_GAP).children(
                            (row * t.columns..(row + 1) * t.columns).map(|i| Tile::new(i, t)),
                        )
                    }))
            }
            Patch::Paragraph => rect()
                .width(Size::fill())
                .padding(Gaps::new_all(WALL_GAP))
                .child(text(
                    patchwork_paragraph(k, patchwork_paragraph_words(t.count())),
                    wall_text_size(t.height).0,
                    Color::WHITE,
                    false,
                )),
            Patch::Pile => {
                let (columns, rows) = patchwork_pile_grid(t.columns, t.rows);
                let opaque = patchwork_pile_opaque(k, columns * rows);
                rect().expanded().children((0..columns * rows).map(|i| {
                    let x = WALL_GAP + (i % columns) as f32 * (t.width + WALL_GAP);
                    let y = WALL_GAP + (i / columns) as f32 * (t.height + WALL_GAP);
                    let [r, g, b] = wall_fill_rgb(i);
                    let fill = if i == opaque {
                        rgb(PATCHWORK_PILE_OPAQUE_RGB)
                    } else {
                        Color::from_argb(PATCHWORK_PILE_ALPHA, r, g, b)
                    };
                    rect()
                        .position(Position::new_absolute().left(x).top(y))
                        .child(Tile {
                            width: pw,
                            height: ph,
                            fill,
                            ..Tile::new(i, t)
                        })
                }))
            }
        }
    }
}

fn chart(i: usize, w: f32, h: f32) -> Rect {
    let mut path = PathBuilder::new();
    path.move_to((0., h * (1. - patchwork_chart_value(i, 0))));
    for p in 1..PATCHWORK_CHART_POINTS {
        path.line_to((
            p as f32 * w / (PATCHWORK_CHART_POINTS - 1) as f32,
            h * (1. - patchwork_chart_value(i, p)),
        ));
    }
    let path = path.detach();
    let mut stroke = Paint::default();
    stroke
        .set_anti_alias(true)
        .set_style(PaintStyle::Stroke)
        .set_stroke_width(PATCHWORK_CHART_STROKE)
        .set_color(rgb(patchwork_chart_rgb(i)));
    rect()
        .width(Size::px(w))
        .height(Size::px(h))
        .background(RenderCallback::new(move |ctx| {
            if let RenderTarget::Background { canvas, .. } = &ctx.target {
                canvas.draw_path(&path, &stroke);
            }
        }))
}

#[derive(PartialEq)]
struct Card {
    id: usize,
    tiles: PatchTiles,
    cards: Cards,
}

impl Component for Card {
    fn render(&self) -> impl IntoElement {
        let (t, c, id) = (self.tiles, self.cards, self.id);
        rect()
            .spacing(WALL_GAP)
            .child(
                rect()
                    .width(Size::px(patchwork_reorder_width(c.columns, t.width)))
                    .height(Size::px(t.height))
                    .corner_radius(WALL_CORNER)
                    .background(rgb(patchwork_card_rgb(id)))
                    .padding(Gaps::new_all(wall_padding(t.height)))
                    .child(
                        text(
                            patchwork_card_title(id),
                            wall_text_size(t.height).0,
                            Color::WHITE,
                            true,
                        )
                        .max_lines(1),
                    ),
            )
            .children((0..c.rows).map(|row| {
                rect().horizontal().spacing(WALL_GAP).children(
                    (row * c.columns..(row + 1) * c.columns)
                        .map(|i| Tile::new(patchwork_card_tile(id, i, c.tiles()), t)),
                )
            }))
    }
}

#[derive(PartialEq)]
struct Nest {
    index: usize,
    width: f32,
    height: f32,
    unit: f32,
    number: usize,
}

impl Component for Nest {
    fn render(&self) -> impl IntoElement {
        let mut inner = text(
            format!("{:03}", self.number),
            wall_text_size(self.unit).0,
            Color::WHITE,
            true,
        )
        .max_lines(1)
        .into_element();
        for level in (0..PATCHWORK_DEPTH).rev() {
            inner = rect()
                .expanded()
                .background(rgb(patchwork_deep_rgb(self.index, level)))
                .padding(Gaps::new_all(wall_padding(self.unit)))
                .child(inner)
                .into_element();
        }
        rect()
            .width(Size::px(self.width))
            .height(Size::px(self.height))
            .overflow(Overflow::Clip)
            .child(inner)
    }
}
