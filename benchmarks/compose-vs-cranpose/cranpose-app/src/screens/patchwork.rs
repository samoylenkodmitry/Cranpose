//! The patchwork (`../../../README.md`): nine patches of the still wall's
//! tiles. Two never change; in the others a frame changes a small part.
//!
//! In Cranpose's idiom a tile, a cell or a nest reads a state derived from
//! the frame, which changes only when its own content does: a frame composes
//! or draws again the part that changed and nothing else. The cards move
//! in graphics layers, which compose and draw nothing again. The reorder
//! patch's row composes again each frame with its items under their keys,
//! and the pile patch in one call whose tiles skip but for the two whose
//! fill changes.

use std::rc::Rc;

use cranpose::{LazyItems, prelude::*};
use cranpose_foundation::lazy::rememberLazyListState;
use cranpose_ui::widgets::{FlowRow, FlowSpec};

use super::{
    chat::{Chat, ChatShown},
    text_style,
    wall::{BACKGROUND, Clock, INK, ONE_LINE, Tile, Wall, rgb, tile_content, tile_rows},
};
use crate::data::{
    self, Cards, ChatToken, InTurn, PATCH_COLUMNS, PATCH_ROWS, PATCHWORK_CHART_POINTS,
    PATCHWORK_DEPTH, Patch, PatchTiles, WALL_GAP,
};

/// The patchwork's tier and the frame it freezes on; 0 runs on.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct PatchworkLoad {
    pub tier: usize,
    pub freeze: u32,
}

// One patch: its tiles and its size in points.
#[derive(Clone, Copy, PartialEq, Debug)]
struct Grid {
    tiles: PatchTiles,
    patch_width: f32,
    patch_height: f32,
}

impl Grid {
    // A patch's own box: the patch's size, which cuts what lies past it.
    fn patch(self) -> Modifier {
        Modifier::empty()
            .size_points(self.patch_width, self.patch_height)
            .clip_to_bounds()
    }
}

#[composable]
pub fn PatchworkScreen(load: PatchworkLoad) {
    let tier = data::wall_tier(load.tier);
    let frame = rememberMutableStateOf(|| 0u32);
    Column(
        Modifier::empty().fill_max_size().background(BACKGROUND),
        ColumnSpec::default(),
        move || {
            Clock(frame);
            BoxWithConstraints(
                Modifier::empty().fill_max_width().weight(1.0),
                move |scope| {
                    let patches_width = scope.max_width().0 - data::CHAT_WIDTH;
                    let patch_width = (patches_width / PATCH_COLUMNS as f32).floor();
                    let patch_height = (scope.max_height().0 / PATCH_ROWS as f32).floor();
                    Patches(
                        frame,
                        load,
                        Grid {
                            tiles: PatchTiles::new(tier, patch_width, patch_height),
                            patch_width,
                            patch_height,
                        },
                    );
                },
            );
        },
    );
}

#[composable]
fn Patches(frame: MutableState<u32>, load: PatchworkLoad, grid: Grid) {
    let freeze = load.freeze;
    let list_state = rememberLazyListState();
    let chat = ChatShown {
        blocks: rememberMutableStateOf(|| 0usize),
        last: rememberMutableStateOf(|| 0usize),
        token: rememberMutableStateOf(ChatToken::default),
        list: rememberLazyListState(),
    };
    let feed = remember(|| {
        data::ChatFeed::start()
            .map_err(|error| log::warn!("chat data thread: {error}"))
            .ok()
            .map(Rc::new)
    })
    .with(|feed| feed.clone());
    let scroll_rows = data::patchwork_scroll_rows(grid.tiles.columns);
    LaunchedEffect(
        (freeze, grid.tiles.columns, grid.tiles.rows),
        async move |scope| {
            let clock = scope.runtime().frame_clock();
            let mut index = 0u32;
            loop {
                clock.next_frame().await;
                index += 1;
                frame.set(index);
                list_state.scroll_to_item(
                    data::patchwork_scroll_first(index, scroll_rows, grid.tiles.rows),
                    0.0,
                );
                if let Some(feed) = &feed {
                    chat.advance(feed);
                }
                if freeze > 0 && index >= freeze {
                    log::info!("PERF frozen frame={index}");
                    break;
                }
            }
        },
    );
    Row(Modifier::empty(), RowSpec::default(), move || {
        Column(Modifier::empty(), ColumnSpec::default(), move || {
            for row in 0..PATCH_ROWS {
                Row(Modifier::empty(), RowSpec::default(), move || {
                    for patch in &Patch::ALL[row * PATCH_COLUMNS..(row + 1) * PATCH_COLUMNS] {
                        show(*patch, frame, list_state, grid);
                    }
                });
            }
        });
        Chat(frame, chat);
    });
}

