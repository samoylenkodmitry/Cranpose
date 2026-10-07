//! The gauntlet: one screen that loads every stage of a frame at once, heavy
//! enough at its device's tier that no framework holds 60 fps. `Gauntlet.kt`
//! draws the same screen, element for element, in Jetpack Compose's own
//! idiom; the benchmark's README describes it.
//!
//! Every frame advances a frame index `k` and everything follows from it,
//! never from wall time, so both frameworks do the same work per frame:
//! - the content width follows `k`, so every visible node is measured again;
//! - the list scrolls a fixed distance;
//! - ticker prices, card progress and their labels change;
//! - sparklines and badge tilts are drawn from `k` without recomposing;
//! - stacked panels over the list move and tilt in their graphics layers,
//!   their content never changing.

use std::rc::Rc;

use cranpose::{LazyItems, prelude::*};
use cranpose_foundation::lazy::rememberLazyListState;
use cranpose_ui::{
    text::{AnnotatedString, FontWeight, SpanStyle},
    widgets::{FlowRow, FlowRowSpec},
};

use super::{clamped_text_options, text_style};
use crate::data::{
    self, AVATAR_COUNT, AVATAR_SIZE, CHIP_BACKGROUND, GAUNTLET_SPARK_POINTS, GRADIENT_END,
    GauntletRow, GauntletTier, LAYER_COLUMNS, PALETTE, Post, Ticker,
};

const INK: Color = Color::from_rgb_u8(0x11, 0x18, 0x27);
const BODY: Color = Color::from_rgb_u8(0x37, 0x41, 0x51);
const MUTED: Color = Color::from_rgb_u8(0x6B, 0x72, 0x80);
const HAIRLINE: Color = Color::from_rgb_u8(0xE5, 0xE7, 0xEB);
const PANEL: Color = Color::from_rgb_u8(0xE2, 0xE8, 0xF0);
const UP: Color = Color::from_rgb_u8(0x16, 0xA3, 0x4A);
const DOWN: Color = Color::from_rgb_u8(0xDC, 0x26, 0x26);
const LAYER_BACKGROUND: Color = Color::from_rgba_u8(0x1E, 0x29, 0x3B, 0xC0);
const NESTED_BACKGROUND: Color = Color::from_rgba_u8(0xFF, 0xFF, 0xFF, 0xE6);
const LEVEL_BACKGROUND: [Color; 2] = [
    Color::from_rgb_u8(0xF1, 0xF5, 0xF9),
    Color::from_rgb_u8(0xCB, 0xD5, 0xE1),
];
/// List rows: no measurement window reaches the end.
const ROWS: usize = 1_000_000;
/// How far the list scrolls each frame, in dp.
const SCROLL_PER_FRAME: f32 = 3.0;

/// What `am start` asked of the gauntlet.
#[derive(Clone, Copy, PartialEq)]
pub struct GauntletLoad {
    /// Load tier, 1 to 16.
    pub tier: usize,
    /// Stop on this frame and hold still, for picture comparisons; 0 runs on.
    pub freeze: u32,
}

/// Everything a card needs that does not change while the screen lives.
#[derive(Clone, PartialEq)]
struct Shared {
    posts: Rc<[Rc<Post>]>,
    avatars: Rc<[Option<ImageBitmap>]>,
    tickers: Rc<[Ticker]>,
    tier: GauntletTier,
    frame: MutableState<u32>,
}

