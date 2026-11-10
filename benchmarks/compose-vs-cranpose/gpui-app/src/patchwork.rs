//! The patchwork in GPUI (`../../README.md`): nine patches of the still wall's
//! tiles. Two never change; in the others a frame changes a small part.
//!
//! In GPUI's idiom every patch is a view that the window embeds cached, and
//! GPUI reuses the layout and painting of a cached view until something
//! notifies it. The frame notifies only the views whose content changed: the
//! recolor patch's row of fills, the deep patch's row of digits, the pile
//! patch's row with the two tiles that turn opaque or translucent, the
//! paragraph. The tiles' text never changes, so the recolor and deep patches
//! keep it in a cached layer of its own over the fills or boxes. The reorder
//! patch lays out its items again, each a cached view that GPUI reuses where
//! its place did not change. A cached view that moves is drawn again, since
//! its bounds are part of its cache key. The cards of the cards patch move
//! every frame, so each card is a canvas that paints text shaped once and
//! costs no layout. The scroll patch draws the rows GPUI's `uniform_list`
//! shows.

use std::sync::Arc;

use gpui::{
    AnyView, App, AppContext, Bounds, ContentMask, Context, Div, Entity, Font, FontWeight,
    IntoElement, ParentElement, PathBuilder, Render, ScrollStrategy, ShapedLine, SharedString,
    StyleRefinement, Styled, TextAlign, TextRun, TitlebarOptions, UniformListScrollHandle, Window,
    WindowBounds, WindowOptions, canvas, div, fill, point, prelude::FluentBuilder, px, relative,
    size, uniform_list,
};
use perf_data::*;

use super::{
    chat::{Chat, ChatList, ChatStatus, gif_frames},
    rgb,
    wall::{LINE_HEIGHT, TileGeometry, Tiles, clock, tile, tile_box, top_bar},
};

/// The tiles of one patch and its size in points.
#[derive(Clone, Copy)]
struct Grid {
    tier: WallTier,
    tiles: PatchTiles,
    height: f32,
}

impl Grid {
    /// The tile at `column` and `row`, offset from a patch's corner.
    fn at(self, column: usize, row: usize) -> (f32, f32) {
        (
            WALL_GAP + column as f32 * (self.tiles.width + WALL_GAP),
            WALL_GAP + row as f32 * (self.tiles.height + WALL_GAP),
        )
    }
}

/// A cached view that fills the box it sits in.
fn whole() -> StyleRefinement {
    StyleRefinement::default().size_full()
}

/// A cached view of a size of its own, among its siblings.
fn sized(width: f32, height: f32) -> StyleRefinement {
    StyleRefinement::default()
        .w(px(width))
        .h(px(height))
        .flex_none()
}

/// A cached view of a size of its own, at `left` and `top` of its parent.
fn placed(left: f32, top: f32, width: f32, height: f32) -> StyleRefinement {
    StyleRefinement::default()
        .absolute()
        .left(px(left))
        .top(px(top))
        .w(px(width))
        .h(px(height))
}

/// A cached view that covers the box it sits in.
fn cover() -> StyleRefinement {
    StyleRefinement::default()
        .absolute()
        .top_0()
        .left_0()
        .size_full()
}

/// `rows` rows built by `row`, `WALL_GAP` apart and from the patch's edges.
fn grid_of(rows: usize, row: impl Fn(usize) -> Div) -> Div {
    div()
        .flex()
        .flex_col()
        .gap(px(WALL_GAP))
        .p(px(WALL_GAP))
        .children((0..rows).map(row))
}

/// Line charts that never change.
struct Charts {
    grid: Grid,
}

impl Render for Charts {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        let tiles = self.grid.tiles;
        let (columns, rows) = patchwork_pairs(tiles.columns, tiles.rows);
        let (width, height) = (
            patchwork_double(tiles.width),
            patchwork_double(tiles.height),
        );
        grid_of(rows, move |row| {
            div()
                .flex()
                .gap(px(WALL_GAP))
                .children((0..columns).map(move |column| {
                    let index = row * columns + column;
                    canvas(
                        |_, _, _| {},
                        move |bounds, _, window, _| chart(window, bounds, index),
                    )
                    .flex_none()
                    .w(px(width))
                    .h(px(height))
                }))
        })
    }
}