// Patch `patch` in its box.
fn show(patch: Patch, frame: MutableState<u32>, list_state: LazyListState, grid: Grid) {
    match patch {
        Patch::Still => Still(grid),
        Patch::Charts => Charts(grid),
        Patch::Recolor => Recolor(frame, grid),
        Patch::Cards => CardStack(frame, grid),
        Patch::Reorder => Reorder(frame, grid),
        Patch::Deep => Deep(frame, grid),
        Patch::Scroll => ScrollList(list_state, grid),
        Patch::Paragraph => Paragraph(frame, grid),
        Patch::Pile => Pile(frame, grid),
    }
}

// Tiles that never change.
#[composable]
fn Still(grid: Grid) {
    Box(grid.patch(), BoxSpec::default(), move || {
        Wall(
            grid.tiles.columns,
            grid.tiles.rows,
            grid.tiles.width,
            grid.tiles.height,
        );
    });
}

// Line charts that never change: each draws its line in a draw block that
// reads no state, so no frame records it again.
#[composable]
fn Charts(grid: Grid) {
    let (columns, rows) = data::patchwork_pairs(grid.tiles.columns, grid.tiles.rows);
    Box(grid.patch(), BoxSpec::default(), move || {
        tile_rows(columns, rows, move |index| {
            Chart(
                index,
                data::patchwork_double(grid.tiles.width),
                data::patchwork_double(grid.tiles.height),
            );
        });
    });
}

#[composable]
fn Chart(index: usize, width: f32, height: f32) {
    Canvas(Modifier::empty().size_points(width, height), move |scope| {
        let size = scope.size();
        let step = size.width / (PATCHWORK_CHART_POINTS - 1) as f32;
        let mut line = Path::new();
        for point in 0..PATCHWORK_CHART_POINTS {
            let at = Point {
                x: point as f32 * step,
                y: size.height * (1.0 - data::patchwork_chart_value(index, point)),
            };
            if point == 0 {
                line.move_to(at);
            } else {
                line.line_to(at);
            }
        }
        scope.draw_path(
            &line,
            Brush::solid(rgb(data::patchwork_chart_rgb(index))),
            DrawStyle::Stroke(Stroke::new(data::PATCHWORK_CHART_STROKE)),
        );
    });
}

// Tiles whose fill changes in turn: each tile's draw block reads its own
// fill, derived from the frame, so a frame draws again the fills that
// changed and composes and lays out nothing.
#[composable]
fn Recolor(frame: MutableState<u32>, grid: Grid) {
    Box(grid.patch(), BoxSpec::default(), move || {
        tile_rows(grid.tiles.columns, grid.tiles.rows, move |index| {
            RecolorTile(frame, index, grid);
        });
    });
}

#[composable]
fn RecolorTile(frame: MutableState<u32>, index: usize, grid: Grid) {
    let (width, height) = (grid.tiles.width, grid.tiles.height);
    let turn = grid.tiles.in_turn();
    let fill = derivedStateOf(move || data::patchwork_recolor_rgb(index, frame.get(), turn));
    Column(
        Modifier::empty()
            .size_points(width, height)
            .draw_behind(move |scope| {
                scope.draw_round_rect(
                    Brush::solid(rgb(fill.get())),
                    CornerRadii::uniform(data::WALL_CORNER),
                );
            })
            .padding(data::wall_padding(height)),
        ColumnSpec::default(),
        move || tile_content(index, height),
    );
}

