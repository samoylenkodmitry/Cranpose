//! The gauntlet in Xilem: the screen `compose-app/.../Gauntlet.kt` draws,
//! element for element, in Xilem's own idiom: the app logic builds a tree of
//! views from the app's state, Xilem compares it with the last tree and
//! changes Masonry's widgets only where a view differs, and Vello draws them
//! on wgpu. The benchmark's README describes it; `desktop.py` runs it:
//! `PERF_TIER=16 PERF_FONTS=../fonts cargo run --release`.
//!
//! Every frame advances a frame index and everything follows from it, never
//! from wall time. A widget of the app's own asks Masonry for every
//! animation frame and hands each to the app logic, which advances the frame
//! and builds the tree again; the parts that never change are memoized, so
//! Xilem skips them. The list builds the rows from the first one on screen
//! down to the window's bottom, and each row reports its height once Masonry
//! has laid it out. Xilem has no wrapping row, no box that clips, scrolls
//! or turns about its centre, and no canvas: `widgets` adds them.

mod widgets;

use std::collections::HashMap;

use perf_data::*;
use widgets::{Hold, Picture, drawing, flow, frame_clock, hold, ignore_height};
use xilem::{
    AnyWidgetView, Blob, Color, EventLoop, FontWeight, Vec2, WidgetView, WindowOptions, Xilem,
    core::{memoize, one_of::Either},
    masonry::properties::{
        LineBreaking,
        types::{Length, UnitPoint},
    },
    style::{Padding, Style as _},
    view::{
        CrossAxisAlignment, FlexExt as _, Label, ZStackExt as _, flex_col, flex_row, label,
        sized_box, zstack,
    },
    winit::dpi::LogicalSize,
};

const FOOTER_LABELS: [&str; 3] = ["likes", "replies", "shares"];
const INK: Color = rgb([0x11, 0x18, 0x27]);
const BODY: Color = rgb([0x37, 0x41, 0x51]);
const MUTED: Color = rgb([0x6B, 0x72, 0x80]);
const HAIRLINE: Color = rgb([0xE5, 0xE7, 0xEB]);
const PANEL: Color = rgb([0xE2, 0xE8, 0xF0]);
const BACKGROUND: Color = rgb([0xEE, 0xF0, 0xF5]);
const TOP_BAR: Color = rgb([0x1E, 0x2A, 0x4A]);
const UP: Color = rgb([0x16, 0xA3, 0x4A]);
const DOWN: Color = rgb([0xDC, 0x26, 0x26]);
const LAYER_BACKGROUND: Color = Color::from_rgba8(0x1E, 0x29, 0x3B, 0xC0);
const NESTED_BACKGROUND: Color = Color::from_rgba8(0xFF, 0xFF, 0xFF, 0xE6);
/// Lines of text 1.4 em apart, as the other apps set them; Xilem's labels
/// keep their font's own line height, which a clamped paragraph is cut by.
const LINE: f64 = 1.4;

const fn rgb(value: [u8; 3]) -> Color {
    Color::from_rgb8(value[0], value[1], value[2])
}

fn palette(index: usize) -> Color {
    rgb(PALETTE_RGB[index % PALETTE_RGB.len()])
}

fn px(value: f32) -> Length {
    Length::px(f64::from(value))
}

/// A label in Roboto at `size`.
fn styled<T: Into<String>>(content: T, size: f32, bold: bool) -> Label {
    label(content.into())
        .text_size(size)
        .font("Roboto")
        .weight(if bold {
            FontWeight::BOLD
        } else {
            FontWeight::NORMAL
        })
}

/// One line of text in Roboto at `size`.
fn text<T: Into<String>>(
    content: T,
    size: f32,
    color: Color,
    bold: bool,
) -> impl WidgetView<App> + use<T> {
    styled(content, size, bold).color(color)
}

/// Wrapped text cut after `lines` lines.
fn paragraph<T: Into<String>>(
    content: T,
    size: f32,
    color: Color,
    bold: bool,
    lines: usize,
) -> impl WidgetView<App> + use<T> {
    hold(
        styled(content, size, bold)
            .line_break_mode(LineBreaking::WordWrap)
            .color(color),
        Hold::Clamp {
            max: f64::from(size) * LINE * lines as f64,
        },
        ignore_height,
    )
}

