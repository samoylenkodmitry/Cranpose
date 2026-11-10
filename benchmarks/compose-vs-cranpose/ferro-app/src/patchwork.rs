//! The patchwork in FerroUI (`../README.md`): nine patches of the still
//! wall's tiles, a chat (`chat.rs`) beside them.
//!
//! In FerroUI's idiom every control is made once and what a frame changes is
//! a signal that only its readers follow. A driver effect per patch reads the
//! frame and sets the signals of the rows, tiles or cards that changed, so
//! the tiles that did not change run no code. Recolor and deep keep one
//! signal per row; pile keeps one flag per tile and sets two of them; the
//! cards follow the frame in a render transform, which lays out nothing. The
//! reorder patch moves two children of its panel in place. The scroll patch
//! is a `ListBox`, whose virtualizing panel realizes the rows in view and
//! whose template recycles the components of rows that left. The paragraph
//! is one text, set whole: the text control shapes its whole content again
//! for a changed word.

use std::{cell::Cell, rc::Rc};

use ferroui_base::{
    Point, Ref, Vector,
    layout::Orientation,
    media::{Color, ITransform, Points, TextWrapping, TranslateTransform},
};
use ferroui_controls::{
    Border, ItemsSource, ListBox, ScrollViewer, StackPanel, WrapPanel,
    primitives::{ScrollBarVisibility, TemplatedControl},
};
use ferroui_view::{prelude::*, template};
use perf_data::*;

use super::{
    chat, txt,
    wall::{cells, screen, tile, tile_content, tile_frame},
};

const INK: &str = "#FFFFFF";

fn color(rgb: [u8; 3]) -> Color {
    Color::from_rgb(rgb[0], rgb[1], rgb[2])
}

/// One patch's tiles and its size in points.
#[derive(Clone, Copy)]
struct Area {
    tiles: PatchTiles,
    width: f32,
    height: f32,
}

impl Area {
    /// The patch's own box: its size, which cuts what lies past it.
    fn frame(self) -> ferroui_view::El<Border> {
        border()
            .width(f64::from(self.width))
            .height(f64::from(self.height))
            .clip_to_bounds(true)
    }
}

pub(crate) fn patchwork(frame: Signal<u32>) -> Element {
    let tier = wall_tier(perf_data::launch().tier);
    let (width, height) = DESKTOP_WINDOW;
    let patch_width = ((width as f32 - CHAT_WIDTH) / PATCH_COLUMNS as f32).floor();
    let patch_height = ((height as f32 - 56.0 - WALL_CLOCK_HEIGHT) / PATCH_ROWS as f32).floor();
    let area = Area {
        tiles: PatchTiles::new(tier, patch_width, patch_height),
        width: patch_width,
        height: patch_height,
    };
    let scroll = scroll_list(area, frame);
    let patches = grid()
        .column_definitions(&format!("{patch_width},{patch_width},{patch_width}"))
        .row_definitions(&format!("{patch_height},{patch_height},{patch_height}"))
        .children(
            Patch::ALL
                .iter()
                .enumerate()
                .map(|(place, kind)| {
                    let content = match kind {
                        Patch::Still => still(area),
                        Patch::Charts => charts(area),
                        Patch::Recolor => recolor(area, frame),
                        Patch::Cards => card_stack(area, frame),
                        Patch::Reorder => reorder(area, frame),
                        Patch::Deep => deep(area, frame),
                        Patch::Scroll => scroll.clone(),
                        Patch::Paragraph => paragraph(area, frame),
                        Patch::Pile => pile(area, frame),
                    };
                    area.frame()
                        .grid_row((place / PATCH_COLUMNS) as i32)
                        .grid_column((place % PATCH_COLUMNS) as i32)
                        .child(content)
                })
                .collect::<Vec<_>>(),
        );
    screen(
        "Patchwork",
        frame,
        grid()
            .column_definitions(&format!("*,{CHAT_WIDTH}"))
            .child(patches)
            .child(chat::chat(frame).grid_column(1)),
    )
}

