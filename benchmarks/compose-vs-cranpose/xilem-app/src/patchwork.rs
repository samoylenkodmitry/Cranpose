//! The patchwork in Xilem (`../README.md`): nine patches of the still wall's
//! tiles beside a chat. Two patches never change; in the others a frame
//! changes a small part.
//!
//! In Xilem's idiom the app logic builds the tree of views from the frame and
//! `memoize` skips what the frame did not change, so a frame rebuilds only
//! the view of the row, card, item or block whose data changed. The still
//! patch and the charts are memoized on the grid. A recolor or deep row, a
//! card, a reorder item and a pile tile are memoized on their own data. A
//! tile keeps its parts apart from its fill, so a new fill rebuilds none of
//! them. Cards, pile tiles and list rows sit at their places by the
//! transform of a holder, which moves them without a layout. The scroll patch
//! is Xilem's virtual list, scrolled to the frame's row. The paragraph is one
//! label and each frame sets its text. The chat is in `chat.rs`.

use perf_data::*;
use xilem::{
    AnyWidgetView, Color, Vec2, WidgetView,
    core::memoize,
    masonry::properties::{LineBreaking, types::UnitPoint},
    style::{Padding, Style as _},
    view::{
        CrossAxisAlignment, FlexExt as _, flex_col, flex_row, sized_box, virtual_scroll, zstack,
    },
};

use super::{
    chat::chat,
    px, rgb, styled, text, top_bar,
    wall::{BACKGROUND, INK, clock_line, tile_on, tile_row, tiles},
    widgets::{Hold, Picture, anchored, drawing, flow, frame_clock, hold, ignore_height},
};

/// What the patchwork reads, which each frame advances.
pub(crate) struct Patchwork {
    frame: FrameIndex,
    feed: Option<ChatFeed>,
    pub(crate) token: Option<ChatToken>,
}

impl Patchwork {
    pub(crate) fn new() -> Self {
        Self {
            frame: FrameIndex::new(perf_data::launch().freeze),
            feed: ChatFeed::start()
                .map_err(|error| log::warn!("chat data thread: {error}"))
                .ok(),
            token: None,
        }
    }

    // Advances the frame unless the patchwork holds still, and takes the
    // frame's tokens from the data thread; answers whether the frame moved.
    fn advance(&mut self) -> bool {
        let shown = self.frame.get();
        self.frame.advance();
        let moved = self.frame.get() != shown;
        if moved && let Some(feed) = &self.feed {
            for _ in 0..CHAT_TOKENS_PER_FRAME {
                match feed.take() {
                    Some(token) => self.token = Some(token),
                    None => break,
                }
            }
        }
        moved
    }
}

/// One patch: its tiles and its size in points.
#[derive(Clone, Copy, PartialEq, Debug)]
struct Grid {
    tiles: PatchTiles,
    width: f32,
    height: f32,
}

type Boxed = Box<AnyWidgetView<Patchwork>>;

pub(crate) fn patchwork_logic(patchwork: &mut Patchwork) -> impl WidgetView<Patchwork> + use<> {
    let frame = patchwork.frame.get();
    let tier = wall_tier(perf_data::launch().tier);
    let (width, height) = DESKTOP_WINDOW;
    let patch_width = ((width as f32 - CHAT_WIDTH) / PATCH_COLUMNS as f32).floor();
    let patch_height = ((height as f32 - 56.0 - WALL_CLOCK_HEIGHT) / PATCH_ROWS as f32).floor();
    let grid = Grid {
        tiles: PatchTiles::new(tier, patch_width, patch_height),
        width: patch_width,
        height: patch_height,
    };
    let patches = flex_col(
        Patch::ALL
            .chunks(PATCH_COLUMNS)
            .map(|row| {
                flex_row(
                    row.iter()
                        .map(|patch| framed(grid, patch_view(*patch, grid, frame)))
                        .collect::<Vec<_>>(),
                )
                .gap(px(0.0))
            })
            .collect::<Vec<_>>(),
    )
    .gap(px(0.0));
    flex_col((
        frame_clock(Patchwork::advance),
        top_bar("Patchwork"),
        clock_line(frame),
        flex_row((patches, chat(frame, patchwork.token)))
            .gap(px(0.0))
            .cross_axis_alignment(CrossAxisAlignment::Start)
            .flex(1.0),
    ))
    .gap(px(0.0))
    .cross_axis_alignment(CrossAxisAlignment::Start)
    .background_color(BACKGROUND)
}

