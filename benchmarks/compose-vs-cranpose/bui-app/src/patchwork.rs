//! The patchwork (`../../README.md`): nine patches of the still wall's tiles
//! beside a chat. Two never change; in the others a frame changes a small part.
//!
//! In BUI's idiom a frame builds the tree of elements again and the
//! reconciler matches it to the last one by key: a node whose style and
//! children are equal keeps its layout, so only what changed is laid out
//! again. Items that move carry a key of their own (the reorder patch's
//! tiles, the scroll patch's rows, the cards). The cards move by a
//! translation, which never lays out again. The scroll patch is BUI's
//! `uniform_list`, which builds the visible rows from a scroll offset that
//! the frame sets. The charts are canvases that draw a polyline whose points
//! are computed once per size. The chat is in `chat.rs`.

use std::{cell::RefCell, rc::Rc};

use blenderui::prelude::*;
use perf_data::*;

use crate::{
    BOLD,
    chat::Chat,
    color, line, top_bar,
    wall::{self, clock, tile},
};

/// The patchwork's geometry: the patch size in points, its tiles and cards.
#[derive(Clone, Copy)]
struct Grid {
    width: f32,
    height: f32,
    tiles: PatchTiles,
    cards: Cards,
}

impl Grid {
    fn new() -> Self {
        let tier = wall_tier(perf_data::launch().tier);
        let (window_width, window_height) = DESKTOP_WINDOW;
        let width = ((window_width as f32 - CHAT_WIDTH) / PATCH_COLUMNS as f32).floor();
        let height =
            ((window_height as f32 - 56.0 - WALL_CLOCK_HEIGHT) / PATCH_ROWS as f32).floor();
        let tiles = PatchTiles::new(tier, width, height);
        Self {
            width,
            height,
            tiles,
            cards: Cards::new(tiles.columns, tiles.rows, tiles.height, height),
        }
    }

    /// A patch's own box: the patch's size, which cuts what lies past it.
    fn patch(self) -> Div {
        div()
            .relative()
            .flex_none()
            .w(px(self.width))
            .h(px(self.height))
            .overflow_hidden()
    }

    /// A tile of the tier's size with tile number `index`.
    fn tile(self, index: usize, width: f32, fill: Rgba) -> Div {
        tile(index, width, self.tiles.height, self.tiles.height, fill)
    }

    /// The tile at the wall's fill, `units` of them wide.
    fn plain_tile(self, index: usize, units: usize) -> Div {
        self.tile(
            index,
            patchwork_reorder_width(units, self.tiles.width),
            color(wall_fill_rgb(index)),
        )
    }
}

/// One chart's points, in fractions of its box.
struct ChartLine {
    values: Vec<f32>,
    stroke: Rgba,
    /// The points in the window's coordinates, computed for one box.
    placed: RefCell<(Bounds<f32>, Vec<(f32, f32)>)>,
}

impl ChartLine {
    fn new(index: usize) -> Self {
        Self {
            values: (0..PATCHWORK_CHART_POINTS)
                .map(|point| patchwork_chart_value(index, point))
                .collect(),
            stroke: color(patchwork_chart_rgb(index)),
            placed: RefCell::new((
                Bounds {
                    origin: point(0.0, 0.0),
                    size: size(0.0, 0.0),
                },
                Vec::new(),
            )),
        }
    }

    fn paint(&self, bounds: Bounds<f32>, painter: &mut Painter) {
        let mut placed = self.placed.borrow_mut();
        let (at, points) = &mut *placed;
        if *at != bounds {
            *at = bounds;
            let step = bounds.size.width / (PATCHWORK_CHART_POINTS - 1) as f32;
            points.clear();
            points.extend(self.values.iter().enumerate().map(|(index, value)| {
                (
                    bounds.origin.x + index as f32 * step,
                    bounds.origin.y + bounds.size.height * (1.0 - value),
                )
            }));
        }
        painter.line(points, self.stroke, PATCHWORK_CHART_STROKE);
    }
}

pub(crate) struct Patchwork {
    frame: FrameIndex,
    grid: Grid,
    charts: Vec<Rc<ChartLine>>,
    chat: Chat,
}

impl Patchwork {
    pub(crate) fn new() -> Self {
        let grid = Grid::new();
        let (columns, rows) = patchwork_pairs(grid.tiles.columns, grid.tiles.rows);
        Self {
            frame: FrameIndex::new(perf_data::launch().freeze),
            grid,
            charts: (0..columns * rows)
                .map(|index| Rc::new(ChartLine::new(index)))
                .collect(),
            chat: Chat::new(),
        }
    }