fn chart(window: &mut Window, bounds: Bounds<gpui::Pixels>, index: usize) {
    let step = f32::from(bounds.size.width) / (PATCHWORK_CHART_POINTS - 1) as f32;
    let height = f32::from(bounds.size.height);
    let mut line = PathBuilder::stroke(px(PATCHWORK_CHART_STROKE));
    for at in 0..PATCHWORK_CHART_POINTS {
        let place = bounds.origin
            + point(
                px(at as f32 * step),
                px(height * (1.0 - patchwork_chart_value(index, at))),
            );
        if at == 0 {
            line.move_to(place);
        } else {
            line.line_to(place);
        }
    }
    if let Ok(path) = line.build() {
        window.paint_path(path, rgb(patchwork_chart_rgb(index)));
    }
}

/// Tiles whose fill changes in turn: each row of fills is a view that the
/// frame notifies when its step moves on, over which the tiles' text lies in
/// a view that never changes.
struct Recolor {
    grid: Grid,
    rows: Vec<Entity<RecolorRow>>,
    text: Entity<Tiles>,
}

struct RecolorRow {
    tiles: PatchTiles,
    row: usize,
    frame: u32,
}

impl Recolor {
    fn new(grid: Grid, frame: u32, cx: &mut App) -> Self {
        let tiles = grid.tiles;
        Self {
            grid,
            rows: (0..tiles.rows)
                .map(|row| cx.new(|_| RecolorRow { tiles, row, frame }))
                .collect(),
            text: cx.new(|_| Tiles {
                tier: grid.tier,
                columns: tiles.columns,
                rows: tiles.rows,
                filled: false,
            }),
        }
    }

    fn step(&self, frame: u32, cx: &mut Context<Self>) {
        let (columns, turn) = (self.grid.tiles.columns, self.grid.tiles.in_turn());
        for (row, view) in self.rows.iter().enumerate() {
            let first = row * columns;
            if turn.step(first, frame) != turn.step(first, frame - 1) {
                view.update(cx, |row, cx| {
                    row.frame = frame;
                    cx.notify();
                });
            }
        }
    }
}

impl Render for Recolor {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        let tiles = self.grid.tiles;
        let row_width = tiles.columns as f32 * (tiles.width + WALL_GAP) - WALL_GAP;
        div()
            .relative()
            .size_full()
            .child(grid_of(tiles.rows, |row| {
                div().child(
                    self.rows[row]
                        .clone()
                        .cached(sized(row_width, tiles.height)),
                )
            }))
            .child(self.text.clone().cached(cover()))
    }
}

impl Render for RecolorRow {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        let (width, height) = (self.tiles.width, self.tiles.height);
        let (row, columns, turn) = (self.row, self.tiles.columns, self.tiles.in_turn());
        let frame = self.frame;
        div()
            .flex()
            .gap(px(WALL_GAP))
            .children((0..columns).map(move |column| {
                div()
                    .flex_none()
                    .w(px(width))
                    .h(px(height))
                    .rounded(px(WALL_CORNER))
                    .bg(rgb(patchwork_recolor_rgb(
                        row * columns + column,
                        frame,
                        turn,
                    )))
            }))
    }
}

/// Cards that move every frame. The stack renders again each frame for the
/// cards' places; each card is a cached view of its own, which GPUI reuses
/// while its place does not change.
struct CardStack {
    grid: Grid,
    cards: Cards,
    frame: u32,
    views: Vec<(usize, Entity<Card>)>,
}

/// One card: its header over its rows of tiles. A card that moves is drawn
/// again each frame, so it is painted on a canvas from text shaped once, which
/// costs no layout.
struct Card {
    grid: Grid,
    cards: Cards,
    id: usize,
    shaped: Option<Arc<ShapedCard>>,
}