// A patch's own box: the patch's size, which cuts what lies past it.
fn framed(grid: Grid, content: impl WidgetView<Patchwork>) -> impl WidgetView<Patchwork> {
    sized_box(hold(content, Hold::Clip, ignore_height))
        .width(px(grid.width))
        .height(px(grid.height))
}

fn patch_view(patch: Patch, grid: Grid, frame: u32) -> Boxed {
    match patch {
        Patch::Still => Box::new(still(grid)),
        Patch::Charts => Box::new(charts(grid)),
        Patch::Recolor => Box::new(recolor(grid, frame)),
        Patch::Cards => Box::new(cards(grid, frame)),
        Patch::Reorder => Box::new(reorder(grid, frame)),
        Patch::Deep => Box::new(deep(grid, frame)),
        Patch::Scroll => Box::new(scroll(grid, frame)),
        Patch::Paragraph => Box::new(paragraph(grid, frame)),
        Patch::Pile => Box::new(pile(grid, frame)),
    }
}

// Tiles that never change.
fn still(grid: Grid) -> impl WidgetView<Patchwork> + use<> {
    memoize(grid.tiles, |tiles_| {
        tiles(tiles_.columns, tiles_.rows, tiles_.width, tiles_.height)
    })
}

// Line charts that never change: one drawing paints them all, and Masonry
// keeps its picture.
fn charts(grid: Grid) -> impl WidgetView<Patchwork> + use<> {
    memoize(grid.tiles, |tiles| {
        let (columns, rows) = patchwork_pairs(tiles.columns, tiles.rows);
        drawing(Picture::Charts {
            columns,
            rows,
            width: f64::from(patchwork_double(tiles.width)),
            height: f64::from(patchwork_double(tiles.height)),
        })
    })
}

// A frame in which every row of `turn` is at `step`, for the data functions
// that take a frame.
fn in_step(turn: InTurn, step: usize) -> u32 {
    (step * turn.rows) as u32
}

// Tiles whose fill changes in turn: a row is memoized on its step, so a
// frame rebuilds the row whose fills changed.
fn recolor(grid: Grid, frame: u32) -> impl WidgetView<Patchwork> + use<> {
    let turn = grid.tiles.in_turn();
    flex_col(
        (0..grid.tiles.rows)
            .map(|row| {
                memoize(
                    (grid.tiles, row, turn.step(row * grid.tiles.columns, frame)),
                    |&(tiles, row, step)| {
                        let turn = tiles.in_turn();
                        tile_row(
                            tiles.columns,
                            tiles.width,
                            tiles.height,
                            &|column| row * tiles.columns + column,
                            &|index| rgb(patchwork_recolor_rgb(index, in_step(turn, step), turn)),
                        )
                    },
                )
            })
            .collect::<Vec<_>>(),
    )
    .gap(px(WALL_GAP))
    .cross_axis_alignment(CrossAxisAlignment::Start)
    .padding(f64::from(WALL_GAP))
}

// Cards at the frame's places. A card is memoized on its number and sits
// at its place by its holder's transform, so within a move a frame moves the
// cards and rebuilds none of them. Xilem's sequences have no keys: when a
// move gives a place another card, the place rebuilds.
fn cards(grid: Grid, frame: u32) -> impl WidgetView<Patchwork> + use<> {
    let cards = Cards::new(
        grid.tiles.columns,
        grid.tiles.rows,
        grid.tiles.height,
        grid.height,
    );
    zstack(
        (0..cards.places())
            .map(|place| {
                cards.place(place, frame).map(|card| {
                    hold(
                        memoize(
                            (card.id, cards, grid.tiles.width, grid.tiles.height),
                            |&(id, cards, width, height)| card_view(id, cards, width, height),
                        ),
                        Hold::Turn {
                            offset: Vec2::new(f64::from(WALL_GAP), f64::from(card.top)),
                            radians: 0.0,
                        },
                        ignore_height,
                    )
                })
            })
            .collect::<Vec<_>>(),
    )
    .alignment(UnitPoint::TOP_LEFT)
}