/// Tiles that never change.
fn still(area: Area) -> Element {
    let PatchTiles {
        columns,
        rows,
        width,
        height,
    } = area.tiles;
    cells(columns, rows, move |index| tile(index, width, height)).into()
}

/// Line charts that never change: each line is made once.
fn charts(area: Area) -> Element {
    let tiles = area.tiles;
    let (columns, rows) = patchwork_pairs(tiles.columns, tiles.rows);
    let (width, height) = (
        patchwork_double(tiles.width),
        patchwork_double(tiles.height),
    );
    cells(columns, rows, move |index| chart(index, width, height)).into()
}

fn chart(index: usize, width: f32, height: f32) -> Element {
    let step = width / (PATCHWORK_CHART_POINTS - 1) as f32;
    let line = Points::from_items((0..PATCHWORK_CHART_POINTS).map(|point| {
        Point::new(
            f64::from(point as f32 * step),
            f64::from(height * (1.0 - patchwork_chart_value(index, point))),
        )
    }));
    canvas()
        .width(f64::from(width))
        .height(f64::from(height))
        .child(
            polyline()
                .points(line)
                .stroke(color(patchwork_chart_rgb(index)))
                .stroke_thickness(f64::from(PATCHWORK_CHART_STROKE)),
        )
        .into()
}

/// One signal per row of `turn`: the frame its row last changed in. A frame
/// sets only the signal of the row whose step changed.
fn row_marks(frame: Signal<u32>, turn: InTurn) -> Rc<[Signal<u32>]> {
    let marks: Rc<[Signal<u32>]> = (0..turn.rows).map(|_| signal(0u32)).collect();
    let driven = marks.clone();
    effect(move || {
        let frame = frame.get();
        for (row, mark) in driven.iter().enumerate() {
            let first = row * turn.columns;
            if turn.step(first, mark.get_untracked()) != turn.step(first, frame) {
                mark.set(frame);
            }
        }
    });
    marks
}

/// Tiles whose fill follows the mark of their row.
fn recolor(area: Area, frame: Signal<u32>) -> Element {
    let PatchTiles {
        columns,
        rows,
        width,
        height,
    } = area.tiles;
    let turn = area.tiles.in_turn();
    let marks = row_marks(frame, turn);
    cells(columns, rows, move |index| {
        let mark = marks[index / turn.columns];
        tile_frame(width, height)
            .background(move || color(patchwork_recolor_rgb(index, mark.get(), turn)))
            .child(tile_content(index, height))
            .into()
    })
    .into()
}

/// Cards that move every frame. The set of cards changes when a move
/// begins; within a move each card follows the frame in a render transform.
fn card_stack(area: Area, frame: Signal<u32>) -> Element {
    let tiles = area.tiles;
    let cards = Cards::new(tiles.columns, tiles.rows, tiles.height, area.height);
    let moves = signal(0u32);
    effect(move || moves.set_if_changed(frame.get() / PATCHWORK_CARD_FRAMES));
    canvas()
        .child(for_each(
            move || {
                let begun = moves.get() * PATCHWORK_CARD_FRAMES;
                (0..cards.places())
                    .filter_map(|place| cards.place(place, begun))
                    .map(|card| card.id)
                    .collect::<Vec<_>>()
            },
            |id| *id,
            move |id| card(cards, id, tiles, frame),
        ))
        .into()
}

