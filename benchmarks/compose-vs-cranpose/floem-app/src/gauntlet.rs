//! The gauntlet in Floem: the screen `compose-app/.../Gauntlet.kt` draws,
//! element for element. The benchmark's README describes it.
//!
//! Floem builds each view once. The signals of the frame feed the views that
//! change: a ticker tile's texts and bar read a memo of its price, a card's
//! progress reads a memo of its permille, the content's width, the list's
//! shift and each panel's place and turn are style closures that read the
//! frame, which Floem applies as a transform or a box. Floem has no list that
//! measures rows of unequal heights, so a keyed `dyn_stack` makes the rows from
//! the first one on screen to the window's bottom, each row reports its height
//! once laid out, and the list moves by the shift of its first row.
//!
//! Vello makes a clip layer for each view painted under a clip, so nothing
//! that holds the list clips. The list lies under the ticker panel and the top
//! bar, which come after it in the tree and cover what it scrolled past, and
//! the window's edge cuts it at the bottom.

use std::{cell::RefCell, collections::HashMap, rc::Rc};

use floem::{
    IntoView, View, ViewId,
    context::PaintCx,
    kurbo::{BezPath, Point, Rect, Stroke},
    peniko::{Brush, Color, Gradient, Image},
    prelude::*,
    reactive::{Memo, create_effect, create_memo},
    style::Style,
    taffy::{FlexDirection, FlexWrap},
    views::{clip, dyn_stack, rich_text},
};
use floem_renderer::Renderer as _;
use perf_data::*;

use crate::{
    frame::Frame,
    picture::{bitmap, picture},
    rgb,
    text::{Face, Type},
    top_bar,
};

const INK: Color = rgb(GAUNTLET_INK_RGB);
const BODY: Color = rgb(GAUNTLET_BODY_RGB);
const MUTED: Color = rgb(GAUNTLET_MUTED_RGB);
const HAIRLINE: Color = rgb(GAUNTLET_HAIRLINE_RGB);
const PANEL: Color = rgb(GAUNTLET_PANEL_RGB);
const BACKGROUND: Color = rgb(GAUNTLET_BACKGROUND_RGB);
const UP: Color = rgb(GAUNTLET_UP_RGB);
const DOWN: Color = rgb(GAUNTLET_DOWN_RGB);
const LAYER_BACKGROUND: Color = Color::rgba8(0x1E, 0x29, 0x3B, 0xC0);
const NESTED_BACKGROUND: Color = Color::rgba8(0xFF, 0xFF, 0xFF, 0xE6);
const FOOTER_LABELS: [&str; 3] = ["likes", "replies", "shares"];

fn palette(index: usize) -> Color {
    rgb(PALETTE_RGB[index % PALETTE_RGB.len()])
}

/// What every card, ticker tile and level reads: the tier's sizes and the
/// frame.
#[derive(Clone)]
struct Scene {
    s: f32,
    frame: Frame,
    avatars: Rc<Vec<Image>>,
}

/// A label in `size` points.
fn words(content: impl Into<String>, size: f32, bold: bool, color: Color) -> impl IntoView {
    text(content.into()).style(move |s| style_text(s, size, bold, color))
}

fn style_text(s: Style, size: f32, bold: bool, color: Color) -> Style {
    s.font_size(size)
        .color(color)
        .apply_if(bold, |s| s.font_bold())
}

/// Wrapped text cut after `lines` lines, 1.4 lines tall: Floem's labels have
/// no line limit, so a view that clips holds the label.
fn paragraph(
    content: &'static str,
    size: f32,
    color: Color,
    bold: bool,
    lines: usize,
) -> impl IntoView {
    let line = size * 1.4;
    clip(text(content).style(move |s| {
        style_text(s, size, bold, color)
            .line_height(1.4)
            .width_full()
            .min_width(0.0)
    }))
    .style(move |s| {
        s.width_full()
            .min_width(0.0)
            .max_height(line * lines as f32)
    })
}

/// A horizontal bar `share` full on a rounded track, `share` read in a style.
fn bar(share: impl Fn() -> f32 + 'static, color: Color, girth: f32) -> impl IntoView {
    container(empty().style(move |s| {
        s.height_full()
            .width(f64::from(share().clamp(0.0, 1.0) * 100.0).pct())
            .background(color)
            .border_radius(girth / 2.0)
    }))
    .style(move |s| {
        s.height(girth)
            .background(HAIRLINE)
            .border_radius(girth / 2.0)
    })
}

