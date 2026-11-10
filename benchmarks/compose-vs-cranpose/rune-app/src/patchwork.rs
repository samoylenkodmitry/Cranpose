//! The patchwork in Rune (`../README.md`): the view runs again each frame, so
//! what a frame did not change is a `memo` that returns the element it built
//! before, with its layout.
//!
//! - Still and charts: one memo each, drawn as one cached layer.
//! - Recolor: the fills are a row memo versioned by its step, under the labels,
//!   which are one memo drawn as one cached layer.
//! - Deep: a row memo versioned by its step holds the digits only. The boxes of
//!   each nest are a memo of their own.
//! - Cards: one memo and one cached layer per card, under a wrapper that a
//!   `Live` offset moves, so a frame lays out and draws no card again.
//! - Reorder: one memo per item, which a frame puts in a new order.
//! - Scroll: `virtual_rows` builds only the rows that enter the window, and
//!   the frame sets the scroll offset of the viewport without a view.
//! - Paragraph: one text that the view builds again, as `live_text` takes one
//!   line only.
//! - Pile: one memo per row of tiles, versioned by the opaque tile it holds.

use std::{collections::HashMap, time::Duration};

use perf_data::*;
use rune::{
    Color, Cx, El, GrowthSpec, Live, LiveText, ScrollSpec, Shape, Tx, Waker, canvas, col, el, memo,
    row, text,
};

use super::{
    chat::Chat,
    rgb, roboto,
    wall::{tile, tile_parts},
};

/// The key of the scroll patch's viewport.
const SCROLL: &str = "patch-scroll";

/// One patch's size in points and its tiles.
#[derive(Clone, Copy, PartialEq, Debug)]
struct Grid {
    tiles: PatchTiles,
    width: f32,
    height: f32,
}

impl Grid {
    fn tile_size(self) -> [f32; 2] {
        [self.tiles.width, self.tiles.height]
    }

    /// A tile of this grid at wall index `index` on the fill of wall index `fill`.
    fn tile(self, index: usize, fill: [u8; 3]) -> El {
        tile(index, self.tile_size(), self.tiles.height, rgb(fill))
    }

    /// The tile at wall index `index` on its own fill.
    fn wall_tile(self, index: usize) -> El {
        self.tile(index, wall_fill_rgb(index))
    }
}

pub(crate) struct Patchwork {
    frame: FrameIndex,
    clock: LiveText,
    grid: Grid,
    cards: Cards,
    /// The cards in the order of their places, and the offset of each.
    card_ids: Vec<usize>,
    card_offsets: HashMap<usize, Live<[f32; 2]>>,
    pub(crate) chat: Chat,
}

impl Patchwork {
    pub(crate) fn new() -> Self {
        let tier = wall_tier(perf_data::launch().tier);
        let (window_width, window_height) = DESKTOP_WINDOW;
        let width = ((window_width as f32 - CHAT_WIDTH) / PATCH_COLUMNS as f32).floor();
        let height =
            ((window_height as f32 - 56.0 - WALL_CLOCK_HEIGHT) / PATCH_ROWS as f32).floor();
        let tiles = PatchTiles::new(tier, width, height);
        let grid = Grid {
            tiles,
            width,
            height,
        };
        let mut patchwork = Self {
            frame: FrameIndex::new(perf_data::launch().freeze),
            clock: LiveText::new(wall_clock(0)),
            grid,
            cards: Cards::new(tiles.columns, tiles.rows, tiles.height, height),
            card_ids: Vec::new(),
            card_offsets: HashMap::new(),
            chat: Chat::new(),
        };
        patchwork.place_cards(0);
        patchwork
    }

    /// Starts what needs a waker: the chat's pictures.
    pub(crate) fn setup(&mut self, wake: Waker) {
        self.chat.setup(wake);
    }

    /// Moves every card to its place in frame `frame`.
    fn place_cards(&mut self, frame: u32) {
        self.card_ids.clear();
        for place in 0..self.cards.places() {
            if let Some(card) = self.cards.place(place, frame) {
                self.card_ids.push(card.id);
                self.card_offsets
                    .entry(card.id)
                    .or_insert_with(|| Live::new([0.0; 2]))
                    .set([0.0, card.top]);
            }
        }
        let ids = &self.card_ids;
        self.card_offsets.retain(|id, _| ids.contains(id));
    }
}