/// Card `id`: its header over its rows of tiles.
fn card(cards: Cards, id: usize, tiles: PatchTiles, frame: Signal<u32>) -> Element {
    let (label_size, _) = wall_text_size(tiles.height);
    let header = border()
        .width(f64::from(patchwork_reorder_width(
            cards.columns,
            tiles.width,
        )))
        .height(f64::from(tiles.height))
        .background(color(patchwork_card_rgb(id)))
        .corner_radius(f64::from(WALL_CORNER))
        .padding(f64::from(wall_padding(tiles.height)))
        .child(txt(
            patchwork_card_title(id),
            f64::from(label_size),
            INK,
            true,
        ));
    let rows = (0..cards.rows)
        .map(|row| {
            stack_panel()
                .orientation(Orientation::Horizontal)
                .spacing(f64::from(WALL_GAP))
                .children(
                    (0..cards.columns)
                        .map(|column| {
                            let tile_index = patchwork_card_tile(
                                id,
                                row * cards.columns + column,
                                cards.tiles(),
                            );
                            tile(tile_index, tiles.width, tiles.height)
                        })
                        .collect::<Vec<_>>(),
                )
        })
        .collect::<Vec<_>>();
    stack_panel()
        .spacing(f64::from(WALL_GAP))
        .canvas_left(f64::from(WALL_GAP))
        .with(move |control: &Ref<StackPanel>| {
            let translate = TranslateTransform::new();
            let transform: Rc<dyn ITransform> = translate.clone().into();
            control.set_render_transform(Some(transform));
            effect(move || {
                // A card no frame shows waits above the patch.
                translate.set_y(f64::from(
                    cards.top(id, frame.get()).unwrap_or(-cards.height),
                ));
            });
        })
        .child(header)
        .children(rows)
        .into()
}

/// A wrapping row of items. Two neighbours change places every frame: the
/// effect moves those two children of the panel and moves the pair of the
/// frame before back.
fn reorder(area: Area, frame: Signal<u32>) -> Element {
    let tiles = area.tiles;
    let count = patchwork_reorder_count(tiles.count());
    border()
        .padding(f64::from(WALL_GAP))
        .child(
            wrap_panel()
                .item_spacing(f64::from(WALL_GAP))
                .line_spacing(f64::from(WALL_GAP))
                .children(
                    (0..count)
                        .map(|item| {
                            let units = patchwork_reorder_units(item);
                            tile(
                                item,
                                patchwork_reorder_width(units, tiles.width),
                                tiles.height,
                            )
                        })
                        .collect::<Vec<_>>(),
                )
                .with(move |panel: &Ref<WrapPanel>| {
                    if count < 2 {
                        return;
                    }
                    let children = panel.children();
                    let swapped = Cell::new(None::<usize>);
                    effect(move || {
                        let swap = patchwork_reorder_swap(frame.get(), count);
                        if let Some(before) = swapped.replace(Some(swap)) {
                            children.move_item(before, before + 1);
                        }
                        children.move_item(swap, swap + 1);
                    });
                }),
        )
        .into()
}

/// Nests of boxes around digits that follow the mark of their row.
fn deep(area: Area, frame: Signal<u32>) -> Element {
    let tiles = area.tiles;
    let (columns, rows) = patchwork_pairs(tiles.columns, tiles.rows);
    let turn = InTurn { columns, rows };
    let marks = row_marks(frame, turn);
    cells(columns, rows, move |index| {
        nest(index, tiles, marks[index / columns], turn)
    })
    .into()
}

fn nest(index: usize, tiles: PatchTiles, mark: Signal<u32>, turn: InTurn) -> Element {
    let (label_size, _) = wall_text_size(tiles.height);
    let padding = f64::from(wall_padding(tiles.height));
    let mut inner: Element = txt("", f64::from(label_size), INK, true)
        .text(move || patchwork_deep_value(index, mark.get(), turn))
        .into();
    for level in (0..PATCHWORK_DEPTH).rev() {
        inner = border()
            .background(color(patchwork_deep_rgb(index, level)))
            .padding(padding)
            .child(inner)
            .into();
    }
    border()
        .width(f64::from(patchwork_double(tiles.width)))
        .height(f64::from(patchwork_double(tiles.height)))
        .child(inner)
        .into()
}