// Card `id`: its header over its rows of tiles.
fn card_view(
    id: usize,
    cards: Cards,
    width: f32,
    height: f32,
) -> impl WidgetView<Patchwork> + use<> {
    let header = sized_box(text(
        patchwork_card_title(id),
        wall_text_size(height).0,
        INK,
        true,
    ))
    .width(px(patchwork_reorder_width(cards.columns, width)))
    .height(px(height))
    .padding(f64::from(wall_padding(height)))
    .corner_radius(f64::from(WALL_CORNER))
    .background_color(rgb(patchwork_card_rgb(id)));
    let rows = (0..cards.rows)
        .map(|row| {
            tile_row(
                cards.columns,
                width,
                height,
                &|column| patchwork_card_tile(id, row * cards.columns + column, cards.tiles()),
                &|index| rgb(wall_fill_rgb(index)),
            )
        })
        .collect::<Vec<_>>();
    flex_col((header, rows))
        .gap(px(WALL_GAP))
        .cross_axis_alignment(CrossAxisAlignment::Start)
}

// A wrapping row of items in the frame's order, each memoized on its number:
// a frame rebuilds the places whose item changed.
fn reorder(grid: Grid, frame: u32) -> impl WidgetView<Patchwork> + use<> {
    let count = patchwork_reorder_count(grid.tiles.count());
    sized_box(flow(
        (0..count)
            .map(|position| {
                memoize(
                    (
                        patchwork_reorder_item(position, frame, count),
                        grid.tiles.width,
                        grid.tiles.height,
                    ),
                    |&(item, width, height)| {
                        let width = patchwork_reorder_width(patchwork_reorder_units(item), width);
                        tile_on(item, width, height, height, rgb(wall_fill_rgb(item)))
                    },
                )
            })
            .collect::<Vec<_>>(),
        f64::from(WALL_GAP),
    ))
    .padding(f64::from(WALL_GAP))
}

// Nests of boxes around digits that change in turn: a row of nests is
// memoized on its step. A nest holds its digits inside its boxes, so Xilem
// rebuilds all twelve views above them to change them.
fn deep(grid: Grid, frame: u32) -> impl WidgetView<Patchwork> + use<> {
    let (columns, rows) = patchwork_pairs(grid.tiles.columns, grid.tiles.rows);
    let turn = InTurn { columns, rows };
    flex_col(
        (0..rows)
            .map(|row| {
                memoize(
                    (grid.tiles, row, turn.step(row * columns, frame)),
                    |&(tiles, row, step)| {
                        let (columns, rows) = patchwork_pairs(tiles.columns, tiles.rows);
                        let turn = InTurn { columns, rows };
                        flex_row(
                            (row * columns..(row + 1) * columns)
                                .map(|index| {
                                    nest(
                                        index,
                                        patchwork_deep_value(index, in_step(turn, step), turn),
                                        tiles,
                                    )
                                })
                                .collect::<Vec<_>>(),
                        )
                        .gap(px(WALL_GAP))
                    },
                )
            })
            .collect::<Vec<_>>(),
    )
    .gap(px(WALL_GAP))
    .cross_axis_alignment(CrossAxisAlignment::Start)
    .padding(f64::from(WALL_GAP))
}