/// The text of a card, shaped once: its title and the label and value of
/// each tile.
struct ShapedCard {
    title: ShapedLine,
    tiles: Vec<(usize, ShapedLine, ShapedLine)>,
}

impl CardStack {
    fn new(grid: Grid, frame: u32) -> Self {
        Self {
            grid,
            cards: Cards::new(
                grid.tiles.columns,
                grid.tiles.rows,
                grid.tiles.height,
                grid.height,
            ),
            frame,
            views: Vec::new(),
        }
    }

    fn step(&mut self, frame: u32, cx: &mut Context<Self>) {
        self.frame = frame;
        cx.notify();
    }
}

impl Render for CardStack {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let (grid, cards, frame) = (self.grid, self.cards, self.frame);
        let width = patchwork_reorder_width(cards.columns, grid.tiles.width);
        let mut stack = div().relative().size_full();
        for place in (0..cards.places()).filter_map(|place| cards.place(place, frame)) {
            let view = match self.views.iter().find(|(id, _)| *id == place.id) {
                Some((_, view)) => view.clone(),
                None => {
                    let view = cx.new(|_| Card {
                        grid,
                        cards,
                        id: place.id,
                        shaped: None,
                    });
                    self.views.push((place.id, view.clone()));
                    view
                }
            };
            stack = stack.child(view.cached(placed(WALL_GAP, place.top, width, cards.height)));
        }
        self.views.retain(|(id, _)| cards.top(*id, frame).is_some());
        stack
    }
}

impl Card {
    fn shape(&self, window: &Window) -> ShapedCard {
        let height = self.grid.tiles.height;
        let geometry = TileGeometry::new(height);
        let (label, value) = (geometry.label_size, geometry.value_size);
        let line = |text: String, size: f32, weight: FontWeight| {
            let run = TextRun {
                len: text.len(),
                font: Font {
                    weight,
                    ..gpui::font("Roboto")
                },
                color: gpui::white(),
                background_color: None,
                underline: None,
                strikethrough: None,
            };
            window
                .text_system()
                .shape_line(text.into(), px(size), &[run], None)
        };
        ShapedCard {
            title: line(patchwork_card_title(self.id), label, FontWeight::BOLD),
            tiles: (0..self.cards.tiles())
                .map(|tile| {
                    let index = patchwork_card_tile(self.id, tile, self.cards.tiles());
                    (
                        index,
                        line(wall_label(index), label, FontWeight::BOLD),
                        line(wall_value(index), value, FontWeight::NORMAL),
                    )
                })
                .collect(),
        }
    }
}

impl Render for Card {
    fn render(&mut self, window: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        let shaped = match &self.shaped {
            Some(shaped) => shaped.clone(),
            None => {
                let shaped = Arc::new(self.shape(window));
                self.shaped = Some(shaped.clone());
                shaped
            }
        };
        let (cards, id) = (self.cards, self.id);
        let (width, height) = (self.grid.tiles.width, self.grid.tiles.height);
        canvas(
            |_, _, _| {},
            move |bounds, _, window, cx| {
                let geometry = TileGeometry::new(height);
                let header = Bounds::new(
                    bounds.origin,
                    size(
                        px(patchwork_reorder_width(cards.columns, width)),
                        px(height),
                    ),
                );
                window.paint_quad(
                    fill(header, rgb(patchwork_card_rgb(id))).corner_radii(px(WALL_CORNER)),
                );
                window.with_content_mask(Some(ContentMask { bounds: header }), |window| {
                    let at = header.origin + offset(geometry.label);
                    paint_line(window, cx, &shaped.title, at, geometry.label_line);
                });
                for (tile, (index, label, value)) in shaped.tiles.iter().enumerate() {
                    let (column, row) = (tile % cards.columns, tile / cards.columns + 1);
                    let origin = bounds.origin
                        + point(
                            px(column as f32 * (width + WALL_GAP)),
                            px(row as f32 * (height + WALL_GAP)),
                        );
                    let area = Bounds::new(origin, size(px(width), px(height)));
                    paint_tile(window, cx, area, geometry, *index, label, value);
                }
            },
        )
        .size_full()
    }
}