/// What the screen reads, which each frame advances.
struct App {
    frame: u32,
    frozen: bool,
    /// Where the list starts.
    anchor: ListAnchor,
    /// The heights rows reported, from the anchor down.
    heights: HashMap<usize, f32>,
}

impl App {
    /// Advances the frame unless frozen; answers whether it did.
    fn advance(&mut self) -> bool {
        if self.frozen {
            return false;
        }
        self.frame += 1;
        if self.frame == 1 {
            log::info!("PERF first_frame");
        }
        let gap = 8.0 * desktop_gauntlet().tier.scale;
        let heights = &self.heights;
        let anchor = self
            .anchor
            .advanced(self.frame as f32 * SCROLL_PER_FRAME, gap, |row| {
                heights.get(&row).copied()
            });
        if anchor != self.anchor {
            self.anchor = anchor;
            self.heights.retain(|measured, _| *measured >= anchor.row);
        }
        let freeze = desktop_gauntlet().launch.freeze;
        if freeze > 0 && self.frame >= freeze {
            self.frozen = true;
            log::info!("PERF frozen frame={}", self.frame);
        }
        true
    }
}

fn app_logic(app: &mut App) -> impl WidgetView<App> + use<> {
    let tier = desktop_gauntlet().tier;
    let (width, _) = DESKTOP_WINDOW;
    let content = flex_col((
        ticker_panel(app.frame),
        // The panels stack over the list and show only over it, as every
        // app clips them.
        hold(
            zstack((
                list(app),
                (0..tier.layers)
                    .map(|layer| stacked_layer(layer, app.frame).alignment(UnitPoint::TOP_LEFT))
                    .collect::<Vec<_>>(),
            ))
            .alignment(UnitPoint::TOP_LEFT),
            Hold::Clip,
            ignore_height,
        )
        .flex(1.0),
    ))
    .gap(px(0.0))
    .cross_axis_alignment(CrossAxisAlignment::Fill);
    flex_col((
        frame_clock(App::advance),
        sized_box(text("Gauntlet", 20.0, Color::WHITE, true))
            .expand_width()
            .height(px(56.0))
            .padding(Padding::horizontal(16.0))
            .background_color(TOP_BAR),
        sized_box(content)
            .width(px((width as f32 * width_fraction(app.frame)).round()))
            .flex(1.0),
    ))
    .gap(px(0.0))
    .cross_axis_alignment(CrossAxisAlignment::Start)
    .background_color(BACKGROUND)
}

fn ticker_panel(frame: u32) -> impl WidgetView<App> + use<> {
    let s = desktop_gauntlet().tier.scale;
    let tiles = desktop_gauntlet()
        .tickers
        .iter()
        .enumerate()
        .map(|(index, ticker)| ticker_tile(index, ticker, frame))
        .collect::<Vec<_>>();
    sized_box(flow(tiles, f64::from(4.0 * s)))
        .expand_width()
        .padding(f64::from(6.0 * s))
        .background_color(PANEL)
}

/// One quote: symbol, price, change and a bar, all from the frame.
fn ticker_tile(index: usize, ticker: &Ticker, frame: u32) -> impl WidgetView<App> + use<> {
    let s = desktop_gauntlet().tier.scale;
    let cents = ticker_cents(ticker, frame);
    let share =
        (cents - ticker.base_cents + ticker.swing_cents) as f32 / (2 * ticker.swing_cents) as f32;
    sized_box(
        flex_row((
            text(ticker.symbol.clone(), 10.0 * s, INK, true),
            text(cents_text(cents), 10.0 * s, BODY, false),
            text(
                change_text(ticker, cents),
                10.0 * s,
                if cents >= ticker.base_cents { UP } else { DOWN },
                false,
            ),
            drawing(Picture::Bar {
                width: Some(f64::from(20.0 * s)),
                height: f64::from(4.0 * s),
                share: f64::from(share),
                track: HAIRLINE,
                fill: palette(index),
            }),
        ))
        .gap(px(4.0 * s))
        .cross_axis_alignment(CrossAxisAlignment::Center),
    )
    .padding(Padding {
        top: f64::from(3.0 * s),
        bottom: f64::from(3.0 * s),
        left: f64::from(6.0 * s),
        right: f64::from(6.0 * s),
    })
    .corner_radius(f64::from(6.0 * s))
    .background_color(Color::WHITE)
}