// `PATCHWORK_DEPTH` boxes, each inside the one around it, around `digits`.
fn nest(index: usize, digits: String, tiles: PatchTiles) -> impl WidgetView<Patchwork> + use<> {
    let padding = f64::from(wall_padding(tiles.height));
    let mut inner: Boxed = Box::new(text(digits, wall_text_size(tiles.height).0, INK, true));
    for level in (0..PATCHWORK_DEPTH).rev() {
        inner = Box::new(
            sized_box(inner)
                .expand()
                .padding(padding)
                .background_color(rgb(patchwork_deep_rgb(index, level))),
        );
    }
    sized_box(inner)
        .width(px(patchwork_double(tiles.width)))
        .height(px(patchwork_double(tiles.height)))
}

// Xilem's virtual list, scrolled to the frame's first row. A row is memoized
// on its number, so a frame builds the row that came into the list's range.
fn scroll(grid: Grid, frame: u32) -> impl WidgetView<Patchwork> + use<> {
    let total = patchwork_scroll_rows(grid.tiles.columns);
    let first = patchwork_scroll_first(frame, total, grid.tiles.rows);
    let tiles = grid.tiles;
    anchored(
        virtual_scroll(0..total as i64, move |_: &mut Patchwork, row| {
            memoize((tiles, row), |&(tiles, row)| {
                scroll_row(tiles, row as usize)
            })
        }),
        first as i64,
    )
}

// Row `row` of the scroll patch with the gap above and left of it that the
// other apps' lists keep.
fn scroll_row(tiles: PatchTiles, row: usize) -> impl WidgetView<Patchwork> + use<> {
    let gap = f64::from(WALL_GAP);
    sized_box(tile_row(
        tiles.columns,
        tiles.width,
        tiles.height,
        &|column| row * tiles.columns + column,
        &|index| rgb(wall_fill_rgb(index)),
    ))
    .padding(Padding {
        top: gap,
        bottom: 0.0,
        left: gap,
        right: 0.0,
    })
}

// One paragraph, one word of which changes every frame: Xilem has no text
// that changes by the word, so the label takes the whole text.
fn paragraph(grid: Grid, frame: u32) -> impl WidgetView<Patchwork> + use<> {
    sized_box(
        styled(
            patchwork_paragraph(frame, patchwork_paragraph_words(grid.tiles.count())),
            wall_text_size(grid.tiles.height).0,
            false,
        )
        .line_break_mode(LineBreaking::WordWrap)
        .color(INK),
    )
    .padding(f64::from(WALL_GAP))
}

// Translucent tiles lying over one another, each memoized on whether it is
// opaque: a frame rebuilds the two tiles that changed.
fn pile(grid: Grid, frame: u32) -> impl WidgetView<Patchwork> + use<> {
    let (columns, rows) = patchwork_pile_grid(grid.tiles.columns, grid.tiles.rows);
    let opaque = patchwork_pile_opaque(frame, columns * rows);
    zstack(
        (0..columns * rows)
            .map(|index| {
                memoize(
                    (index, index == opaque, grid.tiles, columns),
                    |&(index, opaque, tiles, columns)| pile_tile(index, opaque, tiles, columns),
                )
            })
            .collect::<Vec<_>>(),
    )
    .alignment(UnitPoint::TOP_LEFT)
}

fn pile_tile(
    index: usize,
    opaque: bool,
    tiles: PatchTiles,
    columns: usize,
) -> impl WidgetView<Patchwork> + use<> {
    let fill = if opaque {
        rgb(PATCHWORK_PILE_OPAQUE_RGB)
    } else {
        let [red, green, blue] = wall_fill_rgb(index);
        Color::from_rgba8(red, green, blue, PATCHWORK_PILE_ALPHA)
    };
    hold(
        tile_on(
            index,
            patchwork_double(tiles.width),
            patchwork_double(tiles.height),
            tiles.height,
            fill,
        ),
        Hold::Turn {
            offset: Vec2::new(
                f64::from(WALL_GAP + (index % columns) as f32 * (tiles.width + WALL_GAP)),
                f64::from(WALL_GAP + (index / columns) as f32 * (tiles.height + WALL_GAP)),
            ),
            radians: 0.0,
        },
        ignore_height,
    )
}