fn offset((left, top): (f32, f32)) -> gpui::Point<gpui::Pixels> {
    point(px(left), px(top))
}

fn paint_line(
    window: &mut Window,
    cx: &mut App,
    line: &ShapedLine,
    at: gpui::Point<gpui::Pixels>,
    line_height: f32,
) {
    if let Err(error) = line.paint(at, px(line_height), TextAlign::Left, None, window, cx) {
        log::warn!("text not painted: {error}");
    }
}

/// Paints the tile at `area` from its shaped label and value: its parts lie
/// where `tile_box` puts them, clipped to the tile.
fn paint_tile(
    window: &mut Window,
    cx: &mut App,
    area: Bounds<gpui::Pixels>,
    geometry: TileGeometry,
    index: usize,
    label: &ShapedLine,
    value: &ShapedLine,
) {
    window.paint_quad(fill(area, rgb(wall_fill_rgb(index))).corner_radii(px(WALL_CORNER)));
    window.with_content_mask(Some(ContentMask { bounds: area }), |window| {
        let dot = Bounds::new(
            area.origin + offset(geometry.dot_at),
            size(px(geometry.dot), px(geometry.dot)),
        );
        paint_line(
            window,
            cx,
            label,
            area.origin + offset(geometry.label),
            geometry.label_line,
        );
        window.paint_quad(fill(dot, rgb(wall_dot_rgb(index))).corner_radii(px(geometry.dot / 2.0)));
        paint_line(
            window,
            cx,
            value,
            area.origin + offset(geometry.value),
            geometry.value_line,
        );
    });
}

/// A wrapping row of items, two of which change places every frame. Each
/// item is a cached view; the row lays them out again each frame.
struct Reorder {
    grid: Grid,
    frame: u32,
    /// Each item's width and view, by item number.
    items: Vec<(f32, Entity<Item>)>,
}

/// One tile of the reorder patch, 1 to 3 tiles wide.
struct Item {
    index: usize,
    width: f32,
    height: f32,
}

impl Reorder {
    fn new(grid: Grid, frame: u32, cx: &mut App) -> Self {
        let (tile_width, height) = (grid.tiles.width, grid.tiles.height);
        Self {
            grid,
            frame,
            items: (0..patchwork_reorder_count(grid.tiles.count()))
                .map(|index| {
                    let width = patchwork_reorder_width(patchwork_reorder_units(index), tile_width);
                    (
                        width,
                        cx.new(|_| Item {
                            index,
                            width,
                            height,
                        }),
                    )
                })
                .collect(),
        }
    }

    fn step(&mut self, frame: u32, cx: &mut Context<Self>) {
        self.frame = frame;
        cx.notify();
    }
}

impl Render for Reorder {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        let count = self.items.len();
        let height = self.grid.tiles.height;
        div()
            .size_full()
            .flex()
            .flex_wrap()
            .content_start()
            .gap(px(WALL_GAP))
            .p(px(WALL_GAP))
            .children((0..count).map(|position| {
                let item = patchwork_reorder_item(position, self.frame, count);
                let (width, view) = &self.items[item];
                view.clone().cached(sized(*width, height))
            }))
    }
}

impl Render for Item {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        tile(self.index, self.width, self.height)
    }
}

/// Nests of boxes around digits that change in turn: the boxes are a cached
/// view that never changes, the digits a row of cached views that the frame
/// notifies when the row's number moves on.
struct Deep {
    grid: Grid,
    boxes: Entity<Nests>,
    rows: Vec<Entity<Digits>>,
}

struct Nests {
    grid: Grid,
}

struct Digits {
    grid: Grid,
    row: usize,
    frame: u32,
}