/// The rows from the anchor down to the bottom of the window, moved up by
/// the frame's offset.
fn list(app: &App) -> impl WidgetView<App> + use<> {
    let tier = desktop_gauntlet().tier;
    let s = tier.scale;
    let gap = 8.0 * s;
    let offset = app.frame as f32 * SCROLL_PER_FRAME;
    let (_, window_height) = DESKTOP_WINDOW;
    let rows = app
        .anchor
        .rows(offset, window_height as f32, gap, 10.0 * s, |row| {
            app.heights.get(&row).copied()
        })
        .map(|row| {
            let content = match gauntlet_row(row, tier.columns) {
                GauntletRow::Cards(first) => Either::A(
                    flex_row(
                        (first..first + tier.columns)
                            .map(|card| card_view(card, app.frame).flex(1.0))
                            .collect::<Vec<_>>(),
                    )
                    .gap(px(gap))
                    .cross_axis_alignment(CrossAxisAlignment::Start),
                ),
                GauntletRow::Cluster(cluster) => Either::B(memoize(
                    (cluster, tier.depth, s.to_bits()),
                    |&(cluster, depth, s)| level(cluster, depth, f32::from_bits(s)),
                )),
            };
            hold(content, Hold::Measure, move |app: &mut App, height| {
                app.heights.insert(row, height as f32);
            })
        })
        .collect::<Vec<_>>();
    hold(
        sized_box(
            flex_col(rows)
                .gap(px(gap))
                .cross_axis_alignment(CrossAxisAlignment::Fill),
        )
        .padding(Padding::horizontal(f64::from(gap))),
        Hold::Viewport {
            shift: f64::from(app.anchor.top - offset),
        },
        ignore_height,
    )
}

fn card_view(card: usize, frame: u32) -> impl WidgetView<App> + use<> {
    let s = desktop_gauntlet().tier.scale;
    let post = &desktop_gauntlet().posts[card % POST_COUNT];
    let body = sized_box(
        flex_col((
            memoize((card, s.to_bits()), |&(card, s)| {
                card_header(card, f32::from_bits(s))
            }),
            progress(card, frame),
            drawing(Picture::Sparkline {
                card,
                frame,
                color: palette(post.color),
                height: f64::from(36.0 * s),
                stroke: f64::from(1.5 * s),
            }),
            memoize((card, s.to_bits()), |&(card, s)| {
                card_footer(card, f32::from_bits(s))
            }),
        ))
        .gap(px(6.0 * s))
        .cross_axis_alignment(CrossAxisAlignment::Fill),
    )
    .expand_width()
    .padding(f64::from(10.0 * s))
    .corner_radius(f64::from(12.0 * s))
    .border(HAIRLINE, 1.0)
    .background_color(Color::WHITE);
    // Only Masonry's buttons and text inputs cast shadows: the card's box
    // holder paints its own.
    let body = hold(
        body,
        Hold::Shadow {
            radius: f64::from(12.0 * s),
            offset_y: f64::from(s),
            blur: f64::from(3.0 * s),
            color: Color::BLACK.with_alpha(0.24),
        },
        ignore_height,
    );
    let badge = card.is_multiple_of(5).then(|| {
        // A translucent tag tilting with the frame: turned, never laid out
        // again.
        sized_box(hold(
            sized_box(text("HOT", 9.0 * s, Color::WHITE.with_alpha(0.9), true))
                .padding(Padding {
                    top: f64::from(2.0 * s),
                    bottom: f64::from(2.0 * s),
                    left: f64::from(6.0 * s),
                    right: f64::from(6.0 * s),
                })
                .corner_radius(f64::from(8.0 * s))
                .background_color(palette(0).with_alpha(0.9)),
            Hold::Turn {
                offset: Vec2::ZERO,
                radians: f64::from(badge_degrees(card, frame)).to_radians(),
            },
            ignore_height,
        ))
        .padding(f64::from(6.0 * s))
        .alignment(UnitPoint::TOP_RIGHT)
    });
    zstack((body, badge)).alignment(UnitPoint::TOP_LEFT)
}

