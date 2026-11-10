//! The patchwork (`../../../README.md`): nine patches of the still wall's
//! tiles, each of which changes in a way of its own every frame.
//!
//! In Cranpose's idiom each patch reads the frame as late as it can: the
//! recolor patch in its tiles' draw blocks, the deep patch in its innermost
//! texts, the pile patch in one call whose tiles skip but for the two whose
//! fill changes. The churn patch's cells and the reorder patch's row compose
//! again each frame, the row's items under their keys.

use cranpose::{LazyItems, prelude::*};
use cranpose_foundation::lazy::rememberLazyListState;
use cranpose_ui::widgets::{FlowRow, FlowSpec};

use super::{
    text_style,
    wall::{BACKGROUND, Clock, INK, ONE_LINE, Tile, Wall, rgb, tile_content, tile_rows},
};
use crate::data::{
    self, ChurnKind, PATCH_COLUMNS, PATCH_ROWS, PATCHWORK_CHART_POINTS, PATCHWORK_DEPTH, Patch,
    WALL_GAP,
};

/// The patchwork's tier, the frame it freezes on (0 runs on) and the one
/// patch it shows, when a launch asks for one alone.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct PatchworkLoad {
    pub tier: usize,
    pub freeze: u32,
    pub only: Option<Patch>,
}

// One patch's tiles: `columns` by `rows` of them `width` by `height` points
// each, in a patch `patch_width` by `patch_height` points.
#[derive(Clone, Copy, PartialEq, Debug)]
struct Grid {
    columns: usize,
    rows: usize,
    width: f32,
    height: f32,
    patch_width: f32,
    patch_height: f32,
}