#[composable]
pub fn GauntletScreen(load: GauntletLoad) {
    let tier = data::gauntlet_tier(load.tier);
    let frame = rememberMutableStateOf(|| 0u32);
    let list_state = rememberLazyListState();
    let shared = remember(move || Shared {
        posts: data::posts().into_iter().map(Rc::new).collect(),
        avatars: (0..AVATAR_COUNT)
            .map(|index| {
                ImageBitmap::from_rgba8(AVATAR_SIZE, AVATAR_SIZE, data::avatar_rgba(index))
                    .map_err(|error| log::warn!("avatar {index}: {error}"))
                    .ok()
            })
            .collect(),
        tickers: data::tickers(tier.tickers).into(),
        tier,
        frame,
    })
    .with(|shared| shared.clone());

    let freeze = load.freeze;
    LaunchedEffectAsync(freeze, move |scope| {
        std::boxed::Box::pin(async move {
            let clock = scope.runtime().frame_clock();
            let mut index = 0u32;
            while scope.is_active() {
                clock.next_frame().await;
                if !scope.is_active() {
                    break;
                }
                index += 1;
                frame.set(index);
                list_state.dispatch_scroll_delta(-SCROLL_PER_FRAME);
                if freeze > 0 && index >= freeze {
                    log::info!("PERF frozen frame={index}");
                    break;
                }
            }
        })
    });

    // Only this box reads the frame while composing; its content is one
    // call whose arguments never change, so nothing below it recomposes for
    // the width.
    Box(
        Modifier::empty()
            .fill_max_width_fraction(data::width_fraction(frame.get()))
            .weight(1.0),
        BoxSpec::default(),
        move || GauntletBody(shared.clone(), list_state),
    );
}

#[composable]
fn GauntletBody(shared: Shared, list_state: LazyListState) {
    Column(
        Modifier::empty().fill_max_size(),
        ColumnSpec::default(),
        move || {
            TickerPanel(shared.clone());
            let stack = shared.clone();
            Box(
                Modifier::empty().fill_max_width().weight(1.0),
                BoxSpec::default(),
                move || {
                    CardList(stack.clone(), list_state);
                    for layer in 0..stack.tier.layers {
                        StackedLayer(layer, stack.tier.layer_rows, stack.frame);
                    }
                },
            );
        },
    );
}

#[composable]
fn CardList(shared: Shared, list_state: LazyListState) {
    let s = shared.tier.scale;
    LazyColumn(
        Modifier::empty().fill_max_size(),
        list_state,
        LazyColumnSpec::new()
            .vertical_arrangement(LinearArrangement::spaced_by(8.0 * s))
            .content_padding_all(8.0 * s),
        move |scope| {
            let rows = shared.clone();
            let columns = rows.tier.columns;
            scope.items(
                LazyItems::new(ROWS)
                    .key(|index| index as u64)
                    .content_type(move |index| match data::gauntlet_row(index, columns) {
                        GauntletRow::Cards(_) => 0,
                        GauntletRow::Cluster(_) => 1,
                    }),
                move |row| match data::gauntlet_row(row, columns) {
                    GauntletRow::Cards(first) => CardRow(rows.clone(), first),
                    GauntletRow::Cluster(cluster) => {
                        DeepCluster(cluster, rows.tier.depth, rows.tier.scale)
                    }
                },
            );
        },
    );
}

#[composable]
fn TickerPanel(shared: Shared) {
    let s = shared.tier.scale;
    FlowRow(
        Modifier::empty()
            .fill_max_width()
            .background(PANEL)
            .padding(6.0 * s),
        FlowRowSpec::new()
            .main_axis_spacing(4.0 * s)
            .cross_axis_spacing(4.0 * s),
        move || {
            for index in 0..shared.tickers.len() {
                TickerTile(shared.clone(), index);
            }
        },
    );
}