pub fn screen(frame: Frame) -> impl IntoView {
    let gauntlet = desktop_gauntlet();
    let tier = gauntlet.tier;
    let scene = Scene {
        s: tier.scale,
        frame,
        avatars: Rc::new((0..AVATAR_COUNT).map(round_avatar).collect()),
    };
    let (window_width, _) = DESKTOP_WINDOW;
    let width = move || (window_width as f32 * width_fraction(frame.get())).round();
    // The list is first in the tree, and so is painted first: the ticker panel
    // and the top bar lie over what it scrolled past, and the column runs from
    // the bottom up to show them above it.
    v_stack((
        list(&scene).style(move |s| s.width(width()).flex_grow(1.0_f32).min_height(0.0)),
        ticker_panel(&scene).style(move |s| s.width(width())),
        top_bar("Gauntlet"),
    ))
    .style(|s| {
        s.size_full()
            .flex_direction(FlexDirection::ColumnReverse)
            .items_start()
            .background(BACKGROUND)
    })
}

/// Avatar `index` as a disc: the bitmap is cut to a circle once, as Floem
/// clips to rectangles and rounded rectangles only, and the rows of a list
/// lie under a clip it would add a layer for.
fn round_avatar(index: usize) -> Image {
    let mut pixels = avatar_rgba(index);
    let side = AVATAR_SIZE as usize;
    let radius = side as f32 / 2.0;
    for (at, pixel) in pixels.as_chunks_mut::<4>().0.iter_mut().enumerate() {
        let (x, y) = (
            (at % side) as f32 + 0.5 - radius,
            (at / side) as f32 + 0.5 - radius,
        );
        let reach = (radius - (x * x + y * y).sqrt() + 0.5).clamp(0.0, 1.0);
        pixel[3] = (reach * 255.0) as u8;
    }
    bitmap(AVATAR_SIZE, AVATAR_SIZE, pixels)
}

// ------------------------------------------------------------------- ticker

fn ticker_panel(scene: &Scene) -> impl IntoView + use<> {
    let s = scene.s;
    let tiles = desktop_gauntlet()
        .tickers
        .iter()
        .enumerate()
        .map(|(index, ticker)| ticker_tile(scene, index, ticker));
    h_stack_from_iter(tiles).style(move |st| {
        st.flex_wrap(FlexWrap::Wrap)
            .gap(4.0 * s)
            .padding(6.0 * s)
            .flex_shrink(0.0_f32)
            .background(PANEL)
    })
}

/// One quote: symbol, price, change and a bar, all from the frame.
fn ticker_tile(scene: &Scene, index: usize, ticker: &'static Ticker) -> impl IntoView + use<> {
    let (s, frame) = (scene.s, scene.frame);
    // A frame moves the price of every quote; what reads it is told once.
    let cents = create_memo(move |_| ticker_cents(ticker, frame.get()));
    h_stack((
        words(ticker.symbol.clone(), 10.0 * s, true, INK),
        label(move || cents_text(cents.get()))
            .style(move |st| style_text(st, 10.0 * s, false, BODY)),
        label(move || change_text(ticker, cents.get())).style(move |st| {
            style_text(
                st,
                10.0 * s,
                false,
                if cents.get() >= ticker.base_cents {
                    UP
                } else {
                    DOWN
                },
            )
        }),
        bar(
            move || {
                (cents.get() - ticker.base_cents + ticker.swing_cents) as f32
                    / (2 * ticker.swing_cents) as f32
            },
            palette(index),
            4.0 * s,
        )
        .style(move |st| st.width(20.0 * s)),
    ))
    .style(move |st| {
        st.gap(4.0 * s)
            .items_center()
            .padding_vert(3.0 * s)
            .padding_horiz(6.0 * s)
            .background(Color::WHITE)
            .border_radius(6.0 * s)
    })
}

// --------------------------------------------------------------------- list

struct Measured {
    anchor: ListAnchor,
    heights: HashMap<usize, f32>,
}

type Heights = Rc<RefCell<Measured>>;

/// The rows from the first one on screen down to the window's bottom, and the
/// panels stacked over them.
fn list(scene: &Scene) -> impl IntoView + use<> {
    let (s, frame) = (scene.s, scene.frame);
    let gap = 8.0 * s;
    let measured: Heights = Rc::new(RefCell::new(Measured {
        anchor: ListAnchor { row: 0, top: gap },
        heights: HashMap::new(),
    }));
    let anchor = RwSignal::new(measured.borrow().anchor);
    advance_anchor(&measured, anchor, frame, gap);
    let rows = row_window(&measured, anchor, frame, (gap, s));
    let rows = row_stack(scene, &measured, rows, anchor, gap);
    let layers =
        (0..desktop_gauntlet().tier.layers).map(|layer| stacked_layer(scene, layer).into_any());
    let children: Vec<floem::AnyView> = std::iter::once(rows.into_any()).chain(layers).collect();
    stack_from_iter(children)
}