/// Moves every patch and the chat to the next frame.
pub(crate) fn poll(patchwork: &mut Patchwork, _seconds: f64, tx: &mut Tx) -> bool {
    let before = patchwork.frame.get();
    patchwork.frame.advance();
    let frame = patchwork.frame.get();
    let changed = frame != before;
    if changed {
        let tiles = patchwork.grid.tiles;
        patchwork.clock.set(wall_clock(frame));
        patchwork.place_cards(frame);
        let first = patchwork_scroll_first(frame, patchwork_scroll_rows(tiles.columns), tiles.rows);
        tx.scroll_to_y(SCROLL, first as f32 * (tiles.height + WALL_GAP));
        patchwork.chat.advance(frame);
        tx.instant();
    }
    if !patchwork.frame.frozen() {
        tx.wake_after(Duration::from_millis(1));
    }
    changed
}

pub(crate) fn view(patchwork: &Patchwork, _: &mut Cx<Patchwork>) -> El {
    let frame = patchwork.frame.get();
    let top_bar = memo("top-bar", (), || {
        el().h(56.0)
            .w_full()
            .px(16.0)
            .fill(rgb([0x1E, 0x2A, 0x4A]))
            .center_y()
            .child(
                text("Patchwork")
                    .font(roboto(20.0, true))
                    .color(Color::WHITE),
            )
    });
    let clock = el().h(WALL_CLOCK_HEIGHT).w_full().px(8.0).center_y().child(
        text(wall_clock(0))
            .live_text(&patchwork.clock)
            .font(roboto(WALL_CLOCK_TEXT, false))
            .color(rgb(WALL_INK_RGB)),
    );
    let patches = col().children((0..PATCH_ROWS).map(|line| {
        row().children(
            Patch::ALL[line * PATCH_COLUMNS..(line + 1) * PATCH_COLUMNS]
                .iter()
                .map(|patch| {
                    el().size(patchwork.grid.width, patchwork.grid.height)
                        .clip_rounded(0.0)
                        .child(show(*patch, patchwork, frame))
                }),
        )
    }));
    col()
        .w_full()
        .h_full()
        .fill(rgb(WALL_BACKGROUND_RGB))
        .child(top_bar)
        .child(clock)
        .child(
            row()
                .w_full()
                .grow(1.0)
                .min_h(0.0)
                .child(patches)
                .child(Chat::view(&patchwork.chat)),
        )
}

fn show(patch: Patch, patchwork: &Patchwork, frame: u32) -> El {
    let grid = patchwork.grid;
    match patch {
        Patch::Still => still(grid),
        Patch::Charts => charts(grid),
        Patch::Recolor => recolor(grid, frame),
        Patch::Cards => cards(patchwork),
        Patch::Reorder => reorder(grid, frame),
        Patch::Deep => deep(grid, frame),
        Patch::Scroll => scroll(grid),
        Patch::Paragraph => paragraph(grid, frame),
        Patch::Pile => pile(grid, frame),
    }
}

/// A row of `columns` cells, `WALL_GAP` apart.
fn tile_row(columns: usize, cell: impl FnMut(usize) -> El) -> El {
    row().gap(WALL_GAP).children((0..columns).map(cell))
}

/// Rows of cells, `WALL_GAP` apart and from the patch's edges.
fn tile_rows(rows: impl Iterator<Item = El>) -> El {
    col().gap(WALL_GAP).padding(WALL_GAP).children(rows)
}

/// Tiles that never change: one memo, drawn as one cached layer.
fn still(grid: Grid) -> El {
    memo("still", (), move || {
        let columns = grid.tiles.columns;
        tile_rows(
            (0..grid.tiles.rows)
                .map(|line| tile_row(columns, |column| grid.wall_tile(line * columns + column))),
        )
        .cache()
    })
}

/// Charts that never change: one memo, drawn as one cached layer.
fn charts(grid: Grid) -> El {
    memo("charts", (), move || {
        let (columns, rows) = patchwork_pairs(grid.tiles.columns, grid.tiles.rows);
        let size = [
            patchwork_double(grid.tiles.width),
            patchwork_double(grid.tiles.height),
        ];
        tile_rows(
            (0..rows).map(|line| tile_row(columns, |column| chart(line * columns + column, size))),
        )
        .cache()
    })
}

