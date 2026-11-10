//! The patchwork in Floem (`../README.md`): nine patches of the still wall's
//! tiles beside a chat. Two patches never change; in the others a frame
//! changes a small part.
//!
//! Every patch is built once. A patch that changes a row of its tiles, the
//! places of its cards, two items of a row, a row of its nests or a word of
//! its paragraph reads the frame in the view that changes: a style closure
//! moves a card by a transform, a keyed `dyn_stack` reorders the items and
//! drops and makes cards, a memo per row of nests tells the nests of the row
//! that changed, and a `virtual_stack` in a `scroll` makes the rows the scroll
//! shows. The tiles of the patches that recolor or pile them are painted from
//! the frame they are painted in.

use std::{cell::RefCell, collections::HashMap, rc::Rc};

use floem::{
    IntoView, View, ViewId,
    context::{PaintCx, UpdateCx},
    kurbo::{Point, Rect},
    peniko::{Color, Image},
    prelude::*,
    reactive::{create_effect, create_memo},
    style::Style,
    taffy::{AlignContent, FlexWrap},
    text::TextLayout,
    views::{VirtualDirection, VirtualItemSize, VirtualVector, clip, dyn_stack},
};
use floem_renderer::{Img, Renderer as _};
use perf_data::*;

use crate::{
    chat::chat,
    frame::Frame,
    layer::layer,
    raster::Raster,
    rgb,
    surface::Surface,
    text::{Face, Type},
    tiles::{TileGrid, TileKit},
    top_bar,
    wall::clock_line,
};

const BACKGROUND: Color = rgb(WALL_BACKGROUND_RGB);
const INK: Color = rgb(WALL_INK_RGB);

/// One patch's tiles, and what they are drawn on.
#[derive(Clone)]
struct Grid {
    tiles: PatchTiles,
    width: f32,
    height: f32,
    kit: Rc<TileKit>,
    frame: Frame,
}

impl Grid {
    /// The size of `columns` by `rows` tiles of width `width`, `WALL_GAP`
    /// apart and from the edges.
    fn field_size(&self, columns: usize, rows: usize, width: f32) -> (f32, f32) {
        (
            columns as f32 * (width + WALL_GAP) + WALL_GAP,
            rows as f32 * (self.tiles.height + WALL_GAP) + WALL_GAP,
        )
    }

    fn label_type(&self, face: Face, line: f32) -> Type {
        Type::new(wall_text_size(self.tiles.height).0, line, face, INK)
    }
}

pub fn screen(launch: Launch, frame: Frame) -> impl IntoView {
    let tier = wall_tier(launch.tier);
    let (window_width, window_height) = DESKTOP_WINDOW;
    let width = ((window_width as f32 - CHAT_WIDTH) / PATCH_COLUMNS as f32).floor();
    let height = ((window_height as f32 - 56.0 - WALL_CLOCK_HEIGHT) / PATCH_ROWS as f32).floor();
    let tiles = PatchTiles::new(tier, width, height);
    let grid = Grid {
        tiles,
        width,
        height,
        kit: TileKit::new(tiles.height),
        frame,
    };
    let patches = v_stack_from_iter((0..PATCH_ROWS).map(|row| {
        h_stack_from_iter(
            Patch::ALL[row * PATCH_COLUMNS..(row + 1) * PATCH_COLUMNS]
                .iter()
                .map(|patch| {
                    let content = match patch {
                        Patch::Still => still(&grid).into_any(),
                        Patch::Charts => charts(&grid).into_any(),
                        Patch::Recolor => recolor(&grid).into_any(),
                        Patch::Cards => cards(&grid).into_any(),
                        Patch::Reorder => reorder(&grid).into_any(),
                        Patch::Deep => deep(&grid).into_any(),
                        Patch::Scroll => scroll_patch(&grid).into_any(),
                        Patch::Paragraph => paragraph(&grid).into_any(),
                        Patch::Pile => pile(&grid).into_any(),
                    };
                    // A patch cuts what lies past it. Vello gets a clip layer for
                    // each view painted under a clip, so only the patches that
                    // hold a few views clip. The patches of many views stay in
                    // their bounds, or lie over the patch below, which covers
                    // the rest.
                    let cuts = matches!(patch, Patch::Cards | Patch::Scroll);
                    let patch_style = move |s: Style| {
                        s.size(width, height)
                            .flex_shrink(0.0_f32)
                            .background(BACKGROUND)
                    };
                    if cuts {
                        clip(content).style(patch_style).into_any()
                    } else {
                        container(content).style(patch_style).into_any()
                    }
                }),
        )
    }));
    v_stack((
        top_bar("Patchwork"),
        clock_line(frame),
        h_stack((
            patches,
            chat(frame, window_height as f32 - 56.0 - WALL_CLOCK_HEIGHT),
        ))
        .style(|s| s.flex_grow(1.0_f32).width_full()),
    ))
    .style(|s| s.size_full().background(BACKGROUND))
}