impl Deep {
    fn new(grid: Grid, frame: u32, cx: &mut App) -> Self {
        let (_, rows) = patchwork_pairs(grid.tiles.columns, grid.tiles.rows);
        Self {
            grid,
            boxes: cx.new(|_| Nests { grid }),
            rows: (0..rows)
                .map(|row| cx.new(|_| Digits { grid, row, frame }))
                .collect(),
        }
    }

    fn turn(grid: Grid) -> InTurn {
        let (columns, rows) = patchwork_pairs(grid.tiles.columns, grid.tiles.rows);
        InTurn { columns, rows }
    }

    fn step(&self, frame: u32, cx: &mut Context<Self>) {
        let turn = Self::turn(self.grid);
        for (row, view) in self.rows.iter().enumerate() {
            let first = row * turn.columns;
            if turn.step(first, frame) != turn.step(first, frame - 1) {
                view.update(cx, |digits, cx| {
                    digits.frame = frame;
                    cx.notify();
                });
            }
        }
    }
}

impl Render for Deep {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        let tiles = self.grid.tiles;
        let (columns, _) = patchwork_pairs(tiles.columns, tiles.rows);
        let (width, height) = (
            patchwork_double(tiles.width),
            patchwork_double(tiles.height),
        );
        let row_width = columns as f32 * (width + WALL_GAP) - WALL_GAP;
        let mut deep = div()
            .relative()
            .size_full()
            .child(self.boxes.clone().cached(cover()));
        for (row, digits) in self.rows.iter().enumerate() {
            let (left, top) = (WALL_GAP, WALL_GAP + row as f32 * (height + WALL_GAP));
            deep = deep.child(digits.clone().cached(placed(left, top, row_width, height)));
        }
        deep
    }
}

// Box `level` of nest `index` and everything inside it.
fn nest(index: usize, level: usize, padding: f32) -> Div {
    div()
        .size_full()
        .p(px(padding))
        .bg(rgb(patchwork_deep_rgb(index, level)))
        .when(level + 1 < PATCHWORK_DEPTH, |outer| {
            outer.child(nest(index, level + 1, padding))
        })
}

impl Render for Nests {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        let tiles = self.grid.tiles;
        let (columns, rows) = patchwork_pairs(tiles.columns, tiles.rows);
        let (width, height) = (
            patchwork_double(tiles.width),
            patchwork_double(tiles.height),
        );
        let padding = wall_padding(tiles.height);
        grid_of(rows, move |row| {
            div()
                .flex()
                .gap(px(WALL_GAP))
                .children((0..columns).map(move |column| {
                    div().flex_none().w(px(width)).h(px(height)).child(nest(
                        row * columns + column,
                        0,
                        padding,
                    ))
                }))
        })
    }
}

impl Render for Digits {
    fn render(&mut self, window: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        let tiles = self.grid.tiles;
        let turn = Deep::turn(self.grid);
        let (width, size) = (
            patchwork_double(tiles.width),
            wall_text_size(tiles.height).0,
        );
        // Each box lies the tile's padding inside the one around it, which
        // GPUI's layout rounds to whole device pixels.
        let scale = window.scale_factor();
        let padding = (wall_padding(tiles.height) * scale).round() / scale;
        let inside = PATCHWORK_DEPTH as f32 * padding;
        let (row, frame) = (self.row, self.frame);
        div()
            .relative()
            .size_full()
            .children((0..turn.columns).map(move |column| {
                div()
                    .absolute()
                    .left(px(column as f32 * (width + WALL_GAP) + inside))
                    .top(px(inside))
                    .whitespace_nowrap()
                    .text_size(px(size))
                    .font_weight(FontWeight::BOLD)
                    .child(patchwork_deep_value(
                        row * turn.columns + column,
                        frame,
                        turn,
                    ))
            }))
    }
}

/// A list of rows of tiles that scrolls by one row every frame: GPUI's
/// `uniform_list`, which draws the rows on screen.
struct Scroll {
    grid: Grid,
    handle: UniformListScrollHandle,
    total: usize,
}