fn chart(index: usize, [width, height]: [f32; 2]) -> El {
    let step = width / (PATCHWORK_CHART_POINTS - 1) as f32;
    let line = (0..PATCHWORK_CHART_POINTS)
        .map(|point| {
            [
                point as f32 * step,
                height * (1.0 - patchwork_chart_value(index, point)),
            ]
        })
        .collect::<Vec<_>>();
    let shape = Shape::new()
        .stroke(
            line,
            PATCHWORK_CHART_STROKE,
            rgb(patchwork_chart_rgb(index)),
        )
        .view_box(width, height);
    canvas(shape).w(width).h(height)
}

/// Tiles whose fill changes in turn. The fills are one layer under the tiles'
/// labels, dots and values, which are one cached layer that no frame draws
/// again. A fill row is a memo versioned by its step, so a frame builds and
/// draws the one row whose step changed.
fn recolor(grid: Grid, frame: u32) -> El {
    let turn = grid.tiles.in_turn();
    let columns = grid.tiles.columns;
    let size = grid.tile_size();
    let fills = tile_rows((0..grid.tiles.rows).map(|line| {
        memo(
            format!("recolor{line}"),
            turn.step(line * columns, frame),
            move || {
                tile_row(columns, |column| {
                    let index = line * columns + column;
                    el().size(size[0], size[1])
                        .radius(WALL_CORNER)
                        .fill(rgb(patchwork_recolor_rgb(index, frame, turn)))
                })
            },
        )
    }));
    let contents = memo("recolor-contents", (), move || {
        tile_rows((0..grid.tiles.rows).map(|line| {
            tile_row(columns, |column| {
                el().size(size[0], size[1])
                    .children(tile_parts(line * columns + column, grid.tiles.height))
            })
        }))
        .cache()
        .absolute(0.0, 0.0)
    });
    el().w_full().h_full().child(fills).child(contents)
}

/// Cards that move every frame. A card is a memo drawn as one cached layer,
/// which a wrapper moves by a `Live` offset the frame sets: the view only
/// names the cards in their places.
fn cards(patchwork: &Patchwork) -> El {
    let grid = patchwork.grid;
    let cards = patchwork.cards;
    el().w_full()
        .h_full()
        .children(patchwork.card_ids.iter().map(|id| {
            let id = *id;
            el().key(format!("card{id}"))
                .pin(Some(0.0), Some(0.0), None, None)
                .live_offset(&patchwork.card_offsets[&id])
                .child(memo(format!("card{id}"), (), move || card(id, cards, grid)))
        }))
}

/// Card `id`: its header over its rows of tiles.
fn card(id: usize, cards: Cards, grid: Grid) -> El {
    let padding = wall_padding(grid.tiles.height);
    let header = el()
        .size(
            patchwork_reorder_width(cards.columns, grid.tiles.width),
            grid.tiles.height,
        )
        .radius(WALL_CORNER)
        .fill(rgb(patchwork_card_rgb(id)))
        .padding(padding)
        .child(
            text(patchwork_card_title(id))
                .font(roboto(wall_text_size(grid.tiles.height).0, true))
                .color(rgb(WALL_INK_RGB)),
        );
    col()
        .px(WALL_GAP)
        .gap(WALL_GAP)
        .cache()
        .child(header)
        .children((0..cards.rows).map(|line| {
            tile_row(cards.columns, |column| {
                grid.wall_tile(patchwork_card_tile(
                    id,
                    line * cards.columns + column,
                    cards.tiles(),
                ))
            })
        }))
}

/// A wrapping row of items, two of which change places every frame: each item
/// is a memo, so a frame only puts them in their order.
fn reorder(grid: Grid, frame: u32) -> El {
    let count = patchwork_reorder_count(grid.tiles.count());
    row()
        .w_full()
        .padding(WALL_GAP)
        .gap(WALL_GAP)
        .flex_wrap()
        .children((0..count).map(|position| {
            let item = patchwork_reorder_item(position, frame, count);
            memo(format!("item{item}"), (), move || {
                let width =
                    patchwork_reorder_width(patchwork_reorder_units(item), grid.tiles.width);
                grid.tile(item, wall_fill_rgb(item))
                    .w(width)
                    .key(format!("item{item}"))
            })
        }))
}