// Cards that move every frame. The stack composes again when a move
// begins, as a card comes in or the last one leaves; within a move each
// card's graphics layer reads the frame for its place, so a frame moves the
// cards and composes, lays out and draws none of them again.
#[composable]
fn CardStack(frame: MutableState<u32>, grid: Grid) {
    let cards = Cards::new(
        grid.tiles.columns,
        grid.tiles.rows,
        grid.tiles.height,
        grid.patch_height,
    );
    let moves = derivedStateOf(move || frame.get() / data::PATCHWORK_CARD_FRAMES);
    let begun = moves.get() * data::PATCHWORK_CARD_FRAMES;
    Box(grid.patch(), BoxSpec::default(), move || {
        for place in 0..cards.places() {
            if let Some(card) = cards.place(place, begun) {
                key(card.id, move || Card(frame, cards, card.id, grid));
            }
        }
    });
}

// Card `id`: its header over its rows of tiles.
#[composable]
fn Card(frame: MutableState<u32>, cards: Cards, id: usize, grid: Grid) {
    let width = data::patchwork_reorder_width(cards.columns, grid.tiles.width);
    Column(
        Modifier::empty()
            .padding_horizontal(WALL_GAP)
            .graphics_layer(move || GraphicsLayer {
                // A card no frame shows waits above the patch.
                translation_y: cards.top(id, frame.get()).unwrap_or(-cards.height),
                ..Default::default()
            }),
        ColumnSpec::new().vertical_arrangement(LinearArrangement::spaced_by(WALL_GAP)),
        move || {
            Box(
                Modifier::empty()
                    .size_points(width, grid.tiles.height)
                    .background(rgb(data::patchwork_card_rgb(id)))
                    .rounded_corners(data::WALL_CORNER)
                    .padding(data::wall_padding(grid.tiles.height)),
                BoxSpec::default(),
                move || {
                    TextWithOptions(
                        data::patchwork_card_title(id),
                        Modifier::empty(),
                        text_style(data::wall_text_size(grid.tiles.height).0, INK, true),
                        ONE_LINE,
                    );
                },
            );
            for row in 0..cards.rows {
                Row(
                    Modifier::empty(),
                    RowSpec::new().horizontal_arrangement(LinearArrangement::spaced_by(WALL_GAP)),
                    move || {
                        for column in 0..cards.columns {
                            let tile = row * cards.columns + column;
                            Tile(
                                data::patchwork_card_tile(id, tile, cards.tiles()),
                                grid.tiles.width,
                                grid.tiles.height,
                            );
                        }
                    },
                );
            }
        },
    );
}

// A wrapping row of items, two of which change places every frame: each
// item keeps its key, so a frame moves those two and composes none again.
#[composable]
fn Reorder(frame: MutableState<u32>, grid: Grid) {
    let count = data::patchwork_reorder_count(grid.tiles.count());
    let turned = frame.get();
    FlowRow(
        grid.patch().padding(WALL_GAP),
        FlowSpec::new()
            .main_axis_spacing(WALL_GAP)
            .cross_axis_spacing(WALL_GAP),
        move || {
            for position in 0..count {
                let item = data::patchwork_reorder_item(position, turned, count);
                let width = data::patchwork_reorder_width(
                    data::patchwork_reorder_units(item),
                    grid.tiles.width,
                );
                key(item, move || {
                    Tile(item, width, grid.tiles.height);
                });
            }
        },
    );
}

// What every nest of the patch shares: the tiles' height and the nests
// across and down, which change in turn.
#[derive(Clone, Copy, PartialEq, Debug)]
struct Nest {
    height: f32,
    turn: InTurn,
}

// Nests of boxes around digits that change in turn.
#[composable]
fn Deep(frame: MutableState<u32>, grid: Grid) {
    let (columns, rows) = data::patchwork_pairs(grid.tiles.columns, grid.tiles.rows);
    let nest = Nest {
        height: grid.tiles.height,
        turn: InTurn { columns, rows },
    };
    Box(grid.patch(), BoxSpec::default(), move || {
        tile_rows(columns, rows, move |index| {
            Box(
                Modifier::empty().size_points(
                    data::patchwork_double(grid.tiles.width),
                    data::patchwork_double(grid.tiles.height),
                ),
                BoxSpec::default(),
                move || NestLevel(frame, index, 0, nest),
            );
        });
    });
}