impl Scroll {
    fn new(grid: Grid, frame: u32) -> Self {
        let scroll = Self {
            grid,
            handle: UniformListScrollHandle::new(),
            total: patchwork_scroll_rows(grid.tiles.columns),
        };
        scroll.scroll_to(frame);
        scroll
    }

    fn scroll_to(&self, frame: u32) {
        let first = patchwork_scroll_first(frame, self.total, self.grid.tiles.rows);
        self.handle
            .scroll_to_item_strict(first, ScrollStrategy::Top);
    }

    fn step(&mut self, frame: u32, cx: &mut Context<Self>) {
        self.scroll_to(frame);
        cx.notify();
    }
}

impl Render for Scroll {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        let tiles = self.grid.tiles;
        div().size_full().child(
            uniform_list("scroll-rows", self.total, move |range, _, _| {
                range
                    .map(|row| {
                        div()
                            .h(px(tiles.height + WALL_GAP))
                            .pt(px(WALL_GAP))
                            .pl(px(WALL_GAP))
                            .flex()
                            .gap(px(WALL_GAP))
                            .children((0..tiles.columns).map(move |column| {
                                tile(row * tiles.columns + column, tiles.width, tiles.height)
                            }))
                    })
                    .collect()
            })
            .track_scroll(&self.handle)
            .size_full(),
        )
    }
}

/// One paragraph, one word of which changes every frame.
struct Paragraph {
    grid: Grid,
    text: SharedString,
}

impl Paragraph {
    fn text(grid: Grid, frame: u32) -> SharedString {
        patchwork_paragraph(frame, patchwork_paragraph_words(grid.tiles.count())).into()
    }

    fn step(&mut self, frame: u32, cx: &mut Context<Self>) {
        self.text = Self::text(self.grid, frame);
        cx.notify();
    }
}

impl Render for Paragraph {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        div()
            .size_full()
            .p(px(WALL_GAP))
            .text_size(px(wall_text_size(self.grid.tiles.height).0))
            .child(self.text.clone())
    }
}

/// Translucent tiles lying over one another, in rows that are cached views:
/// the frame notifies the row of the tile that turns translucent and the row
/// of the tile that turns opaque. A row lies wholly under the next, as its
/// tiles come before the next row's.
struct Pile {
    grid: Grid,
    opaque: usize,
    rows: Vec<Entity<PileRow>>,
}

struct PileRow {
    grid: Grid,
    row: usize,
    /// The column of the row's opaque tile.
    opaque: Option<usize>,
}

impl Pile {
    fn new(grid: Grid, frame: u32, cx: &mut App) -> Self {
        let (columns, rows) = patchwork_pile_grid(grid.tiles.columns, grid.tiles.rows);
        let opaque = patchwork_pile_opaque(frame, columns * rows);
        Self {
            grid,
            opaque,
            rows: (0..rows)
                .map(|row| {
                    let opaque = (opaque / columns == row).then_some(opaque % columns);
                    cx.new(|_| PileRow { grid, row, opaque })
                })
                .collect(),
        }
    }

    fn columns(&self) -> usize {
        patchwork_pile_grid(self.grid.tiles.columns, self.grid.tiles.rows).0
    }

    fn step(&mut self, frame: u32, cx: &mut Context<Self>) {
        let (columns, rows) = patchwork_pile_grid(self.grid.tiles.columns, self.grid.tiles.rows);
        let opaque = patchwork_pile_opaque(frame, columns * rows);
        if opaque != self.opaque {
            for (tile, column) in [(self.opaque, None), (opaque, Some(opaque % columns))] {
                self.rows[tile / columns].update(cx, |row, cx| {
                    row.opaque = column;
                    cx.notify();
                });
            }
            self.opaque = opaque;
        }
    }
}