    fn patch(&self, patch: Patch, window: &mut Window) -> Div {
        let grid = self.grid;
        let frame = self.frame.get();
        match patch {
            Patch::Still => {
                grid.patch()
                    .child(wall::grid(grid.tiles.columns, grid.tiles.rows, |index| {
                        grid.plain_tile(index, 1).into_any_element()
                    }))
            }
            Patch::Charts => grid.patch().child(self.charts(grid)),
            Patch::Recolor => grid.patch().child(recolor(grid, frame)),
            Patch::Cards => grid.patch().children(cards(grid, frame)),
            Patch::Reorder => grid.patch().child(reorder(grid, frame)),
            Patch::Deep => grid.patch().child(deep(grid, frame)),
            Patch::Scroll => grid.patch().child(scroll(grid, frame, window)),
            Patch::Paragraph => grid.patch().child(paragraph(grid, frame)),
            Patch::Pile => grid.patch().children(pile(grid, frame)),
        }
    }

    /// Line charts that never change: each draws the same polyline.
    fn charts(&self, grid: Grid) -> Div {
        let (columns, rows) = patchwork_pairs(grid.tiles.columns, grid.tiles.rows);
        wall::grid(columns, rows, |index| {
            let chart = self.charts[index].clone();
            canvas(move |bounds, painter| chart.paint(bounds, painter))
                .flex_none()
                .w(px(patchwork_double(grid.tiles.width)))
                .h(px(patchwork_double(grid.tiles.height)))
                .into_any_element()
        })
    }
}

/// Tiles whose fill changes in turn.
fn recolor(grid: Grid, frame: u32) -> Div {
    let turn = grid.tiles.in_turn();
    wall::grid(grid.tiles.columns, grid.tiles.rows, |index| {
        grid.tile(
            index,
            grid.tiles.width,
            color(patchwork_recolor_rgb(index, frame, turn)),
        )
        .into_any_element()
    })
}

/// The cards the frame shows, each moved to its place by a translation. The
/// set of cards is the one the current move began with.
fn cards(grid: Grid, frame: u32) -> impl Iterator<Item = Div> {
    let cards = grid.cards;
    let begun = frame / PATCHWORK_CARD_FRAMES * PATCHWORK_CARD_FRAMES;
    (0..cards.places())
        .filter_map(move |place| cards.place(place, begun))
        .map(move |card| {
            let top = cards.top(card.id, frame).unwrap_or(-cards.height);
            let width = patchwork_reorder_width(cards.columns, grid.tiles.width);
            let padding = wall_padding(grid.tiles.height);
            let ink = color(WALL_INK_RGB);
            div()
                .key(format!("card-{}", card.id))
                .absolute()
                .left(px(0.0))
                .top(px(0.0))
                .css("transform", format!("translateY({top}px)"))
                .px(px(WALL_GAP))
                .flex()
                .flex_col()
                .gap(px(WALL_GAP))
                .child(
                    div()
                        .relative()
                        .flex_none()
                        .w(px(width))
                        .h(px(grid.tiles.height))
                        .rounded(px(WALL_CORNER))
                        .bg(color(patchwork_card_rgb(card.id)))
                        .child(line(
                            patchwork_card_title(card.id),
                            wall_text_size(grid.tiles.height).0,
                            BOLD,
                            ink,
                            padding,
                            padding,
                        )),
                )
                .children((0..cards.rows).map(move |row| {
                    div()
                        .flex()
                        .gap(px(WALL_GAP))
                        .children((0..cards.columns).map(move |column| {
                            let index = patchwork_card_tile(
                                card.id,
                                row * cards.columns + column,
                                cards.tiles(),
                            );
                            grid.plain_tile(index, 1)
                        }))
                }))
        })
}

/// A wrapping row of items, two of which change places every frame: each
/// item keeps its key.
fn reorder(grid: Grid, frame: u32) -> Div {
    let count = patchwork_reorder_count(grid.tiles.count());
    div()
        .p(px(WALL_GAP))
        .flex()
        .flex_wrap()
        .gap(px(WALL_GAP))
        .children((0..count).map(move |position| {
            let item = patchwork_reorder_item(position, frame, count);
            grid.plain_tile(item, patchwork_reorder_units(item))
                .key(format!("item-{item}"))
        }))
}

/// Nests of boxes around digits that change in turn.
fn deep(grid: Grid, frame: u32) -> Div {
    let (columns, rows) = patchwork_pairs(grid.tiles.columns, grid.tiles.rows);
    let turn = InTurn { columns, rows };
    wall::grid(columns, rows, |index| {
        div()
            .flex_none()
            .w(px(patchwork_double(grid.tiles.width)))
            .h(px(patchwork_double(grid.tiles.height)))
            .child(nest(
                grid,
                index,
                0,
                patchwork_deep_value(index, frame, turn),
            ))
            .into_any_element()
    })
}