// Tiles that never change.
fn still(grid: &Grid) -> impl IntoView {
    let PatchTiles {
        columns,
        rows,
        width,
        ..
    } = grid.tiles;
    let (field_width, field_height) = grid.field_size(columns, rows, width);
    let tiles = TileGrid::new(
        &grid.kit,
        (columns, rows),
        (width, WALL_GAP),
        |index| index,
        |index| rgb(wall_fill_rgb(index)),
    );
    layer((field_width, field_height), BACKGROUND, move |raster| {
        tiles.draw(raster);
    })
    .style(move |s| s.size(field_width, field_height).flex_shrink(0.0_f32))
}

// The line charts, which never change: drawn once into a bitmap.
fn charts(grid: &Grid) -> impl IntoView {
    let PatchTiles {
        columns,
        rows,
        width,
        height,
    } = grid.tiles;
    let (across, down) = patchwork_pairs(columns, rows);
    let (chart_width, chart_height) = (patchwork_double(width), patchwork_double(height));
    let step = chart_width / (PATCHWORK_CHART_POINTS - 1) as f32;
    let size = (grid.width, grid.height);
    layer(size, BACKGROUND, move |raster| {
        for index in 0..across * down {
            let left = WALL_GAP + (index % across) as f32 * (chart_width + WALL_GAP);
            let top = WALL_GAP + (index / across) as f32 * (chart_height + WALL_GAP);
            raster.polyline(
                (0..PATCHWORK_CHART_POINTS).map(|point| {
                    Point::new(
                        f64::from(left + point as f32 * step),
                        f64::from(top + chart_height * (1.0 - patchwork_chart_value(index, point))),
                    )
                }),
                PATCHWORK_CHART_STROKE,
                rgb(patchwork_chart_rgb(index)),
            );
        }
    })
    .style(move |s| s.size(size.0, size.1).flex_shrink(0.0_f32))
}

// Tiles whose fill changes in turn, one row a frame.
fn recolor(grid: &Grid) -> impl IntoView {
    let PatchTiles {
        columns,
        rows,
        width,
        ..
    } = grid.tiles;
    let (turn, frame) = (grid.tiles.in_turn(), grid.frame);
    let (field_width, field_height) = grid.field_size(columns, rows, width);
    TileGrid::new(
        &grid.kit,
        (columns, rows),
        (width, WALL_GAP),
        |index| index,
        move |index| rgb(patchwork_recolor_rgb(index, frame.now(), turn)),
    )
    .field()
    .style(move |s| s.size(field_width, field_height).flex_shrink(0.0_f32))
}

// Cards that move every frame. A card is made when it comes in and dropped
// when it leaves. Its tiles never change, so it is a bitmap, and its style
// reads the frame for its place, which moves it by a transform and lays out
// nothing again.
fn cards(grid: &Grid) -> impl IntoView {
    let PatchTiles {
        columns,
        rows,
        width,
        height,
    } = grid.tiles;
    let place = Cards::new(columns, rows, height, grid.height);
    let (patch_width, patch_height) = (grid.width, grid.height);
    let (frame, kit) = (grid.frame, grid.kit.clone());
    let title_type = grid.label_type(Face::Bold, 1.4);
    let header = patchwork_reorder_width(place.columns, width);
    let pad = wall_padding(height);
    // Which cards a move shows stays the same for all of its frames.
    let moves = create_memo(move |_| frame.get() / PATCHWORK_CARD_FRAMES);
    dyn_stack(
        move || {
            let first = moves.get() * PATCHWORK_CARD_FRAMES;
            (0..place.places())
                .filter_map(|at| place.place(at, first))
                .map(|card| card.id)
                .collect::<Vec<_>>()
        },
        |id| *id,
        move |id| {
            let title = title_type.shape(&patchwork_card_title(id), None);
            let tiles = TileGrid::new(
                &kit,
                (place.columns, place.rows),
                (width, 0.0),
                move |tile| patchwork_card_tile(id, tile, place.tiles()),
                |index| rgb(wall_fill_rgb(index)),
            );
            layer((header, place.height), Color::TRANSPARENT, move |raster| {
                raster.fill_rounded(
                    (0.0, 0.0, f64::from(header), f64::from(height)),
                    f64::from(WALL_CORNER),
                    rgb(patchwork_card_rgb(id)),
                );
                raster.text(&title, Point::new(f64::from(pad), f64::from(pad)));
                raster.set_origin(0.0, height + WALL_GAP);
                tiles.draw(raster);
            })
            .style(move |s| {
                s.absolute()
                    .inset_left(WALL_GAP)
                    .inset_top(0.0)
                    .size(header, place.height)
                    .translate_y(place.top(id, frame.get()).unwrap_or(-place.height))
            })
        },
    )
    .style(move |s| s.size(patch_width, patch_height))
}