/// A list of rows of tiles in a `ListBox`, which realizes the rows in view
/// and recycles the components of the rows that left. The frame sets the
/// scroll offset of its viewer to a row.
fn scroll_list(area: Area, frame: Signal<u32>) -> Element {
    let tiles = area.tiles;
    let total = patchwork_scroll_rows(tiles.columns);
    let pitch = f64::from(tiles.height + WALL_GAP);
    list_box()
        .background("#00000000")
        .border_thickness(0.0)
        .padding(f64::from(WALL_GAP))
        .styles([style("ListBoxItem").set(
            TemplatedControl::padding_property(),
            (0.0, 0.0, 0.0, f64::from(WALL_GAP)),
        )])
        .items_source(ItemsSource::from_values(0..total))
        .item_template(template(move |row: usize| {
            stack_panel()
                .orientation(Orientation::Horizontal)
                .spacing(f64::from(WALL_GAP))
                .children(
                    (0..tiles.columns)
                        .map(|column| tile(row * tiles.columns + column, tiles.width, tiles.height))
                        .collect::<Vec<_>>(),
                )
        }))
        .with(move |list: &Ref<ListBox>| {
            ScrollViewer::set_horizontal_scroll_bar_visibility_on(
                list,
                ScrollBarVisibility::Disabled,
            );
            ScrollViewer::set_vertical_scroll_bar_visibility_on(list, ScrollBarVisibility::Hidden);
            let list = list.clone();
            effect(move || {
                let first = patchwork_scroll_first(frame.get(), total, tiles.rows);
                if let Some(viewer) = list
                    .scroll()
                    .and_then(|scroll| scroll.cast::<ScrollViewer>())
                {
                    viewer.set_offset(Vector::new(0.0, first as f64 * pitch));
                }
            });
        })
        .into()
}

/// One paragraph, set whole when a word changes.
fn paragraph(area: Area, frame: Signal<u32>) -> Element {
    let words = patchwork_paragraph_words(area.tiles.count());
    let (size, _) = wall_text_size(area.tiles.height);
    border()
        .padding(f64::from(WALL_GAP))
        .child(
            txt("", f64::from(size), INK, false)
                .text_wrapping(TextWrapping::Wrap)
                .text(move || patchwork_paragraph(frame.get(), words)),
        )
        .into()
}

/// Translucent tiles lying over one another. Each has a flag for being the
/// opaque one: a frame clears one flag and sets one.
fn pile(area: Area, frame: Signal<u32>) -> Element {
    let tiles = area.tiles;
    let (columns, rows) = patchwork_pile_grid(tiles.columns, tiles.rows);
    let count = columns * rows;
    let opaque: Rc<[Signal<bool>]> = (0..count).map(|_| signal(false)).collect();
    let driven = opaque.clone();
    let last = Cell::new(None::<usize>);
    effect(move || {
        let now = patchwork_pile_opaque(frame.get(), count);
        if let Some(before) = last.replace(Some(now)) {
            driven[before].set_if_changed(false);
        }
        driven[now].set_if_changed(true);
    });
    canvas()
        .children(
            (0..count)
                .map(|index| {
                    let [red, green, blue] = wall_fill_rgb(index);
                    let flag = opaque[index];
                    tile_frame(
                        patchwork_double(tiles.width),
                        patchwork_double(tiles.height),
                    )
                    .canvas_left(f64::from(
                        WALL_GAP + (index % columns) as f32 * (tiles.width + WALL_GAP),
                    ))
                    .canvas_top(f64::from(
                        WALL_GAP + (index / columns) as f32 * (tiles.height + WALL_GAP),
                    ))
                    .background(move || {
                        if flag.get() {
                            color(PATCHWORK_PILE_OPAQUE_RGB)
                        } else {
                            Color::from_argb(PATCHWORK_PILE_ALPHA, red, green, blue)
                        }
                    })
                    .child(tile_content(index, tiles.height))
                })
                .collect::<Vec<_>>(),
        )
        .into()
}
