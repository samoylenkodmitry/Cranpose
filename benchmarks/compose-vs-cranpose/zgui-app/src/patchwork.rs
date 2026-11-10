//! The patchwork in zgui (`../README.md`): nine patches of the still wall's
//! tiles and the chat beside them.
//!
//! In zgui's idiom every element is built once and a signal moves what a
//! frame changes. The recolor and deep patches keep one signal per row of
//! items, which the frame loop sets only for the row that changed, so the
//! other rows run nothing. The cards patch moves each card by a transform
//! that reads the frame, and its keyed list changes only when a move begins.
//! The reorder patch gives each item an `order` signal: a swap sets two. The scroll
//! patch builds the rows in view with `Virtualize` and scrolls its container.
//! The pile patch asks a `Selector` which tile is opaque, so only the two
//! tiles that change run an effect. The charts and the still patch have no
//! signal to read. The paragraph is one text, which a frame writes again.

use std::cell::RefCell;

use perf_data::*;
use zgui::{
    canvas::{Brush, ShapeBuilder, zgui_color::Color},
    elements::{
        Box_, Element, kurbo, {self as e},
    },
    geom::{DevicePx, Point},
    prelude::*,
    reactive::Selector,
    view::{AnyView, ClassName, ForProps},
};
use zgui_ui::virtualize::Virtualize;

use crate::{
    chat::Chat,
    run_frames,
    sheet::hex,
    wall::{page, tile, tile_content, tiles},
};

/// The size of one patch: three patches share the width the chat leaves and
/// the height under the clock line.
pub(crate) fn patch_size() -> (f32, f32) {
    let (width, height) = DESKTOP_WINDOW;
    (
        ((width as f32 - CHAT_WIDTH) / PATCH_COLUMNS as f32).floor(),
        ((height as f32 - 56.0 - WALL_CLOCK_HEIGHT) / PATCH_ROWS as f32).floor(),
    )
}

/// What every patch shares.
struct Patches {
    tier: WallTier,
    tiles: PatchTiles,
    patch_height: f32,
    frame: RwSignal<u32>,
    /// Per row of the recolor patch's tiles and of the deep patch's nests:
    /// the frame its items last changed in.
    recolor: Vec<RwSignal<u32>>,
    deep: Vec<RwSignal<u32>>,
    /// The place each item of the reorder patch lies at.
    places: Vec<RwSignal<usize>>,
    /// The tile of the pile patch that is opaque.
    opaque: RwSignal<usize>,
    port: NodeRef,
    seen: Virtualize,
}

impl Patches {
    fn new(tier: WallTier, tiles: PatchTiles, patch_height: f32) -> Self {
        let port = NodeRef::new();
        let (_, deep_rows) = patchwork_pairs(tiles.columns, tiles.rows);
        let scroll_rows = patchwork_scroll_rows(tiles.columns);
        Self {
            tier,
            tiles,
            patch_height,
            frame: RwSignal::new(0),
            recolor: (0..tiles.rows).map(|_| RwSignal::new(0)).collect(),
            deep: (0..deep_rows).map(|_| RwSignal::new(0)).collect(),
            places: (0..patchwork_reorder_count(tiles.count()))
                .map(RwSignal::new)
                .collect(),
            opaque: RwSignal::new(0),
            port,
            seen: Virtualize::new(
                port,
                Signal::stored_local(scroll_rows),
                scroll_row_size(tier),
                2,
            ),
        }
    }