#[composable]
fn TickerTile(shared: Shared, index: usize) {
    let s = shared.tier.scale;
    let frame = shared.frame;
    let tickers = shared.tickers.clone();
    Row(
        Modifier::empty()
            .background(Color::WHITE)
            .rounded_corners(6.0 * s)
            .padding_symmetric(6.0 * s, 3.0 * s),
        RowSpec::new()
            .horizontal_arrangement(LinearArrangement::spaced_by(4.0 * s))
            .vertical_alignment(VerticalAlignment::CenterVertically),
        move || {
            Text(
                tickers[index].symbol.clone(),
                Modifier::empty(),
                text_style(10.0 * s, INK, true),
            );
            TickerPrice(tickers.clone(), index, frame, s);
            TickerChange(tickers.clone(), index, frame, s);
            let fill = tickers.clone();
            Box(
                Modifier::empty()
                    .size_points(20.0 * s, 4.0 * s)
                    .background(HAIRLINE)
                    .rounded_corners(2.0 * s)
                    .draw_behind(move |scope| {
                        let ticker = &fill[index];
                        let cents = data::ticker_cents(ticker, frame.get());
                        let share = ((cents - ticker.base_cents + ticker.swing_cents) as f32
                            / (2 * ticker.swing_cents) as f32)
                            .clamp(0.0, 1.0);
                        let size = scope.size();
                        scope.draw_round_rect_at(
                            Rect {
                                x: 0.0,
                                y: 0.0,
                                width: size.width * share,
                                height: size.height,
                            },
                            Brush::solid(PALETTE[index % PALETTE.len()]),
                            CornerRadii::uniform(2.0 * s),
                        );
                    }),
                BoxSpec::default(),
                || {},
            );
        },
    );
}

/// The price on this frame: its own scope, so the frame recomposes only it.
#[composable]
fn TickerPrice(tickers: Rc<[Ticker]>, index: usize, frame: MutableState<u32>, s: f32) {
    Text(
        data::cents_text(data::ticker_cents(&tickers[index], frame.get())),
        Modifier::empty(),
        text_style(10.0 * s, BODY, false),
    );
}

/// The signed change, green or red, in its own scope like the price.
#[composable]
fn TickerChange(tickers: Rc<[Ticker]>, index: usize, frame: MutableState<u32>, s: f32) {
    let ticker = &tickers[index];
    let cents = data::ticker_cents(ticker, frame.get());
    Text(
        data::change_text(ticker, cents),
        Modifier::empty(),
        text_style(
            10.0 * s,
            if cents >= ticker.base_cents { UP } else { DOWN },
            false,
        ),
    );
}

#[composable]
fn CardRow(shared: Shared, first: usize) {
    let s = shared.tier.scale;
    // Cranpose's list pads only along its scroll axis: the rows carry the
    // side padding Compose's grid takes as content padding.
    Row(
        Modifier::empty()
            .fill_max_width()
            .padding_horizontal(8.0 * s),
        RowSpec::new().horizontal_arrangement(LinearArrangement::spaced_by(8.0 * s)),
        move || {
            for card in first..first + shared.tier.columns {
                Card(shared.clone(), card);
            }
        },
    );
}

#[composable]
fn Card(shared: Shared, card: usize) {
    let s = shared.tier.scale;
    let post = shared.posts[card % shared.posts.len()].clone();
    let shape = RoundedCornerShape::uniform(12.0 * s);
    Box(
        Modifier::empty().weight(1.0),
        BoxSpec::default(),
        move || {
            let post = post.clone();
            let shared = shared.clone();
            Column(
                Modifier::empty()
                    .fill_max_width()
                    .shadow_with(
                        3.0 * s,
                        LayerShape::Rounded(shape),
                        true,
                        Color::BLACK,
                        Color::BLACK,
                    )
                    .background(Color::WHITE)
                    .rounded_corners(12.0 * s)
                    .border(1.0, HAIRLINE, shape)
                    .padding(10.0 * s),
                ColumnSpec::new().vertical_arrangement(LinearArrangement::spaced_by(6.0 * s)),
                move || {
                    CardHeader(post.clone(), shared.avatars[card % AVATAR_COUNT].clone(), s);
                    TextWithOptions(
                        post.body.clone(),
                        Modifier::empty(),
                        text_style(12.0 * s, BODY, false),
                        clamped_text_options(4),
                    );
                    Progress(card, shared.frame, s);
                    Sparkline(card, post.color, shared.frame, s);
                    Chips(post.clone(), s);
                    Footer(post.clone(), s);
                },
            );
            if card.is_multiple_of(5) {
                Badge(card, shared.frame, s);
            }
        },
    );
}