/// The avatar, the title and the subtitle, then the body text: what a card
/// never changes.
fn card_header(card: usize, s: f32) -> impl WidgetView<App> + use<> {
    let post = &desktop_gauntlet().posts[card % POST_COUNT];
    let (tag, tag_color) = &post.tags[0];
    // `by Ada Lovelace · #rust`: the author bold, the tag in its color.
    let subtitle = flex_row((
        text("by ", 11.0 * s, MUTED, false),
        text(post.author.clone(), 11.0 * s, MUTED, true),
        text(" · ", 11.0 * s, MUTED, false),
        text(
            tag.clone(),
            11.0 * s,
            rgb(GRADIENT_END_RGB[*tag_color]),
            false,
        ),
    ))
    .gap(px(0.0));
    let chips = post
        .tags
        .iter()
        .map(|(value, color)| {
            sized_box(text(
                value.clone(),
                10.0 * s,
                rgb(GRADIENT_END_RGB[*color]),
                false,
            ))
            .padding(Padding {
                top: f64::from(3.0 * s),
                bottom: f64::from(3.0 * s),
                left: f64::from(8.0 * s),
                right: f64::from(8.0 * s),
            })
            .corner_radius(f64::from(10.0 * s))
            .background_color(rgb(CHIP_BACKGROUND_RGB[*color]))
        })
        .collect::<Vec<_>>();
    flex_col((
        flex_row((
            drawing(Picture::Avatar {
                index: card % AVATAR_COUNT,
                side: f64::from(32.0 * s),
            }),
            flex_col((
                paragraph(post.title.clone(), 13.0 * s, INK, true, 2),
                subtitle,
            ))
            .gap(px(0.0))
            .cross_axis_alignment(CrossAxisAlignment::Start)
            .flex(1.0),
        ))
        .gap(px(8.0 * s))
        .cross_axis_alignment(CrossAxisAlignment::Center),
        paragraph(post.body.clone(), 12.0 * s, BODY, false, 4),
        flow(chips, f64::from(4.0 * s)),
    ))
    .gap(px(6.0 * s))
    .cross_axis_alignment(CrossAxisAlignment::Fill)
}

fn progress(card: usize, frame: u32) -> impl WidgetView<App> + use<> {
    let s = desktop_gauntlet().tier.scale;
    let permille = progress_permille(card, frame);
    flex_row((
        drawing(Picture::Bar {
            width: None,
            height: f64::from(6.0 * s),
            share: f64::from(permille) / 1000.0,
            track: HAIRLINE,
            fill: palette(card),
        })
        .flex(1.0),
        text(format!("{}%", permille / 10), 10.0 * s, MUTED, false),
    ))
    .gap(px(6.0 * s))
    .cross_axis_alignment(CrossAxisAlignment::Center)
}

/// Three counters split by dividers as tall as the tallest counter.
fn card_footer(card: usize, s: f32) -> impl WidgetView<App> + use<> {
    let post = &desktop_gauntlet().posts[card % POST_COUNT];
    let counter = |index: usize| {
        flex_col((
            text(post.stats[index].clone(), 12.0 * s, INK, true),
            text(FOOTER_LABELS[index], 9.0 * s, MUTED, false),
        ))
        .gap(px(0.0))
        .cross_axis_alignment(CrossAxisAlignment::Center)
        .flex(1.0)
    };
    let divider = || {
        sized_box(label(""))
            .width(px(1.0))
            .background_color(HAIRLINE)
    };
    flex_row((counter(0), divider(), counter(1), divider(), counter(2)))
        .gap(px(0.0))
        .cross_axis_alignment(CrossAxisAlignment::Fill)
}

/// Levels nested inside one another, each a row of three labels above the
/// next level. A level holds the next, so the views are boxed.
fn level(cluster: usize, remaining: usize, s: f32) -> Box<AnyWidgetView<App>> {
    let chips = (0..3)
        .map(|chip| {
            sized_box(text(
                format!("C{cluster}.L{remaining}.{chip}"),
                9.0 * s,
                Color::WHITE,
                false,
            ))
            .expand_width()
            .corner_radius(f64::from(2.0 * s))
            .background_color(palette(remaining + chip + cluster))
            .flex(1.0)
        })
        .collect::<Vec<_>>();
    let inner = (remaining > 0).then(|| level(cluster, remaining - 1, s));
    Box::new(
        sized_box(
            flex_col((
                flex_row(chips)
                    .gap(px(2.0 * s))
                    .cross_axis_alignment(CrossAxisAlignment::Start),
                inner,
            ))
            .gap(px(s))
            .cross_axis_alignment(CrossAxisAlignment::Fill),
        )
        .expand_width()
        .padding(Padding {
            top: f64::from(s),
            bottom: f64::from(s),
            left: f64::from(3.0 * s),
            right: f64::from(s),
        })
        .corner_radius(f64::from(4.0 * s))
        .background_color(rgb(LEVEL_BACKGROUND_RGB[remaining % 2])),
    )
}