    /// Moves every patch to `frame`, the one after the frame it showed.
    fn step(&self, frame: u32) {
        self.frame.set(frame);
        let turn = self.tiles.in_turn();
        for (row, last) in self.recolor.iter().enumerate() {
            if turn.step(row * turn.columns, frame) != turn.step(row * turn.columns, frame - 1) {
                last.set(frame);
            }
        }
        let (columns, rows) = patchwork_pairs(self.tiles.columns, self.tiles.rows);
        let nests = InTurn { columns, rows };
        for (row, last) in self.deep.iter().enumerate() {
            if nests.step(row * columns, frame) != nests.step(row * columns, frame - 1) {
                last.set(frame);
            }
        }
        // The two places swapped in the last frame and the two swapped in this
        // one: every other item lies at the place of its own number.
        let count = self.places.len();
        for place in [frame - 1, frame].into_iter().flat_map(|frame| {
            let swap = patchwork_reorder_swap(frame, count);
            [swap, swap + 1]
        }) {
            if place < count {
                let item = patchwork_reorder_item(place, frame, count);
                if self.places[item].get_untracked() != place {
                    self.places[item].set(place);
                }
            }
        }
        let (columns, rows) = patchwork_pile_grid(self.tiles.columns, self.tiles.rows);
        let opaque = patchwork_pile_opaque(frame, columns * rows);
        if opaque != self.opaque.get_untracked() {
            self.opaque.set(opaque);
        }
        let first = patchwork_scroll_first(
            frame,
            patchwork_scroll_rows(self.tiles.columns),
            self.tiles.rows,
        );
        let offset = first as f32 * scroll_row_size(self.tier) * self.port.scale();
        self.port.scroll_to(
            ScrollTarget::Offset(Point::new(DevicePx(0.0), DevicePx(offset))),
            ScrollBehavior::Instant,
        );
    }

    /// The patches in the screen's order, three to a row.
    fn view(&self) -> impl IntoView + use<> {
        e::column()
            .class("patches")
            .children(Patch::ALL.chunks(PATCH_COLUMNS).map(|row| {
                AnyView::new(
                    e::row()
                        .class("patch-row")
                        .children(row.iter().map(|patch| self.patch(*patch))),
                )
            }))
    }

    fn patch(&self, patch: Patch) -> AnyView {
        match patch {
            Patch::Still => AnyView::new(patch_box().child(tiles(
                self.tiles.columns,
                self.tiles.rows,
                self.tier,
            ))),
            Patch::Charts => AnyView::new(patch_box().child(self.charts())),
            Patch::Recolor => AnyView::new(patch_box().child(self.recolored())),
            Patch::Cards => AnyView::new(patch_box().child(self.cards())),
            Patch::Reorder => AnyView::new(self.reorder()),
            Patch::Deep => AnyView::new(patch_box().child(self.nests())),
            Patch::Scroll => AnyView::new(patch_box().child(self.scroll())),
            Patch::Paragraph => {
                AnyView::new(patch_box().class("paragraph").child(self.paragraph()))
            }
            Patch::Pile => AnyView::new(patch_box().child(self.pile())),
        }
    }

    /// Line charts: each canvas draws its line once, as no signal is read.
    fn charts(&self) -> Element<zgui::elements::Column> {
        let (columns, rows) = patchwork_pairs(self.tiles.columns, self.tiles.rows);
        e::column().class("tiles").children((0..rows).map(|line| {
            AnyView::new(e::row().class("line").children(
                (0..columns).map(move |column| AnyView::new(chart(line * columns + column))),
            ))
        }))
    }