#[composable]
fn CardHeader(post: Rc<Post>, avatar: Option<ImageBitmap>, s: f32) {
    let subtitle = rememberKeyed(post.id, |_| subtitle(&post));
    Row(
        Modifier::empty().fill_max_width(),
        RowSpec::new()
            .horizontal_arrangement(LinearArrangement::spaced_by(8.0 * s))
            .vertical_alignment(VerticalAlignment::CenterVertically),
        move || {
            let circle = Modifier::empty()
                .size_points(32.0 * s, 32.0 * s)
                .graphics_layer_value(GraphicsLayer {
                    shape: LayerShape::Rounded(RoundedCornerShape::uniform(16.0 * s)),
                    clip: true,
                    ..Default::default()
                });
            match avatar.clone() {
                Some(bitmap) => {
                    Image(
                        bitmap,
                        None,
                        circle,
                        Alignment::CENTER,
                        ContentScale::Crop,
                        1.0,
                        None,
                    );
                }
                None => {
                    Box(
                        circle.background(PALETTE[post.color]),
                        BoxSpec::default(),
                        || {},
                    );
                }
            }
            let post = post.clone();
            let subtitle = subtitle.clone();
            Column(
                Modifier::empty().weight(1.0),
                ColumnSpec::default(),
                move || {
                    TextWithOptions(
                        post.title.clone(),
                        Modifier::empty(),
                        text_style(13.0 * s, INK, true),
                        clamped_text_options(2),
                    );
                    TextWithOptions(
                        subtitle.clone(),
                        Modifier::empty(),
                        text_style(11.0 * s, MUTED, false),
                        clamped_text_options(1),
                    );
                },
            );
        },
    );
}

/// `by Ada Lovelace · #rust`: the author bold, the tag in its color.
fn subtitle(post: &Post) -> Rc<AnnotatedString> {
    let (tag, color) = &post.tags[0];
    Rc::new(
        AnnotatedString::builder()
            .append("by ")
            .push_style(SpanStyle {
                font_weight: Some(FontWeight::BOLD),
                ..Default::default()
            })
            .append(&post.author)
            .pop()
            .append(" · ")
            .push_style(SpanStyle {
                color: Some(GRADIENT_END[*color]),
                ..Default::default()
            })
            .append(tag)
            .pop()
            .to_annotated_string(),
    )
}

#[composable]
fn Progress(card: usize, frame: MutableState<u32>, s: f32) {
    let color = PALETTE[card % PALETTE.len()];
    Row(
        Modifier::empty().fill_max_width(),
        RowSpec::new()
            .horizontal_arrangement(LinearArrangement::spaced_by(6.0 * s))
            .vertical_alignment(VerticalAlignment::CenterVertically),
        move || {
            Box(
                Modifier::empty()
                    .weight(1.0)
                    .height(6.0 * s)
                    .background(HAIRLINE)
                    .rounded_corners(3.0 * s)
                    .draw_behind(move |scope| {
                        let share = data::progress_permille(card, frame.get()) as f32 / 1000.0;
                        let size = scope.size();
                        scope.draw_round_rect_at(
                            Rect {
                                x: 0.0,
                                y: 0.0,
                                width: size.width * share,
                                height: size.height,
                            },
                            Brush::solid(color),
                            CornerRadii::uniform(3.0 * s),
                        );
                    }),
                BoxSpec::default(),
                || {},
            );
            ProgressLabel(card, frame, s);
        },
    );
}

/// The percent beside the bar, in its own scope like the ticker's labels.
#[composable]
fn ProgressLabel(card: usize, frame: MutableState<u32>, s: f32) {
    Text(
        format!("{}%", data::progress_permille(card, frame.get()) / 10),
        Modifier::empty(),
        text_style(10.0 * s, MUTED, false),
    );
}