impl Grid {
    fn tiles(self) -> usize {
        self.columns * self.rows
    }

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
                    let patch_width = (scope.max_width().0 / PATCH_COLUMNS as f32).floor();
                    let patch_height = (scope.max_height().0 / PATCH_ROWS as f32).floor();
                    let (columns, rows) = data::wall_grid(tier, patch_width, patch_height);
                    Patches(
                        frame,
                        load,
                        Grid {
                            columns,
                            rows,
                            width: tier.tile_width,
                            height: tier.tile_height,
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
    let jump_rows = data::patchwork_jump_rows(grid.columns);
    LaunchedEffect((freeze, grid.columns, grid.rows), async move |scope| {
        let clock = scope.runtime().frame_clock();
        let mut index = 0u32;
        loop {
            clock.next_frame().await;
            index += 1;
            frame.set(index);
            list_state.scroll_to_item(data::patchwork_jump_first(index, jump_rows, grid.rows), 0.0);
            if freeze > 0 && index >= freeze {
                log::info!("PERF frozen frame={index}");
                break;
            }
        }
    });
    Column(Modifier::empty(), ColumnSpec::default(), move || {
        for row in 0..PATCH_ROWS {
            Row(Modifier::empty(), RowSpec::default(), move || {
                for patch in &Patch::ALL[row * PATCH_COLUMNS..(row + 1) * PATCH_COLUMNS] {
                    if load.only.is_some_and(|only| only != *patch) {
                        Box(grid.patch(), BoxSpec::default(), || {});
                        continue;
                    }
                    show(*patch, frame, list_state, grid);
                }
            });
        }
    });
}

// Patch `patch` in its box.
fn show(patch: Patch, frame: MutableState<u32>, list_state: LazyListState, grid: Grid) {
    match patch {
        Patch::Still => Still(grid),
        Patch::Charts => Charts(grid),
        Patch::Recolor => Recolor(frame, grid),
        Patch::Churn => Churn(frame, grid),
        Patch::Reorder => Reorder(frame, grid),
        Patch::Deep => Deep(frame, grid),
        Patch::Jump => Jump(list_state, grid),
        Patch::Paragraph => Paragraph(frame, grid),
        Patch::Pile => Pile(frame, grid),
    }
}

// Tiles that never change.
#[composable]
fn Still(grid: Grid) {
    Box(grid.patch(), BoxSpec::default(), move || {
        Wall(grid.columns, grid.rows, grid.width, grid.height);
    });
}

// Line charts that never change: each draws its line in a draw block that
// reads no state, so no frame records it again.
#[composable]
fn Charts(grid: Grid) {
    let (columns, rows) = data::patchwork_pairs(grid.columns, grid.rows);
    Box(grid.patch(), BoxSpec::default(), move || {
        tile_rows(columns, rows, move |index| {
            Chart(
                index,
                data::patchwork_double(grid.width),
                data::patchwork_double(grid.height),
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

// Tiles whose fill follows the frame: each tile's draw block reads it, so a
// frame draws the fills again and composes and lays out nothing.
#[composable]
fn Recolor(frame: MutableState<u32>, grid: Grid) {
    Box(grid.patch(), BoxSpec::default(), move || {
        tile_rows(grid.columns, grid.rows, move |index| {
            RecolorTile(frame, index, grid.width, grid.height);
        });
    });
}

#[composable]
fn RecolorTile(frame: MutableState<u32>, index: usize, width: f32, height: f32) {
    Column(
        Modifier::empty()
            .size_points(width, height)
            .draw_behind(move |scope| {
                scope.draw_round_rect(
                    Brush::solid(rgb(data::patchwork_recolor_rgb(index, frame.get()))),
                    CornerRadii::uniform(data::WALL_CORNER),
                );
            })
            .padding(data::wall_padding(height)),
        ColumnSpec::default(),
        move || tile_content(index, height),
    );
}

// Cells that show another kind of content every frame.
#[composable]
fn Churn(frame: MutableState<u32>, grid: Grid) {
    Box(grid.patch(), BoxSpec::default(), move || {
        tile_rows(grid.columns, grid.rows, move |index| {
            ChurnCell(frame, index, grid.width, grid.height);
        });
    });
}

#[composable]
fn ChurnCell(frame: MutableState<u32>, index: usize, width: f32, height: f32) {
    let cell = Modifier::empty()
        .size_points(width, height)
        .rounded_corners(data::WALL_CORNER);
    match data::patchwork_churn(index, frame.get()) {
        ChurnKind::Tile => {
            Tile(index, width, height);
        }
        ChurnKind::Blank => {
            Box(
                cell.background(rgb(data::wall_dot_rgb(index))),
                BoxSpec::default(),
                || {},
            );
        }
        ChurnKind::Label => {
            Box(
                cell.background(rgb(data::wall_fill_rgb(index)))
                    .padding(data::wall_padding(height)),
                BoxSpec::default(),
                move || {
                    TextWithOptions(
                        data::wall_label(index),
                        Modifier::empty(),
                        text_style(data::wall_text_size(height).0, INK, true),
                        ONE_LINE,
                    );
                },
            );
        }
    }
}

// A wrapping row of items whose order turns by one item every frame: each
// item keeps its key, so a frame moves the items and composes none again.
#[composable]
fn Reorder(frame: MutableState<u32>, grid: Grid) {
    let count = data::patchwork_reorder_count(grid.tiles());
    let turned = frame.get();
    FlowRow(
        grid.patch().padding(WALL_GAP),
        FlowSpec::new()
            .main_axis_spacing(WALL_GAP)
            .cross_axis_spacing(WALL_GAP),
        move || {
            for position in 0..count {
                let item = data::patchwork_reorder_item(position, turned, count);
                let width =
                    data::patchwork_reorder_width(data::patchwork_reorder_units(item), grid.width);
                key(item, move || {
                    Tile(item, width, grid.height);
                });
            }
        },
    );
}

// Nests of boxes around digits that change every frame.
#[composable]
fn Deep(frame: MutableState<u32>, grid: Grid) {
    let (columns, rows) = data::patchwork_pairs(grid.columns, grid.rows);
    Box(grid.patch(), BoxSpec::default(), move || {
        tile_rows(columns, rows, move |index| {
            Box(
                Modifier::empty().size_points(
                    data::patchwork_double(grid.width),
                    data::patchwork_double(grid.height),
                ),
                BoxSpec::default(),
                move || NestLevel(frame, index, 0, grid.height),
            );
        });
    });
}

// Box `level` of nest `index` and everything inside it, for tiles `height`
// points tall.
#[composable]
fn NestLevel(frame: MutableState<u32>, index: usize, level: usize, height: f32) {
    Box(
        Modifier::empty()
            .fill_max_size()
            .background(rgb(data::patchwork_deep_rgb(index, level)))
            .padding(data::wall_padding(height)),
        BoxSpec::default(),
        move || {
            if level + 1 < PATCHWORK_DEPTH {
                NestLevel(frame, index, level + 1, height);
            } else {
                NestValue(frame, index, height);
            }
        },
    );
}

// The digits inside a nest: the one composable of the nest that reads the
// frame.
#[composable]
fn NestValue(frame: MutableState<u32>, index: usize, height: f32) {
    TextWithOptions(
        data::patchwork_deep_value(index, frame.get()),
        Modifier::empty(),
        text_style(data::wall_text_size(height).0, INK, true),
        ONE_LINE,
    );
}

// A lazy list of rows of tiles, which the frame loop scrolls to other rows
// every frame.
#[composable]
fn Jump(list_state: LazyListState, grid: Grid) {
    LazyColumn(
        grid.patch(),
        list_state,
        LazyColumnSpec::new()
            .vertical_arrangement(LinearArrangement::spaced_by(WALL_GAP))
            .content_padding_all(WALL_GAP),
        move |scope| {
            scope.items(
                LazyItems::new(data::patchwork_jump_rows(grid.columns)).key(|row| row as u64),
                move |row| JumpRow(row, grid),
            );
        },
    );
}

#[composable]
fn JumpRow(row: usize, grid: Grid) {
    Row(
        Modifier::empty(),
        RowSpec::new().horizontal_arrangement(LinearArrangement::spaced_by(WALL_GAP)),
        move || {
            for column in 0..grid.columns {
                Tile(row * grid.columns + column, grid.width, grid.height);
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
        data::patchwork_paragraph(frame.get(), data::patchwork_paragraph_words(grid.tiles())),
        Modifier::empty(),
        text_style(data::wall_text_size(grid.height).0, INK, false),
    );
}

// Translucent tiles lying over one another. This call reads the frame; a
// tile composes again only when it turns opaque or translucent.
#[composable]
fn Pile(frame: MutableState<u32>, grid: Grid) {
    let (columns, rows) = data::patchwork_pile_grid(grid.columns, grid.rows);
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
                WALL_GAP + column as f32 * (grid.width + WALL_GAP),
                WALL_GAP + row as f32 * (grid.height + WALL_GAP),
            )
            .size_points(
                data::patchwork_double(grid.width),
                data::patchwork_double(grid.height),
            )
            .background(fill)
            .rounded_corners(data::WALL_CORNER)
            .padding(data::wall_padding(grid.height)),
        ColumnSpec::default(),
        move || tile_content(index, grid.height),
    );
}