impl Render for Pile {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        let tiles = self.grid.tiles;
        let width =
            (self.columns() - 1) as f32 * (tiles.width + WALL_GAP) + patchwork_double(tiles.width);
        let height = patchwork_double(tiles.height);
        div()
            .relative()
            .size_full()
            .children(self.rows.iter().enumerate().map(|(row, view)| {
                let (left, top) = self.grid.at(0, row);
                view.clone().cached(placed(left, top, width, height))
            }))
    }
}

impl Render for PileRow {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        let tiles = self.grid.tiles;
        let (columns, _) = patchwork_pile_grid(tiles.columns, tiles.rows);
        let (width, height) = (
            patchwork_double(tiles.width),
            patchwork_double(tiles.height),
        );
        let (row, opaque) = (self.row, self.opaque);
        div()
            .relative()
            .size_full()
            .children((0..columns).map(move |column| {
                let index = row * columns + column;
                let fill = if opaque == Some(column) {
                    rgb(PATCHWORK_PILE_OPAQUE_RGB)
                } else {
                    rgb(wall_fill_rgb(index)).opacity(f32::from(PATCHWORK_PILE_ALPHA) / 255.0)
                };
                tile_box(index, width, height, tiles.height, Some(fill))
                    .absolute()
                    .left(px(column as f32 * (tiles.width + WALL_GAP)))
                    .top(px(0.0))
            }))
    }
}

/// The window's view: the top bar, the clock, the patches and the chat. It
/// renders once a frame; the frame's step notifies the views that changed.
struct Patchwork {
    frame: FrameIndex,
    patch_size: (f32, f32),
    chat_height: f32,
    chat: Chat,
    still: Entity<Tiles>,
    charts: Entity<Charts>,
    recolor: Entity<Recolor>,
    cards: Entity<CardStack>,
    reorder: Entity<Reorder>,
    deep: Entity<Deep>,
    scroll: Entity<Scroll>,
    paragraph: Entity<Paragraph>,
    pile: Entity<Pile>,
    list: Entity<ChatList>,
    status: Entity<ChatStatus>,
}

impl Patchwork {
    // Moves on to the next frame: the frame's views take their changes and
    // the chat takes the frame's tokens.
    fn step(&mut self, cx: &mut Context<Self>) {
        self.frame.advance();
        let frame = self.frame.get();
        self.recolor.update(cx, |patch, cx| patch.step(frame, cx));
        self.cards.update(cx, |patch, cx| patch.step(frame, cx));
        self.reorder.update(cx, |patch, cx| patch.step(frame, cx));
        self.deep.update(cx, |patch, cx| patch.step(frame, cx));
        self.scroll.update(cx, |patch, cx| patch.step(frame, cx));
        self.paragraph.update(cx, |patch, cx| patch.step(frame, cx));
        self.pile.update(cx, |patch, cx| patch.step(frame, cx));
        let token = self.chat.take();
        self.list.update(cx, |list, cx| list.show(token, frame, cx));
        self.status
            .update(cx, |status, cx| status.show(token, frame, cx));
        cx.notify();
    }
}