// A wrapping row of items in the frame's order: each frame two neighbours
// change places, which the keyed stack does by moving their views. An item
// never changes, so it is a bitmap.
fn reorder(grid: &Grid) -> impl IntoView {
    let count = patchwork_reorder_count(grid.tiles.count());
    let (frame, kit) = (grid.frame, grid.kit.clone());
    let (tile_width, tile_height) = (grid.tiles.width, grid.tiles.height);
    dyn_stack(
        move || {
            (0..count)
                .map(|position| patchwork_reorder_item(position, frame.get(), count))
                .collect::<Vec<_>>()
        },
        |item| *item,
        move |item| {
            let width = patchwork_reorder_width(patchwork_reorder_units(item), tile_width);
            let tile = TileGrid::new(
                &kit,
                (1, 1),
                (width, 0.0),
                move |_| item,
                |index| rgb(wall_fill_rgb(index)),
            );
            layer((width, tile_height), Color::TRANSPARENT, move |raster| {
                tile.draw(raster)
            })
            .style(move |s| s.size(width, tile_height).flex_shrink(0.0_f32))
        },
    )
    .style(move |s| {
        s.flex_wrap(FlexWrap::Wrap)
            .align_content(Some(AlignContent::FlexStart))
            .gap(WALL_GAP)
            .padding(WALL_GAP)
            .size_full()
    })
}

// A nest of boxes around digits, which change when the nest's row comes in
// turn: the nest shapes its digits again and paints, and nothing else does.
// Its boxes never change, so they are a bitmap that the nests of one color
// share.
struct Nest {
    id: ViewId,
    size: (f32, f32),
    pad: f32,
    index: usize,
    digits: TextLayout,
    digit_type: Type,
    boxes: Rc<RefCell<HashMap<usize, Image>>>,
}

fn deep(grid: &Grid) -> impl IntoView {
    let (columns, rows) = patchwork_pairs(grid.tiles.columns, grid.tiles.rows);
    let turn = InTurn { columns, rows };
    let frame = grid.frame;
    let size = (
        patchwork_double(grid.tiles.width),
        patchwork_double(grid.tiles.height),
    );
    let pad = wall_padding(grid.tiles.height);
    let digit_type = grid.label_type(Face::Bold, 1.4);
    let boxes = Rc::new(RefCell::new(HashMap::new()));
    v_stack_from_iter((0..rows).map(|row| {
        let first = row * columns;
        let boxes = boxes.clone();
        // The step of a row's nests moves one row of the patch at a time.
        let step = create_memo(move |_| turn.step(first, frame.get()));
        h_stack_from_iter((first..first + columns).map(|index| {
            let id = ViewId::new();
            create_effect(move |_| {
                step.get();
                id.update_state(patchwork_deep_value(index, frame.now(), turn));
            });
            Nest {
                id,
                size,
                pad,
                index,
                digits: digit_type.shape(&patchwork_deep_value(index, 0, turn), None),
                digit_type,
                boxes: boxes.clone(),
            }
            .style(move |s| s.size(size.0, size.1).flex_shrink(0.0_f32))
        }))
        .style(|s| s.gap(WALL_GAP))
    }))
    .style(|s| s.gap(WALL_GAP).padding(WALL_GAP))
}

impl View for Nest {
    fn id(&self) -> ViewId {
        self.id
    }

    fn update(&mut self, _cx: &mut UpdateCx, state: Box<dyn std::any::Any>) {
        if let Ok(value) = state.downcast::<String>() {
            self.digits = self.digit_type.shape(&value, None);
            self.id.request_paint();
        }
    }