/// A translucent panel stacked over the list. Its content never changes and
/// is memoized: each frame turns and moves its box, and turns its nested
/// card back, through the boxes' transforms alone.
fn stacked_layer(layer: usize, frame: u32) -> impl WidgetView<App> + use<> {
    let radians = f64::from(layer_degrees(layer, frame)).to_radians();
    let rows = desktop_gauntlet().tier.layer_rows;
    hold(
        sized_box(
            flex_col((
                memoize((layer, rows), |&(layer, rows)| layer_cells(layer, rows)),
                hold(
                    sized_box(
                        flex_col((
                            text(format!("Nested in layer {}", layer + 1), 11.0, INK, true),
                            text("Tilts against its panel", 11.0, BODY, false),
                        ))
                        .gap(px(2.0))
                        .cross_axis_alignment(CrossAxisAlignment::Start),
                    )
                    .expand_width()
                    .padding(8.0)
                    .corner_radius(8.0)
                    .background_color(NESTED_BACKGROUND),
                    Hold::Turn {
                        offset: Vec2::ZERO,
                        radians: -radians,
                    },
                    ignore_height,
                ),
            ))
            .gap(px(8.0))
            .cross_axis_alignment(CrossAxisAlignment::Fill),
        )
        .width(px(220.0))
        .padding(10.0)
        .corner_radius(12.0)
        .background_color(LAYER_BACKGROUND),
        Hold::Turn {
            offset: Vec2::new(
                f64::from(layer_x(layer, frame)),
                f64::from(layer_y(layer, frame)),
            ),
            radians,
        },
        ignore_height,
    )
}

/// A stacked panel's title and rows of numbered cells.
fn layer_cells(layer: usize, rows: usize) -> impl WidgetView<App> + use<> {
    let lines = (0..rows)
        .map(|row| {
            flex_row(
                (row * LAYER_COLUMNS..(row + 1) * LAYER_COLUMNS)
                    .map(|cell| {
                        sized_box(text((cell + 1).to_string(), 9.0, Color::WHITE, false))
                            .width(px(22.0))
                            .height(px(22.0))
                            .corner_radius(4.0)
                            .background_color(palette(layer_cell_color(layer, cell)))
                    })
                    .collect::<Vec<_>>(),
            )
            .gap(px(3.0))
        })
        .collect::<Vec<_>>();
    flex_col((
        text(format!("Layer {}", layer + 1), 13.0, Color::WHITE, true),
        flex_col(lines)
            .gap(px(3.0))
            .cross_axis_alignment(CrossAxisAlignment::Start),
    ))
    .gap(px(8.0))
    .cross_axis_alignment(CrossAxisAlignment::Start)
}

fn main() {
    perf_data::log_to_stdout();
    let s = desktop_gauntlet().tier.scale;
    let app = App {
        frame: 0,
        frozen: false,
        anchor: ListAnchor {
            row: 0,
            top: 8.0 * s,
        },
        heights: HashMap::new(),
    };
    let (width, height) = DESKTOP_WINDOW;
    let window = WindowOptions::new("Gauntlet")
        .with_initial_inner_size(LogicalSize::new(f64::from(width), f64::from(height)))
        .with_resizable(false);
    let mut xilem = Xilem::new_simple(app, app_logic, window);
    for file in ["Roboto-Regular.ttf", "Roboto-Bold.ttf"] {
        match std::fs::read(font_path(file)) {
            Ok(bytes) => xilem = xilem.with_font(Blob::from(bytes)),
            Err(error) => log::warn!("no {file}: {error}"),
        }
    }
    if let Err(error) = xilem.run_in(EventLoop::with_user_event()) {
        log::error!("the event loop failed: {error}");
    }
}