    /// Tiles of which a row reads its row's signal for its fill.
    fn recolored(&self) -> Element<zgui::elements::Column> {
        let (columns, tier, turn) = (self.tiles.columns, self.tier, self.tiles.in_turn());
        e::column()
            .class("tiles")
            .children(
                self.recolor
                    .iter()
                    .copied()
                    .enumerate()
                    .map(move |(line, last)| {
                        AnyView::new(e::row().class("line").children((0..columns).map(
                            move |column| {
                                let index = line * columns + column;
                                AnyView::new(tile_content(
                                    e::r#box().style_property("background-color", move || {
                                        Some(hex(patchwork_recolor_rgb(index, last.get(), turn)))
                                    }),
                                    index,
                                    tier,
                                ))
                            },
                        )))
                    }),
            )
    }

    /// The cards that rest or move in the patch. The list changes when a move
    /// begins; within a move each card's transform reads the frame.
    fn cards(&self) -> impl IntoView + use<> {
        let cards = Cards::new(
            self.tiles.columns,
            self.tiles.rows,
            self.tier.tile_height,
            self.patch_height,
        );
        let (frame, tier) = (self.frame, self.tier);
        let moves = Memo::new(move |_| frame.get() / PATCHWORK_CARD_FRAMES);
        ForProps::builder()
            .each(move || {
                let begun = moves.get() * PATCHWORK_CARD_FRAMES;
                (0..cards.places())
                    .filter_map(|place| cards.place(place, begun))
                    .map(|card| card.id)
                    .collect::<Vec<_>>()
            })
            .key(|id: &usize| *id)
            .children(move |id| card(id, cards, tier, frame))
            .build()
            .render()
    }

    /// A wrapping row of items, each placed by its `order`: a swap sets the
    /// order of two items and no other item runs anything.
    fn reorder(&self) -> impl IntoView + use<> {
        let tier = self.tier;
        e::row()
            .class("patch reorder")
            .children(
                self.places
                    .iter()
                    .copied()
                    .enumerate()
                    .map(move |(item, place)| {
                        let width =
                            patchwork_reorder_width(patchwork_reorder_units(item), tier.tile_width);
                        AnyView::new(
                            tile(item, tier)
                                .style_property("width", Some(format!("{width}px")))
                                .style_property("order", move || Some(place.get().to_string())),
                        )
                    }),
            )
    }

    /// Nests whose digits read their row's signal.
    fn nests(&self) -> Element<zgui::elements::Column> {
        let (columns, rows) = patchwork_pairs(self.tiles.columns, self.tiles.rows);
        let (tier, nests) = (self.tier, InTurn { columns, rows });
        e::column()
            .class("tiles")
            .children(
                self.deep
                    .iter()
                    .copied()
                    .enumerate()
                    .map(move |(line, last)| {
                        AnyView::new(e::row().class("line").children((0..columns).map(
                            move |column| {
                                AnyView::new(nest(line * columns + column, last, nests, tier))
                            },
                        )))
                    }),
            )
    }

    /// A list of rows of tiles of which `Virtualize` builds the rows in view;
    /// the frame loop scrolls its container by a row.
    fn scroll(&self) -> impl IntoView + use<> {
        let (columns, tier, seen) = (self.tiles.columns, self.tier, self.seen);
        e::scroll().class("scroller").node_ref(self.port).child(
            e::column()
                .style_property("padding-top", move || {
                    Some(format!("{}px", seen.window().lead))
                })
                .style_property("padding-bottom", move || {
                    Some(format!("{}px", seen.window().trail))
                })
                .child(
                    ForProps::builder()
                        .each(move || seen.window().indices())
                        .key(|row: &usize| *row)
                        .children(move |row| {
                            e::row()
                                .class("scroll-row")
                                .children((0..columns).map(move |column| {
                                    AnyView::new(tile(row * columns + column, tier))
                                }))
                        })
                        .build()
                        .render(),
                ),
        )
    }

    /// One text, written again each frame.
    fn paragraph(&self) -> impl IntoView + use<> {
        let (frame, words) = (self.frame, patchwork_paragraph_words(self.tiles.count()));
        e::text().child(move || patchwork_paragraph(frame.get(), words))
    }

    /// Translucent tiles over one another; a selector tells each tile whether
    /// it is the opaque one, so a frame runs the two tiles that change.
    fn pile(&self) -> impl IntoView + use<> {
        let (columns, rows) = patchwork_pile_grid(self.tiles.columns, self.tiles.rows);
        let (tier, opaque) = (self.tier, self.opaque);
        let selector = Selector::new(move || opaque.get());
        e::r#box()
            .class("pile-field")
            .children((0..columns * rows).map(move |index| {
                let selector = selector.clone();
                AnyView::new(tile_content(
                    e::r#box()
                        .class(format!("pile pile{}", index % PALETTE_RGB.len()))
                        .class_toggle(ClassName::new("pile-opaque"), move || {
                            selector.is_selected(&index)
                        })
                        .style_property(
                            "left",
                            Some(format!(
                                "{}px",
                                WALL_GAP + (index % columns) as f32 * (tier.tile_width + WALL_GAP)
                            )),
                        )
                        .style_property(
                            "top",
                            Some(format!(
                                "{}px",
                                WALL_GAP + (index / columns) as f32 * (tier.tile_height + WALL_GAP)
                            )),
                        ),
                    index,
                    tier,
                ))
            }))
    }
}

/// The height of a row of the scroll patch: a tile and the gap under it.
fn scroll_row_size(tier: WallTier) -> f32 {
    tier.tile_height + WALL_GAP
}