    fn paint(&mut self, cx: &mut PaintCx) {
        let (width, height) = self.size;
        let scale = cx.scale() as f32;
        let (index, pad) = (self.index, self.pad);
        let image = self
            .boxes
            .borrow_mut()
            .entry(index % PALETTE_RGB.len())
            .or_insert_with(|| {
                let mut raster = Raster::new(self.size, scale, Color::TRANSPARENT);
                for level in 0..PATCHWORK_DEPTH {
                    let inset = f64::from(pad) * level as f64;
                    raster.fill_rounded(
                        (
                            inset,
                            inset,
                            f64::from(width) - inset,
                            f64::from(height) - inset,
                        ),
                        0.0,
                        rgb(patchwork_deep_rgb(index, level)),
                    );
                }
                raster.into_image()
            })
            .clone();
        cx.draw_img(
            Img {
                img: image,
                hash: &[],
            },
            Rect::new(0.0, 0.0, f64::from(width), f64::from(height)),
        );
        let inset = f64::from(pad) * PATCHWORK_DEPTH as f64;
        cx.draw_text(&self.digits, Point::new(inset, inset));
    }
}

// The rows of the scroll patch's list.
struct Rows(usize);

impl VirtualVector<usize> for Rows {
    fn total_len(&self) -> usize {
        self.0
    }

    fn slice(&mut self, range: std::ops::Range<usize>) -> impl Iterator<Item = usize> {
        range
    }
}

// A list of rows of tiles in a scroll view, which moves one row a frame: the
// virtual stack makes the rows the viewport shows and drops the one that left.
fn scroll_patch(grid: &Grid) -> impl IntoView {
    let PatchTiles {
        columns,
        rows,
        width,
        height,
    } = grid.tiles;
    let total = patchwork_scroll_rows(columns);
    let pitch = f64::from(height + WALL_GAP);
    let (frame, kit) = (grid.frame, grid.kit.clone());
    let (row_width, row_height) = (grid.field_size(columns, 1, width).0, height + WALL_GAP);
    let (patch_width, patch_height) = (grid.width, grid.height);
    scroll(
        virtual_stack(
            VirtualDirection::Vertical,
            VirtualItemSize::Fixed(Box::new(move || pitch)),
            move || Rows(total),
            |row| *row,
            move |row| {
                TileGrid::new(
                    &kit,
                    (columns, 1),
                    (width, WALL_GAP),
                    move |column| row * columns + column,
                    |index| rgb(wall_fill_rgb(index)),
                )
                .field()
                .style(move |s| s.size(row_width, row_height).flex_shrink(0.0_f32))
            },
        )
        .style(|s| s.flex_col().width_full()),
    )
    .scroll_to(move || {
        let first = patchwork_scroll_first(frame.get(), total, rows);
        Some(Point::new(0.0, first as f64 * pitch))
    })
    .scroll_style(|s| s.hide_bars(true))
    .style(move |s| s.size(patch_width, patch_height))
}

// One paragraph of words, kept as lines of text shaped once on a bitmap. A
// word that changes shapes its line again and draws it over the old one;
// digits are as wide as one another, so no line breaks at another word for it.
// A paragraph is ten thousand glyphs, more than Vello draws in a frame.
struct Paragraph {
    id: ViewId,
    words: usize,
    digit_type: Type,
    lines: Vec<Line>,
    size: (f32, f32),
    // The lines that the bitmap does not show as they are yet.
    stale: Vec<usize>,
    raster: Option<Raster>,
    image: Option<Image>,
}

struct Line {
    first_word: usize,
    text: String,
    layout: TextLayout,
}

fn paragraph(grid: &Grid) -> impl IntoView {
    let words = patchwork_paragraph_words(grid.tiles.count());
    let digit_type = grid.label_type(Face::Regular, 1.4);
    let text = patchwork_paragraph(0, words);
    // Where a paragraph wraps is where its first shaping wraps.
    let wrapped = digit_type.shape(&text, Some(grid.width - 2.0 * WALL_GAP));
    let word_bytes = wall_label(0).len() + 1;
    let mut starts: Vec<usize> = wrapped
        .layout_runs()
        .filter_map(|run| run.glyphs.first().map(|glyph| glyph.start / word_bytes))
        .collect();
    starts.dedup();
    let lines: Vec<Line> = starts
        .iter()
        .enumerate()
        .map(|(line, first_word)| {
            let end = starts.get(line + 1).copied().unwrap_or(words);
            Line::new(&text, *first_word, end, digit_type)
        })
        .collect();
    let id = ViewId::new();
    let frame = grid.frame;
    create_effect(move |_| id.update_state(frame.get()));
    let size = (grid.width, grid.height);
    Paragraph {
        id,
        words,
        digit_type,
        stale: (0..lines.len()).collect(),
        lines,
        size,
        raster: None,
        image: None,
    }
    .style(move |s| s.size(size.0, size.1).flex_shrink(0.0_f32))
}