#[composable]
fn Sparkline(card: usize, color: usize, frame: MutableState<u32>, s: f32) {
    let line = PALETTE[color];
    let fill = Brush::vertical_gradient(
        vec![line.with_alpha(0.25), line.with_alpha(0.0)],
        0.0,
        36.0 * s,
    );
    Canvas(
        Modifier::empty().fill_max_width().height(36.0 * s),
        move |scope| {
            let size = scope.size();
            let step = size.width / (GAUNTLET_SPARK_POINTS - 1) as f32;
            let k = frame.get();
            let point = |index: usize| Point {
                x: index as f32 * step,
                y: size.height * (1.0 - data::spark_value(card, index, k)),
            };
            let mut area = Path::new();
            area.move_to(Point {
                x: 0.0,
                y: size.height,
            });
            for index in 0..GAUNTLET_SPARK_POINTS {
                area.line_to(point(index));
            }
            area.line_to(Point {
                x: size.width,
                y: size.height,
            });
            area.close();
            scope.draw_path(&area, fill.clone(), DrawStyle::Fill);
            let mut stroke = Path::new();
            stroke.move_to(point(0));
            for index in 1..GAUNTLET_SPARK_POINTS {
                stroke.line_to(point(index));
            }
            scope.draw_path(
                &stroke,
                Brush::solid(line),
                DrawStyle::Stroke(Stroke::new(1.5 * s)),
            );
        },
    );
}

#[composable]
fn Chips(post: Rc<Post>, s: f32) {
    FlowRow(
        Modifier::empty(),
        FlowRowSpec::new()
            .main_axis_spacing(4.0 * s)
            .cross_axis_spacing(4.0 * s),
        move || {
            for (tag, color) in &post.tags {
                Text(
                    tag.clone(),
                    Modifier::empty()
                        .background(CHIP_BACKGROUND[*color])
                        .rounded_corners(10.0 * s)
                        .padding_symmetric(8.0 * s, 3.0 * s),
                    text_style(10.0 * s, GRADIENT_END[*color], false),
                );
            }
        },
    );
}

/// Three counters split by dividers as tall as the tallest counter: an
/// intrinsic measurement on every pass.
#[composable]
fn Footer(post: Rc<Post>, s: f32) {
    Row(
        Modifier::empty()
            .fill_max_width()
            .height_intrinsic(IntrinsicSize::Min),
        RowSpec::default(),
        move || {
            for (index, label) in ["likes", "replies", "shares"].into_iter().enumerate() {
                if index > 0 {
                    Box(
                        Modifier::empty()
                            .width(1.0)
                            .fill_max_height()
                            .background(HAIRLINE),
                        BoxSpec::default(),
                        || {},
                    );
                }
                let value = post.stats[index].clone();
                Column(
                    Modifier::empty().weight(1.0),
                    ColumnSpec::new().horizontal_alignment(HorizontalAlignment::CenterHorizontally),
                    move || {
                        Text(
                            value.clone(),
                            Modifier::empty(),
                            text_style(12.0 * s, INK, true),
                        );
                        Text(label, Modifier::empty(), text_style(9.0 * s, MUTED, false));
                    },
                );
            }
        },
    );
}

/// A translucent tag over every fifth card, tilting with the frame in its
/// graphics layer: drawn again each frame, never measured again.
#[composable]
fn Badge(card: usize, frame: MutableState<u32>, s: f32) {
    Text(
        "HOT",
        Modifier::empty()
            .align(Alignment::TOP_END)
            .padding(6.0 * s)
            .graphics_layer(move || GraphicsLayer {
                rotation_z: data::badge_degrees(card, frame.get()),
                alpha: 0.9,
                ..Default::default()
            })
            .background(PALETTE[0])
            .rounded_corners(8.0 * s)
            .padding_symmetric(6.0 * s, 2.0 * s),
        text_style(9.0 * s, Color::WHITE, true),
    );
}

/// `depth` levels nested inside one another, each a row of three labels above
/// the next level.
#[composable]
fn DeepCluster(cluster: usize, depth: usize, s: f32) {
    Box(
        Modifier::empty()
            .fill_max_width()
            .padding_horizontal(8.0 * s),
        BoxSpec::default(),
        move || Level(cluster, depth, s),
    );
}