/// One patch's box, which cuts what lies past its edge.
fn patch_box() -> Element<Box_> {
    e::r#box().class("patch")
}

/// Chart `index`: a canvas that draws its line once.
fn chart(index: usize) -> impl IntoView {
    let [red, green, blue] = patchwork_chart_rgb(index);
    let stroke = Brush::Solid(Color::srgb_u8(red, green, blue, 255));
    e::canvas().class("chart").draw(move |cx| {
        let (width, height) = (f64::from(cx.size.width.0), f64::from(cx.size.height.0));
        let step = width / (PATCHWORK_CHART_POINTS - 1) as f64;
        let mut line = kurbo::BezPath::new();
        for point in 0..PATCHWORK_CHART_POINTS {
            let at = kurbo::Point::new(
                point as f64 * step,
                height * (1.0 - f64::from(patchwork_chart_value(index, point))),
            );
            if point == 0 {
                line.move_to(at);
            } else {
                line.line_to(at);
            }
        }
        cx.scene.push(
            ShapeBuilder::new(line)
                .stroke(stroke.clone(), f64::from(PATCHWORK_CHART_STROKE))
                .build(),
        );
    })
}

/// Card `id`: its header over its rows of tiles, moved by a transform that
/// reads the frame.
fn card(id: usize, cards: Cards, tier: WallTier, frame: RwSignal<u32>) -> impl IntoView + use<> {
    let (label, _) = wall_text_size(tier.tile_height);
    let head = e::r#box()
        .class("card-head")
        .style_text(Some(format!(
            "width: {}px; background-color: {}; padding: {}px",
            patchwork_reorder_width(cards.columns, tier.tile_width),
            hex(patchwork_card_rgb(id)),
            wall_padding(tier.tile_height),
        )))
        .child(
            e::text()
                .class("card-title")
                .style_text(Some(format!("font-size: {label}px")))
                .child(patchwork_card_title(id)),
        );
    let rows = (0..cards.rows).map(move |row| {
        AnyView::new(
            e::row()
                .class("line")
                .children((0..cards.columns).map(move |column| {
                    let index =
                        patchwork_card_tile(id, row * cards.columns + column, cards.tiles());
                    AnyView::new(tile(index, tier))
                })),
        )
    });
    e::column()
        .class("card")
        .style_property("transform", move || {
            let top = cards.top(id, frame.get()).unwrap_or(-cards.height);
            Some(format!("translateY({top}px)"))
        })
        .child(head)
        .children(rows)
}

/// Nest `index`: boxes inside one another around digits that follow the row's
/// signal.
fn nest(index: usize, last: RwSignal<u32>, turn: InTurn, tier: WallTier) -> impl IntoView {
    let (label, _) = wall_text_size(tier.tile_height);
    let digits = e::text()
        .class("nest-text")
        .style_text(Some(format!("font-size: {label}px")))
        .child(move || patchwork_deep_value(index, last.get(), turn));
    let padding = format!("padding: {}px", wall_padding(tier.tile_height));
    let mut inner = AnyView::new(digits);
    for level in (0..PATCHWORK_DEPTH).rev() {
        inner = AnyView::new(
            e::column()
                .class("nest-level")
                .style_text(Some(format!(
                    "background-color: {}; {padding}",
                    hex(patchwork_deep_rgb(index, level)),
                )))
                .child(inner),
        );
    }
    e::column().class("nest").child(inner)
}

pub(crate) fn screen() -> AnyView {
    let launch = perf_data::launch();
    let tier = wall_tier(launch.tier);
    let (patch_width, patch_height) = patch_size();
    let patches = Patches::new(
        tier,
        PatchTiles::new(tier, patch_width, patch_height),
        patch_height,
    );
    let frame = patches.frame;
    let chat = Chat::new();
    let page = page("Patchwork", frame).child(
        e::row()
            .class("body")
            .child(patches.view())
            .child(chat.view(frame)),
    );
    let index = RefCell::new(FrameIndex::new(launch.freeze));
    run_frames(move || {
        let (current, frozen) = {
            let mut index = index.borrow_mut();
            index.advance();
            (index.get(), index.frozen())
        };
        patches.step(current);
        chat.advance();
        !frozen
    });
    AnyView::new(page)
}