impl Line {
    // The words `first..end` of `text`, each in `wall_label`'s width.
    fn new(text: &str, first: usize, end: usize, digit_type: Type) -> Self {
        let word_bytes = wall_label(0).len() + 1;
        let line = text[first * word_bytes..end * word_bytes - 1].to_owned();
        Self {
            first_word: first,
            layout: digit_type.shape(&line, None),
            text: line,
        }
    }
}

impl View for Paragraph {
    fn id(&self) -> ViewId {
        self.id
    }

    fn update(&mut self, _cx: &mut UpdateCx, state: Box<dyn std::any::Any>) {
        let Ok(frame) = state.downcast::<u32>() else {
            return;
        };
        let text = patchwork_paragraph(*frame, self.words);
        let word_bytes = wall_label(0).len() + 1;
        for (index, line) in self.lines.iter_mut().enumerate() {
            let start = line.first_word * word_bytes;
            let now = &text[start..start + line.text.len()];
            if now != line.text {
                line.text = now.to_owned();
                line.layout = self.digit_type.shape(now, None);
                self.stale.push(index);
            }
        }
        self.id.request_paint();
    }

    fn paint(&mut self, cx: &mut PaintCx) {
        let (width, height) = self.size;
        let raster = self
            .raster
            .get_or_insert_with(|| Raster::new(self.size, cx.scale() as f32, BACKGROUND));
        let line_height = self.digit_type.line_height();
        for index in self.stale.drain(..) {
            let top = WALL_GAP + index as f32 * line_height;
            raster.clear_rect((0.0, top, width, line_height), BACKGROUND);
            raster.text(
                &self.lines[index].layout,
                Point::new(f64::from(WALL_GAP), f64::from(top)),
            );
            self.image = None;
        }
        let image = self.image.get_or_insert_with(|| raster.snapshot());
        cx.draw_img(
            Img {
                img: image.clone(),
                hash: &[],
            },
            Rect::new(0.0, 0.0, f64::from(width), f64::from(height)),
        );
    }
}

// Translucent tiles lying over one another, one of which is opaque in each
// frame.
struct Pile {
    id: ViewId,
    kit: Rc<TileKit>,
    frame: Frame,
    grid: (usize, usize),
    size: (f32, f32),
    step: (f32, f32),
    text: Vec<crate::tiles::TileText>,
}

fn pile(grid: &Grid) -> impl IntoView {
    let (columns, rows) = patchwork_pile_grid(grid.tiles.columns, grid.tiles.rows);
    let kit = grid.kit.clone();
    let (patch_width, patch_height) = (grid.width, grid.height);
    Pile {
        id: ViewId::new(),
        text: (0..columns * rows).map(|index| kit.text(index)).collect(),
        kit,
        frame: grid.frame,
        grid: (columns, rows),
        size: (
            patchwork_double(grid.tiles.width),
            patchwork_double(grid.tiles.height),
        ),
        step: (grid.tiles.width + WALL_GAP, grid.tiles.height + WALL_GAP),
    }
    .style(move |s| s.size(patch_width, patch_height).flex_shrink(0.0_f32))
}

impl View for Pile {
    fn id(&self) -> ViewId {
        self.id
    }

    fn paint(&mut self, cx: &mut PaintCx) {
        let (columns, rows) = self.grid;
        let opaque = patchwork_pile_opaque(self.frame.now(), columns * rows);
        for (index, text) in self.text.iter().enumerate() {
            let fill = if index == opaque {
                rgb(PATCHWORK_PILE_OPAQUE_RGB)
            } else {
                let [red, green, blue] = wall_fill_rgb(index);
                Color::rgba8(red, green, blue, PATCHWORK_PILE_ALPHA)
            };
            let at = (
                f64::from(WALL_GAP) + (index % columns) as f64 * f64::from(self.step.0),
                f64::from(WALL_GAP) + (index / columns) as f64 * f64::from(self.step.1),
            );
            self.kit.paint(cx, at, self.size, fill, index, text);
        }
    }
}