// The anchor moves on once its row scrolled off, as far as the rows reported
// their heights.
fn advance_anchor(measured: &Heights, anchor: RwSignal<ListAnchor>, frame: Frame, gap: f32) {
    let measured = measured.clone();
    create_effect(move |_| {
        let offset = frame.get() as f32 * SCROLL_PER_FRAME;
        let next = {
            let mut measured = measured.borrow_mut();
            let next = measured
                .anchor
                .advanced(offset, gap, |row| measured.heights.get(&row).copied());
            if next != measured.anchor {
                measured.anchor = next;
                measured.heights.retain(|row, _| *row >= next.row);
            }
            next
        };
        // The signal is set once the borrow ended: what reads it borrows the
        // heights too.
        if next != anchor.get_untracked() {
            anchor.set(next);
        }
    });
}

// The rows from the anchor down to the window's bottom.
fn row_window(
    measured: &Heights,
    anchor: RwSignal<ListAnchor>,
    frame: Frame,
    (gap, s): (f32, f32),
) -> Memo<std::ops::Range<usize>> {
    let measured = measured.clone();
    let (_, window_height) = DESKTOP_WINDOW;
    create_memo(move |_| {
        let offset = frame.get() as f32 * SCROLL_PER_FRAME;
        let anchor = anchor.get();
        let measured = measured.borrow();
        anchor.rows(offset, window_height as f32, gap, 10.0 * s, |row| {
            measured.heights.get(&row).copied()
        })
    })
}

// The keyed stack of rows, moved by the shift of its first row.
fn row_stack(
    scene: &Scene,
    measured: &Heights,
    rows: Memo<std::ops::Range<usize>>,
    anchor: RwSignal<ListAnchor>,
    gap: f32,
) -> impl IntoView + use<> {
    let (scene_rows, frame, measured) = (scene.clone(), scene.frame, measured.clone());
    dyn_stack(
        move || rows.get(),
        |row| *row,
        move |row| {
            let measured = measured.clone();
            row_view(&scene_rows, row).on_resize(move |rect: Rect| {
                measured
                    .borrow_mut()
                    .heights
                    .insert(row, rect.height() as f32);
            })
        },
    )
    .style(move |st| {
        st.flex_direction(FlexDirection::Column)
            .gap(gap)
            .padding_horiz(gap)
            .width_full()
            .translate_y(anchor.get().top - frame.get() as f32 * SCROLL_PER_FRAME)
    })
}

fn row_view(scene: &Scene, row: usize) -> floem::AnyView {
    let tier = desktop_gauntlet().tier;
    let s = scene.s;
    match gauntlet_row(row, tier.columns) {
        GauntletRow::Cards(first) => h_stack_from_iter((first..first + tier.columns).map(|card| {
            card_view(scene, card).style(|st| st.flex_grow(1.0_f32).flex_basis(0.0).min_width(0.0))
        }))
        .style(move |st| st.gap(8.0 * s).width_full())
        .into_any(),
        GauntletRow::Cluster(cluster) => level(cluster, tier.depth, s).into_any(),
    }
}

// --------------------------------------------------------------------- card

fn card_view(scene: &Scene, card: usize) -> impl IntoView + use<> {
    let (s, frame) = (scene.s, scene.frame);
    let post = &desktop_gauntlet().posts[card % POST_COUNT];
    let permille = create_memo(move |_| progress_permille(card, frame.get()));
    let body = v_stack((
        card_header(scene, card, post),
        paragraph(&post.body, 12.0 * s, BODY, false, 4),
        progress(permille, card, s),
        Spark {
            id: ViewId::new(),
            card,
            frame,
            color: palette(post.color),
            stroke: 1.5 * s,
        }
        .style(move |st| st.width_full().height(36.0 * s)),
        chips(post, s),
        footer(post, s),
    ))
    .style(move |st| {
        st.width_full()
            .gap(6.0 * s)
            .padding(10.0 * s)
            .background(Color::WHITE)
            .border(1.0)
            .border_color(HAIRLINE)
            .border_radius(12.0 * s)
            .box_shadow_color(Color::rgba8(0, 0, 0, 61))
            .box_shadow_h_offset(0.0)
            .box_shadow_v_offset(s)
            .box_shadow_blur(3.0 * s)
    });
    // A translucent tag tilting with the frame, turned and never laid out
    // again.
    let badge = card.is_multiple_of(5).then(|| {
        text("HOT").style(move |st| {
            style_text(st, 9.0 * s, true, Color::WHITE.multiply_alpha(0.9))
                .absolute()
                .inset_top(6.0 * s)
                .inset_right(6.0 * s)
                .padding_vert(2.0 * s)
                .padding_horiz(6.0 * s)
                .background(palette(0).multiply_alpha(0.9))
                .border_radius(8.0 * s)
                .rotate(f64::from(badge_degrees(card, frame.get()).to_radians()))
        })
    });
    let children: Vec<floem::AnyView> = std::iter::once(body.into_any())
        .chain(badge.map(|badge| badge.into_any()))
        .collect();
    stack_from_iter(children).style(|st| st.width_full())
}