impl Render for Patchwork {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        if !self.frame.frozen() {
            let window_view = cx.entity();
            window.on_next_frame(move |_, cx| window_view.update(cx, |view, cx| view.step(cx)));
        }
        let (width, height) = self.patch_size;
        let patches: [AnyView; PATCH_COLUMNS * PATCH_ROWS] = [
            self.still.clone().into(),
            self.charts.clone().into(),
            self.recolor.clone().into(),
            self.cards.clone().into(),
            self.reorder.clone().into(),
            self.deep.clone().into(),
            self.scroll.clone().into(),
            self.paragraph.clone().into(),
            self.pile.clone().into(),
        ];
        let status = CHAT_STATUS_HEIGHT;
        div()
            .size_full()
            .flex()
            .flex_col()
            .font_family("Roboto")
            .line_height(relative(LINE_HEIGHT))
            .text_color(gpui::white())
            .bg(rgb(WALL_BACKGROUND_RGB))
            .child(top_bar("Patchwork"))
            .child(clock(self.frame.get()))
            .child(
                div()
                    .flex_1()
                    .min_h(px(0.0))
                    .flex()
                    .justify_between()
                    .child(div().flex_none().children((0..PATCH_ROWS).map(|row| {
                        div().flex().children(
                            patches[row * PATCH_COLUMNS..(row + 1) * PATCH_COLUMNS]
                                .iter()
                                .map(|patch| {
                                    // A patch clips what lies past its box.
                                    div()
                                        .flex_none()
                                        .w(px(width))
                                        .h(px(height))
                                        .overflow_hidden()
                                        .child(patch.clone().cached(whole()))
                                }),
                        )
                    })))
                    .child(
                        div()
                            .flex_none()
                            .w(px(CHAT_WIDTH))
                            .h(px(self.chat_height))
                            .child(
                                self.list
                                    .clone()
                                    .cached(sized(CHAT_WIDTH, self.chat_height - status)),
                            )
                            .child(self.status.clone().cached(sized(CHAT_WIDTH, status))),
                    ),
            )
    }
}

/// Opens the patchwork's window.
pub fn open(load: Launch, cx: &mut App) {
    let tier = wall_tier(load.tier);
    let (window_width, window_height) = DESKTOP_WINDOW;
    let (area_width, area_height) = (
        window_width as f32,
        window_height as f32 - 56.0 - WALL_CLOCK_HEIGHT,
    );
    let patch_width = ((area_width - CHAT_WIDTH) / PATCH_COLUMNS as f32).floor();
    let patch_height = (area_height / PATCH_ROWS as f32).floor();
    let grid = Grid {
        tier,
        tiles: PatchTiles::new(tier, patch_width, patch_height),
        height: patch_height,
    };
    let bounds = Bounds::centered(
        None,
        size(px(window_width as f32), px(window_height as f32)),
        cx,
    );
    let options = WindowOptions {
        window_bounds: Some(WindowBounds::Windowed(bounds)),
        titlebar: Some(TitlebarOptions {
            title: Some("Patchwork".into()),
            ..Default::default()
        }),
        is_resizable: false,
        // The window draws every frame even when another window is in
        // front, as the other frameworks' windows do.
        inactive_frame_interval: None,
        ..Default::default()
    };
    // The data thread starts before the window is built, so that the first
    // frame takes its tokens as every frame does.
    let chat = Chat::start();
    if let Err(error) = cx.open_window(options, |_, cx| {
        // The first frame is frame 1, shown before any step.
        let mut frame = FrameIndex::new(load.freeze);
        frame.advance();
        let first = frame.get();
        let tiles = grid.tiles;
        let still = cx.new(|_| Tiles {
            tier,
            columns: tiles.columns,
            rows: tiles.rows,
            filled: true,
        });
        let charts = cx.new(|_| Charts { grid });
        let recolor = cx.new(|cx| Recolor::new(grid, first, cx));
        let cards = cx.new(|_| CardStack::new(grid, first));
        let reorder = cx.new(|cx| Reorder::new(grid, first, cx));
        let deep = cx.new(|cx| Deep::new(grid, first, cx));
        let scroll = cx.new(|_| Scroll::new(grid, first));
        let paragraph = cx.new(|_| Paragraph {
            grid,
            text: Paragraph::text(grid, first),
        });
        let pile = cx.new(|cx| Pile::new(grid, first, cx));
        let token = chat.take();
        let list = cx.new(|_| ChatList::new(gif_frames(), token, first));
        let status = cx.new(|_| ChatStatus::new(token, first));
        cx.new(|_| Patchwork {
            frame,
            patch_size: (patch_width, patch_height),
            chat_height: area_height,
            chat,
            still,
            charts,
            recolor,
            cards,
            reorder,
            deep,
            scroll,
            paragraph,
            pile,
            list,
            status,
        })
    }) {
        log::error!("no window: {error}");
    }
    cx.activate(true);
}