// Box `level` of nest `index` and everything inside it, for tiles `height`
// points tall.
#[composable]
fn NestLevel(frame: MutableState<u32>, index: usize, level: usize, nest: Nest) {
    let height = nest.height;
    Box(
        Modifier::empty()
            .fill_max_size()
            .background(rgb(data::patchwork_deep_rgb(index, level)))
            .padding(data::wall_padding(height)),
        BoxSpec::default(),
        move || {
            if level + 1 < PATCHWORK_DEPTH {
                NestLevel(frame, index, level + 1, nest);
            } else {
                NestValue(frame, index, nest);
            }
        },
    );
}

// The digits inside a nest: the one composable of the nest that changes,
// when its own digits do.
#[composable]
fn NestValue(frame: MutableState<u32>, index: usize, nest: Nest) {
    let Nest { height, turn } = nest;
    let number = derivedStateOf(move || data::patchwork_deep_number(index, frame.get(), turn));
    TextWithOptions(
        format!("{:03}", number.get()),
        Modifier::empty(),
        text_style(data::wall_text_size(height).0, INK, true),
        ONE_LINE,
    );
}

// A lazy list of rows of tiles, which the frame loop scrolls by one row
// every frame.
#[composable]
fn ScrollList(list_state: LazyListState, grid: Grid) {
    LazyColumn(
        grid.patch(),
        list_state,
        LazyColumnSpec::new()
            .vertical_arrangement(LinearArrangement::spaced_by(WALL_GAP))
            .content_padding_all(WALL_GAP),
        move |scope| {
            scope.items(
                LazyItems::new(data::patchwork_scroll_rows(grid.tiles.columns))
                    .key(|row| row as u64),
                move |row| ScrollRow(row, grid),
            );
        },
    );
}

#[composable]
fn ScrollRow(row: usize, grid: Grid) {
    Row(
        Modifier::empty(),
        RowSpec::new().horizontal_arrangement(LinearArrangement::spaced_by(WALL_GAP)),
        move || {
            for column in 0..grid.tiles.columns {
                Tile(
                    row * grid.tiles.columns + column,
                    grid.tiles.width,
                    grid.tiles.height,
                );
            }
        },
    );
}

// One paragraph, one word of which changes every frame.
#[composable]
fn Paragraph(frame: MutableState<u32>, grid: Grid) {
    Box(
        grid.patch().padding(WALL_GAP),
        BoxSpec::default(),
        move || ParagraphText(frame, grid),
    );
}

#[composable]
fn ParagraphText(frame: MutableState<u32>, grid: Grid) {
    Text(
        data::patchwork_paragraph(
            frame.get(),
            data::patchwork_paragraph_words(grid.tiles.count()),
        ),
        Modifier::empty(),
        text_style(data::wall_text_size(grid.tiles.height).0, INK, false),
    );
}

// Translucent tiles lying over one another. This call reads the frame; a
// tile composes again only when it turns opaque or translucent.
#[composable]
fn Pile(frame: MutableState<u32>, grid: Grid) {
    let (columns, rows) = data::patchwork_pile_grid(grid.tiles.columns, grid.tiles.rows);
    let opaque = data::patchwork_pile_opaque(frame.get(), columns * rows);
    Box(grid.patch(), BoxSpec::default(), move || {
        for index in 0..columns * rows {
            PileTile(
                index,
                index % columns,
                index / columns,
                grid,
                index == opaque,
            );
        }
    });
}

#[composable]
fn PileTile(index: usize, column: usize, row: usize, grid: Grid, opaque: bool) {
    let fill = if opaque {
        rgb(data::PATCHWORK_PILE_OPAQUE_RGB)
    } else {
        let [red, green, blue] = data::wall_fill_rgb(index);
        Color::from_rgba_u8(red, green, blue, data::PATCHWORK_PILE_ALPHA)
    };
    Column(
        Modifier::empty()
            .offset(
                WALL_GAP + column as f32 * (grid.tiles.width + WALL_GAP),
                WALL_GAP + row as f32 * (grid.tiles.height + WALL_GAP),
            )
            .size_points(
                data::patchwork_double(grid.tiles.width),
                data::patchwork_double(grid.tiles.height),
            )
            .background(fill)
            .rounded_corners(data::WALL_CORNER)
            .padding(data::wall_padding(grid.tiles.height)),
        ColumnSpec::default(),
        move || tile_content(index, grid.tiles.height),
    );
}