/// The avatar, the title and the subtitle.
fn card_header(scene: &Scene, card: usize, post: &'static Post) -> impl IntoView + use<> {
    let s = scene.s;
    let (tag, tag_color) = &post.tags[0];
    // `by Ada Lovelace · #rust`: the author bold, the tag in its color.
    let subtitle = Type::new(11.0 * s, 1.3, Face::Regular, MUTED).shape_runs(&[
        ("by ", false, MUTED),
        (&post.author, true, MUTED),
        (" · ", false, MUTED),
        (tag, false, rgb(GRADIENT_END_RGB[*tag_color])),
    ]);
    let subtitle = rich_text(move || subtitle.clone());
    let avatar = picture(scene.avatars.clone(), move || card % AVATAR_COUNT)
        .style(move |st| st.size(32.0 * s, 32.0 * s).flex_shrink(0.0_f32));
    let heading = v_stack((
        paragraph(&post.title, 13.0 * s, INK, true, 2),
        clip(subtitle).style(|st| st.width_full().min_width(0.0)),
    ))
    .style(|st| st.flex_grow(1.0_f32).flex_basis(0.0).min_width(0.0));
    h_stack((avatar, heading)).style(move |st| st.gap(8.0 * s).items_center().width_full())
}

fn progress(permille: Memo<u32>, card: usize, s: f32) -> impl IntoView {
    h_stack((
        bar(
            move || permille.get() as f32 / 1000.0,
            palette(card),
            6.0 * s,
        )
        .style(|st| st.flex_grow(1.0_f32).flex_basis(0.0)),
        label(move || format!("{}%", permille.get() / 10))
            .style(move |st| style_text(st, 10.0 * s, false, MUTED)),
    ))
    .style(move |st| st.gap(6.0 * s).items_center().width_full())
}

fn chips(post: &'static Post, s: f32) -> impl IntoView {
    h_stack_from_iter(post.tags.iter().map(move |(tag, color)| {
        container(words(
            tag.clone(),
            10.0 * s,
            false,
            rgb(GRADIENT_END_RGB[*color]),
        ))
        .style(move |st| {
            st.padding_vert(3.0 * s)
                .padding_horiz(8.0 * s)
                .background(rgb(CHIP_BACKGROUND_RGB[*color]))
                .border_radius(10.0 * s)
        })
    }))
    .style(move |st| st.flex_wrap(FlexWrap::Wrap).gap(4.0 * s).width_full())
}

/// Three counters split by dividers as tall as the row.
fn footer(post: &'static Post, s: f32) -> impl IntoView {
    let counters = (0..FOOTER_LABELS.len()).flat_map(|index| {
        let counter = v_stack((
            words(post.stats[index].clone(), 12.0 * s, true, INK),
            words(FOOTER_LABELS[index], 9.0 * s, false, MUTED),
        ))
        .style(|st| {
            st.items_center()
                .flex_grow(1.0_f32)
                .flex_basis(0.0)
                .min_width(0.0)
        })
        .into_any();
        let divider = (index > 0).then(|| {
            empty()
                .style(|st| st.width(1.0).background(HAIRLINE).flex_shrink(0.0_f32))
                .into_any()
        });
        divider.into_iter().chain(std::iter::once(counter))
    });
    h_stack_from_iter(counters).style(|st| st.width_full())
}

// ---------------------------------------------------------------- sparkline

/// A card's 48-point line over a fading fill, painted from the frame.
struct Spark {
    id: ViewId,
    card: usize,
    frame: Frame,
    color: Color,
    stroke: f32,
}

impl View for Spark {
    fn id(&self) -> ViewId {
        self.id
    }