#[composable]
fn Level(cluster: usize, remaining: usize, s: f32) {
    Column(
        Modifier::empty()
            .fill_max_width()
            .background(LEVEL_BACKGROUND[remaining % 2])
            .rounded_corners(4.0 * s)
            .padding_each(3.0 * s, 1.0 * s, 1.0 * s, 1.0 * s),
        ColumnSpec::new().vertical_arrangement(LinearArrangement::spaced_by(1.0 * s)),
        move || {
            Row(
                Modifier::empty().fill_max_width(),
                RowSpec::new().horizontal_arrangement(LinearArrangement::spaced_by(2.0 * s)),
                move || {
                    for chip in 0..3 {
                        Text(
                            format!("C{cluster}.L{remaining}.{chip}"),
                            Modifier::empty()
                                .weight(1.0)
                                .background(PALETTE[(remaining + chip + cluster) % PALETTE.len()])
                                .rounded_corners(2.0 * s),
                            text_style(9.0 * s, Color::WHITE, false),
                        );
                    }
                },
            );
            if remaining > 0 {
                Level(cluster, remaining - 1, s);
            }
        },
    );
}

/// A translucent panel stacked over the list. Its content never changes:
/// each frame its graphics layer moves and tilts it, and its nested card's
/// tilts the card back the other way, so nothing is composed or measured
/// again.
#[composable]
fn StackedLayer(layer: usize, rows: usize, frame: MutableState<u32>) {
    Column(
        Modifier::empty()
            .width(220.0)
            .graphics_layer(move || {
                let frame = frame.get();
                GraphicsLayer {
                    translation_x: data::layer_x(layer, frame),
                    translation_y: data::layer_y(layer, frame),
                    rotation_z: data::layer_degrees(layer, frame),
                    ..Default::default()
                }
            })
            .background(LAYER_BACKGROUND)
            .rounded_corners(12.0)
            .padding(10.0),
        ColumnSpec::new().vertical_arrangement(LinearArrangement::spaced_by(8.0)),
        move || {
            Text(
                format!("Layer {}", layer + 1),
                Modifier::empty(),
                text_style(13.0, Color::WHITE, true),
            );
            Column(
                Modifier::empty(),
                ColumnSpec::new().vertical_arrangement(LinearArrangement::spaced_by(3.0)),
                move || {
                    for row in 0..rows {
                        LayerCells(layer, row);
                    }
                },
            );
            NestedCard(layer, frame);
        },
    );
}

/// One row of a stacked panel's numbered cells.
#[composable]
fn LayerCells(layer: usize, row: usize) {
    Row(
        Modifier::empty(),
        RowSpec::new().horizontal_arrangement(LinearArrangement::spaced_by(3.0)),
        move || {
            for cell in row * LAYER_COLUMNS..(row + 1) * LAYER_COLUMNS {
                Box(
                    Modifier::empty()
                        .size_points(22.0, 22.0)
                        .background(PALETTE[data::layer_cell_color(layer, cell)])
                        .rounded_corners(4.0),
                    BoxSpec::new().content_alignment(Alignment::CENTER),
                    move || {
                        Text(
                            (cell + 1).to_string(),
                            Modifier::empty(),
                            text_style(9.0, Color::WHITE, false),
                        );
                    },
                );
            }
        },
    );
}

#[composable]
fn NestedCard(layer: usize, frame: MutableState<u32>) {
    Column(
        Modifier::empty()
            .fill_max_width()
            .graphics_layer(move || GraphicsLayer {
                rotation_z: -data::layer_degrees(layer, frame.get()),
                ..Default::default()
            })
            .background(NESTED_BACKGROUND)
            .rounded_corners(8.0)
            .padding(8.0),
        ColumnSpec::new().vertical_arrangement(LinearArrangement::spaced_by(2.0)),
        move || {
            Text(
                format!("Nested in layer {}", layer + 1),
                Modifier::empty(),
                text_style(11.0, INK, true),
            );
            Text(
                "Tilts against its panel",
                Modifier::empty(),
                text_style(11.0, BODY, false),
            );
        },
    );
}