/// Nests of boxes around digits that change in turn. The boxes of a nest are a
/// memo that never changes, the digits over them are not: a row of nests is a
/// memo versioned by its step, which builds the digits of the row alone.
fn deep(grid: Grid, frame: u32) -> El {
    let (columns, rows) = patchwork_pairs(grid.tiles.columns, grid.tiles.rows);
    let turn = InTurn { columns, rows };
    tile_rows((0..rows).map(|line| {
        memo(
            format!("deep{line}"),
            turn.step(line * columns, frame),
            move || {
                tile_row(columns, |column| {
                    let index = line * columns + column;
                    nest(index, patchwork_deep_number(index, frame, turn), grid)
                })
            },
        )
    }))
}

/// Nest `index`: `PATCHWORK_DEPTH` boxes, each inside the one before, with
/// `number` inside the innermost.
fn nest(index: usize, number: usize, grid: Grid) -> El {
    let padding = wall_padding(grid.tiles.height);
    let boxes = memo(format!("nest{index}"), (), move || {
        let innermost = el()
            .w_full()
            .h_full()
            .fill(rgb(patchwork_deep_rgb(index, PATCHWORK_DEPTH - 1)));
        (0..PATCHWORK_DEPTH - 1)
            .rev()
            .fold(innermost, |inner, level| {
                el().w_full()
                    .h_full()
                    .padding(padding)
                    .fill(rgb(patchwork_deep_rgb(index, level)))
                    .child(inner)
            })
    });
    let inset = PATCHWORK_DEPTH as f32 * padding;
    el().w(patchwork_double(grid.tiles.width))
        .h(patchwork_double(grid.tiles.height))
        .child(boxes)
        .child(
            text(format!("{number:03}"))
                .font(roboto(wall_text_size(grid.tiles.height).0, true))
                .color(rgb(WALL_INK_RGB))
                .absolute(inset, inset),
        )
}

/// A list of rows of tiles that the frame scrolls by one row: `virtual_rows`
/// builds only the rows that enter its window.
fn scroll(grid: Grid) -> El {
    let columns = grid.tiles.columns;
    col()
        .key(SCROLL)
        .size(grid.width, grid.height)
        .scroll(ScrollSpec {
            stick_bottom: false,
        })
        .child(
            col()
                .key("scroll-rows")
                .w_full()
                .padding(WALL_GAP)
                .gap(WALL_GAP)
                .growth(GrowthSpec::default())
                .virtual_rows(
                    patchwork_scroll_rows(columns),
                    grid.tiles.height,
                    |_: &Patchwork, line| format!("row{line}").into(),
                    move |_: &Patchwork, line| {
                        tile_row(columns, |column| grid.wall_tile(line * columns + column))
                    },
                )
                .virtual_rows_version(()),
        )
}

/// One paragraph. Rune's `live_text` takes one line only, so the view builds
/// the whole text again for the one word that changed.
fn paragraph(grid: Grid, frame: u32) -> El {
    let size = wall_text_size(grid.tiles.height).0;
    el().w_full().padding(WALL_GAP).child(
        text(patchwork_paragraph(
            frame,
            patchwork_paragraph_words(grid.tiles.count()),
        ))
        .w_full()
        .font(roboto(size, false))
        .color(rgb(WALL_INK_RGB))
        .wrap(),
    )
}

/// Translucent tiles lying over one another: a row of them is a memo versioned
/// by the opaque tile it holds, if any.
fn pile(grid: Grid, frame: u32) -> El {
    let (columns, rows) = patchwork_pile_grid(grid.tiles.columns, grid.tiles.rows);
    let opaque = patchwork_pile_opaque(frame, columns * rows);
    let size = [
        patchwork_double(grid.tiles.width),
        patchwork_double(grid.tiles.height),
    ];
    let alpha = f32::from(PATCHWORK_PILE_ALPHA) / 255.0;
    el().w_full().h_full().children((0..rows).map(|line| {
        let held = (opaque / columns == line).then_some(opaque % columns);
        memo(format!("pile{line}"), held, move || {
            el().w_full()
                .h(size[1])
                .absolute(
                    WALL_GAP,
                    WALL_GAP + line as f32 * (grid.tiles.height + WALL_GAP),
                )
                .children((0..columns).map(|column| {
                    let index = line * columns + column;
                    let fill = if held == Some(column) {
                        rgb(PATCHWORK_PILE_OPAQUE_RGB)
                    } else {
                        rgb(wall_fill_rgb(index)).alpha(alpha)
                    };
                    tile(index, size, grid.tiles.height, fill)
                        .absolute(column as f32 * (grid.tiles.width + WALL_GAP), 0.0)
                }))
        })
    }))
}