    fn paint(&mut self, cx: &mut PaintCx) {
        let layout = self.id.get_layout().unwrap_or_default();
        let (width, height) = (f64::from(layout.size.width), f64::from(layout.size.height));
        let step = width / (GAUNTLET_SPARK_POINTS - 1) as f64;
        let frame = self.frame.now();
        let point = |index: usize| {
            Point::new(
                index as f64 * step,
                height * f64::from(1.0 - spark_value(self.card, index, frame)),
            )
        };
        let mut area = BezPath::new();
        area.move_to((0.0, height));
        let mut line = BezPath::new();
        for index in 0..GAUNTLET_SPARK_POINTS {
            area.line_to(point(index));
            if index == 0 {
                line.move_to(point(index));
            } else {
                line.line_to(point(index));
            }
        }
        area.line_to((width, height));
        area.close_path();
        let fade = Gradient::new_linear((0.0, 0.0), (0.0, height)).with_stops([
            (0.0, self.color.multiply_alpha(0.25)),
            (1.0, Color::TRANSPARENT),
        ]);
        cx.fill(&area, &Brush::Gradient(fade), 0.0);
        cx.stroke(&line, self.color, &Stroke::new(f64::from(self.stroke)));
    }
}

// ------------------------------------------------------------------ cluster

/// Levels nested inside one another, each a row of three labels above the
/// next level.
fn level(cluster: usize, remaining: usize, s: f32) -> floem::AnyView {
    let chips = (0..3).map(move |chip| {
        container(words(
            format!("C{cluster}.L{remaining}.{chip}"),
            9.0 * s,
            false,
            Color::WHITE,
        ))
        .style(move |st| {
            st.flex_grow(1.0_f32)
                .flex_basis(0.0)
                .min_width(0.0)
                .background(palette(remaining + chip + cluster))
                .border_radius(2.0 * s)
        })
    });
    let inner = (remaining > 0).then(|| level(cluster, remaining - 1, s));
    let background = rgb(LEVEL_BACKGROUND_RGB[remaining % 2]);
    let children: Vec<floem::AnyView> = std::iter::once(
        h_stack_from_iter(chips)
            .style(move |st| st.gap(2.0 * s).width_full())
            .into_any(),
    )
    .chain(inner)
    .collect();
    v_stack_from_iter(children)
        .style(move |st| {
            st.gap(s)
                .width_full()
                .padding_top(s)
                .padding_right(s)
                .padding_bottom(s)
                .padding_left(3.0 * s)
                .background(background)
                .border_radius(4.0 * s)
        })
        .into_any()
}

// ------------------------------------------------------------------- layers

/// A translucent panel stacked over the list. Its content never changes: each
/// frame its style turns and moves its box, and turns its nested card back.
fn stacked_layer(scene: &Scene, layer: usize) -> impl IntoView + use<> {
    let frame = scene.frame;
    let rows = desktop_gauntlet().tier.layer_rows;
    let cells = v_stack_from_iter((0..rows).map(|row| {
        h_stack_from_iter(
            (row * LAYER_COLUMNS..(row + 1) * LAYER_COLUMNS).map(|cell| {
                container(words((cell + 1).to_string(), 9.0, false, Color::WHITE)).style(
                    move |st| {
                        st.size(22.0, 22.0)
                            .flex_shrink(0.0_f32)
                            .items_center()
                            .justify_center()
                            .background(palette(layer_cell_color(layer, cell)))
                            .border_radius(4.0)
                    },
                )
            }),
        )
        .style(|st| st.gap(3.0))
    }))
    .style(|st| st.gap(3.0));
    let nested = v_stack((
        words(format!("Nested in layer {}", layer + 1), 11.0, true, INK),
        words("Tilts against its panel", 11.0, false, BODY),
    ))
    .style(move |st| {
        st.gap(2.0)
            .padding(8.0)
            .background(NESTED_BACKGROUND)
            .border_radius(8.0)
            .rotate(-f64::from(layer_degrees(layer, frame.get()).to_radians()))
    });
    v_stack((
        words(format!("Layer {}", layer + 1), 13.0, true, Color::WHITE),
        cells,
        nested,
    ))
    .style(move |st| {
        st.absolute()
            .inset_left(0.0)
            .inset_top(0.0)
            .width(220.0)
            .gap(8.0)
            .padding(10.0)
            .background(LAYER_BACKGROUND)
            .border_radius(12.0)
            .translate_x(layer_x(layer, frame.get()))
            .translate_y(layer_y(layer, frame.get()))
            .rotate(f64::from(layer_degrees(layer, frame.get()).to_radians()))
    })
}