/// Box `level` of nest `index` and everything inside it.
fn nest(grid: Grid, index: usize, level: usize, digits: String) -> Div {
    let inner = if level + 1 < PATCHWORK_DEPTH {
        nest(grid, index, level + 1, digits).into_any_element()
    } else {
        crate::text(
            digits,
            wall_text_size(grid.tiles.height).0,
            BOLD,
            color(WALL_INK_RGB),
        )
        .into_any_element()
    };
    div()
        .w_full()
        .h_full()
        .box_border()
        .p(px(wall_padding(grid.tiles.height)))
        .bg(color(patchwork_deep_rgb(index, level)))
        .child(inner)
}

/// A list of rows of tiles, scrolled by one row every frame. `uniform_list`
/// builds the rows in view from the offset.
fn scroll(grid: Grid, frame: u32, window: &mut Window) -> Div {
    const LIST: &str = "patchwork-scroll";
    let pitch = grid.tiles.height + WALL_GAP;
    let rows = patchwork_scroll_rows(grid.tiles.columns);
    let first = patchwork_scroll_first(frame, rows, grid.tiles.rows);
    let offset = first as f32 * pitch;
    if window.scroll_offset(LIST).1 != offset {
        window.set_scroll_offset(LIST, 0.0, offset);
    }
    div().pl(px(WALL_GAP)).pt(px(WALL_GAP)).child(
        uniform_list(LIST, rows, move |range, _, _| {
            range
                .map(|row| {
                    div()
                        .key(format!("row-{row}"))
                        .flex()
                        .gap(px(WALL_GAP))
                        .children((0..grid.tiles.columns).map(move |column| {
                            grid.plain_tile(row * grid.tiles.columns + column, 1)
                        }))
                })
                .collect::<Vec<_>>()
        })
        .item_height(px(pitch))
        .w(px(grid.width - WALL_GAP))
        .h(px(grid.height - WALL_GAP)),
    )
}

/// One paragraph, one word of which changes every frame.
fn paragraph(grid: Grid, frame: u32) -> Div {
    let size = wall_text_size(grid.tiles.height).0;
    div().p(px(WALL_GAP)).child(
        div()
            .text_size(px(size))
            .line_height(px(size * 1.4))
            .text_color(color(WALL_INK_RGB))
            .child(patchwork_paragraph(
                frame,
                patchwork_paragraph_words(grid.tiles.count()),
            )),
    )
}

/// Translucent tiles lying over one another, one of which is opaque.
fn pile(grid: Grid, frame: u32) -> impl Iterator<Item = Div> {
    let (columns, rows) = patchwork_pile_grid(grid.tiles.columns, grid.tiles.rows);
    let opaque = patchwork_pile_opaque(frame, columns * rows);
    (0..columns * rows).map(move |index| {
        let fill = if index == opaque {
            color(PATCHWORK_PILE_OPAQUE_RGB)
        } else {
            let [red, green, blue] = wall_fill_rgb(index);
            Rgba {
                a: f32::from(PATCHWORK_PILE_ALPHA) / 255.0,
                ..color([red, green, blue])
            }
        };
        tile(
            index,
            patchwork_double(grid.tiles.width),
            patchwork_double(grid.tiles.height),
            grid.tiles.height,
            fill,
        )
        .absolute()
        .left(px(
            WALL_GAP + (index % columns) as f32 * (grid.tiles.width + WALL_GAP)
        ))
        .top(px(
            WALL_GAP + (index / columns) as f32 * (grid.tiles.height + WALL_GAP)
        ))
    })
}

impl Render for Patchwork {
    fn render(&mut self, window: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        let before = self.frame.get();
        self.frame.advance();
        if !self.frame.frozen() {
            window.request_animation_frame();
        }
        if self.frame.get() != before {
            self.chat.take_tokens();
        }
        self.chat.read_status(window);
        let frame = self.frame.get();
        let patches = (0..PATCH_ROWS).map(|row| {
            div().flex().children(
                Patch::ALL[row * PATCH_COLUMNS..(row + 1) * PATCH_COLUMNS]
                    .iter()
                    .map(|patch| self.patch(*patch, window))
                    .collect::<Vec<_>>(),
            )
        });
        html().size_full().child(
            body()
                .m_0()
                .size_full()
                .flex()
                .flex_col()
                .bg(color(WALL_BACKGROUND_RGB))
                .child(top_bar("Patchwork"))
                .child(clock(frame))
                .child(
                    div()
                        .flex()
                        .flex_1()
                        .min_h(px(0.0))
                        .child(div().flex_none().children(patches))
                        .child(self.chat.view(frame)),
                ),
        )
    }
}
