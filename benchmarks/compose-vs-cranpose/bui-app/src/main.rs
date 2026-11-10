//! The gauntlet in BUI: the screen `compose-app/.../Gauntlet.kt` draws, in
//! BUI's own idiom: a view that builds the tree of HTML-like elements from
//! the frame, which BUI lays out with taffy and rasterizes on the CPU. The
//! benchmark's README describes it; `desktop.py` runs it:
//! `PERF_TIER=16 PERF_FONTS=../fonts cargo run --release`.
//!
//! Every frame advances a frame index and everything follows from it, never
//! from wall time. The list builds only the rows from the first one on screen
//! to the window's bottom; each row's height is read from the last layout.

mod chat;
mod patchwork;
mod wall;

use std::{collections::HashMap, sync::Arc};

use blenderui::prelude::*;
use perf_data::*;

pub(crate) const REGULAR: FontWeight = FontWeight::NORMAL;
pub(crate) const BOLD: FontWeight = FontWeight::BOLD;
const FOOTER_LABELS: [&str; 3] = ["likes", "replies", "shares"];

/// An sRGB color.
pub(crate) fn color(value: [u8; 3]) -> Rgba {
    rgb((u32::from(value[0]) << 16) | (u32::from(value[1]) << 8) | u32::from(value[2]))
}

fn palette(index: usize) -> Rgba {
    color(PALETTE_RGB[index % PALETTE_RGB.len()])
}

fn ink() -> Rgba {
    rgb(0x111827)
}
fn body_color() -> Rgba {
    rgb(0x374151)
}
fn muted() -> Rgba {
    rgb(0x6B7280)
}
fn hairline() -> Rgba {
    rgb(0xE5E7EB)
}

/// A line of text in Roboto at `size`, lines 1.4 em apart.
pub(crate) fn text(content: impl Into<String>, size: f32, weight: FontWeight, color: Rgba) -> Div {
    div()
        .whitespace_nowrap()
        .text_size(px(size))
        .line_height(px(size * 1.4))
        .font_weight(weight)
        .text_color(color)
        .child(content.into())
}

/// A line of text at its own `left` and `top` in a box that sizes it.
pub(crate) fn line(
    content: impl Into<String>,
    size: f32,
    weight: FontWeight,
    color: Rgba,
    left: f32,
    top: f32,
) -> impl IntoElement {
    text(content, size, weight, color)
        .absolute()
        .left(px(left))
        .top(px(top))
}

/// Wrapped text cut after `lines` lines: a box as tall as the lines.
fn paragraph(
    content: impl Into<String>,
    size: f32,
    weight: FontWeight,
    color: Rgba,
    lines: usize,
) -> Div {
    div()
        .text_size(px(size))
        .line_height(px(size * 1.4))
        .font_weight(weight)
        .text_color(color)
        .max_h(px(size * 1.4 * lines as f32))
        .overflow_hidden()
        .child(content.into())
}

/// A track with a fill over `share` of its width.
fn bar(share: f32, fill: Rgba, height: f32) -> Div {
    div()
        .h(px(height))
        .rounded(px(height / 2.0))
        .bg(hairline())
        .child(
            div()
                .h_full()
                .w(relative(share.clamp(0.0, 1.0)))
                .rounded(px(height / 2.0))
                .bg(fill),
        )
}

struct Gauntlet {
    frame: FrameIndex,
    anchor: ListAnchor,
    /// The heights rows reported, from the anchor down.
    heights: HashMap<usize, f32>,
    avatars: Vec<Arc<Image>>,
}

impl Gauntlet {
    fn new() -> Self {
        let s = desktop_gauntlet().tier.scale;
        Self {
            avatars: (0..AVATAR_COUNT)
                .filter_map(|index| {
                    Image::from_rgba(AVATAR_SIZE, AVATAR_SIZE, &avatar_rgba(index)).map(Arc::new)
                })
                .collect(),
            anchor: ListAnchor {
                row: 0,
                top: 8.0 * s,
            },
            heights: HashMap::new(),
            frame: FrameIndex::new(desktop_gauntlet().launch.freeze),
        }
    }

    /// How far the frame has scrolled the list.
    fn scroll_offset(&self) -> f32 {
        self.frame.get() as f32 * SCROLL_PER_FRAME
    }

    /// The rows from the anchor down to the bottom of the window.
    fn visible_rows(&self) -> std::ops::Range<usize> {
        let s = desktop_gauntlet().tier.scale;
        self.anchor.rows(
            self.scroll_offset(),
            DESKTOP_WINDOW.1 as f32,
            8.0 * s,
            10.0 * s,
            |row| self.heights.get(&row).copied(),
        )
    }

    /// Reads the heights of the rows the last layout placed, then advances
    /// the frame unless frozen.
    fn advance(&mut self, window: &Window) {
        let gap = 8.0 * desktop_gauntlet().tier.scale;
        for row in self.visible_rows() {
            if let Some(bounds) = window.bounds_of(&format!("row{row}")) {
                self.heights.insert(row, bounds.size.height);
            }
        }
        if self.frame.frozen() {
            return;
        }
        self.frame.advance();
        let heights = &self.heights;
        let anchor = self
            .anchor
            .advanced(self.scroll_offset(), gap, |row| heights.get(&row).copied());
        if anchor != self.anchor {
            self.anchor = anchor;
            self.heights.retain(|measured, _| *measured >= anchor.row);
        }
    }

    fn ticker_panel(&self) -> impl IntoElement {
        let s = desktop_gauntlet().tier.scale;
        div()
            .flex()
            .flex_wrap()
            .gap(px(4.0 * s))
            .p(px(6.0 * s))
            .bg(rgb(0xE2E8F0))
            .children(
                desktop_gauntlet()
                    .tickers
                    .iter()
                    .enumerate()
                    .map(|(index, ticker)| {
                        let cents = ticker_cents(ticker, self.frame.get());
                        let change = if cents >= ticker.base_cents {
                            rgb(0x16A34A)
                        } else {
                            rgb(0xDC2626)
                        };
                        let share = (cents - ticker.base_cents + ticker.swing_cents) as f32
                            / (2 * ticker.swing_cents) as f32;
                        div()
                            .flex()
                            .items_center()
                            .gap(px(4.0 * s))
                            .px(px(6.0 * s))
                            .py(px(3.0 * s))
                            .rounded(px(6.0 * s))
                            .bg(white())
                            .child(text(ticker.symbol.clone(), 10.0 * s, BOLD, ink()))
                            .child(text(cents_text(cents), 10.0 * s, REGULAR, body_color()))
                            .child(text(change_text(ticker, cents), 10.0 * s, REGULAR, change))
                            .child(bar(share, palette(index), 4.0 * s).w(px(20.0 * s)))
                    }),
            )
    }

    /// The rows from the anchor down to the bottom of the window, moved up by
    /// the frame's offset.
    fn list(&self) -> impl IntoElement {
        let tier = desktop_gauntlet().tier;
        let s = tier.scale;
        let gap = 8.0 * s;
        let rows = self.visible_rows().map(|row| {
            let content = match gauntlet_row(row, tier.columns) {
                GauntletRow::Cards(first) => div()
                    .flex()
                    .items_start()
                    .gap(px(gap))
                    .children((first..first + tier.columns).map(|card| self.card(card)))
                    .into_any_element(),
                GauntletRow::Cluster(cluster) => level(cluster, tier.depth, s).into_any_element(),
            };
            div().id(format!("row{row}")).child(content)
        });
        div()
            .absolute()
            .left(px(0.0))
            .right(px(0.0))
            .top(px(self.anchor.top - self.scroll_offset()))
            .px(px(gap))
            .flex()
            .flex_col()
            .gap(px(gap))
            .children(rows)
    }

    fn card(&self, card: usize) -> impl IntoElement {
        let s = desktop_gauntlet().tier.scale;
        let frame = self.frame.get();
        let post = &desktop_gauntlet().posts[card % POST_COUNT];
        let spark_color = palette(post.color);
        let sparkline = canvas(move |bounds, painter| {
            sparkline(painter, bounds, card, frame, spark_color, 1.5 * s);
        })
        .w_full()
        .h(px(36.0 * s));
        let body = div()
            .flex()
            .flex_col()
            .gap(px(6.0 * s))
            .p(px(10.0 * s))
            .bg(white())
            .rounded(px(12.0 * s))
            .border_1()
            .border_color(hairline())
            .css(
                "box-shadow",
                format!("0px {}px {}px rgba(0, 0, 0, 0.24)", s, 3.0 * s),
            )
            .child(card_header(post, self.avatars.get(card % AVATAR_COUNT), s))
            .child(paragraph(
                post.body.clone(),
                12.0 * s,
                REGULAR,
                body_color(),
                4,
            ))
            .child(card_chips(post, s))
            .child(card_progress(card, frame, s))
            .child(sparkline)
            .child(card_footer(post, s));
        let cell = div().relative().flex_1().min_w(px(0.0)).child(body);
        if card.is_multiple_of(5) {
            cell.child(card_badge(card, frame, s))
        } else {
            cell
        }
    }
}

/// The avatar, the title and the subtitle.
fn card_header(post: &Post, avatar: Option<&Arc<Image>>, s: f32) -> Div {
    let (tag, tag_color) = &post.tags[0];
    let avatar = match avatar {
        Some(image) => img().image(image.clone()),
        None => img(),
    }
    .flex_none()
    .w(px(32.0 * s))
    .h(px(32.0 * s))
    .rounded(px(16.0 * s));
    div()
        .flex()
        .items_center()
        .gap(px(8.0 * s))
        .child(avatar)
        .child(
            div()
                .flex_1()
                .min_w(px(0.0))
                .child(paragraph(post.title.clone(), 13.0 * s, BOLD, ink(), 2))
                // `by Ada Lovelace · #rust`: the author bold, the tag in its color.
                .child(
                    div()
                        .flex()
                        .overflow_hidden()
                        .whitespace_nowrap()
                        .text_size(px(11.0 * s))
                        .line_height(px(11.0 * s * 1.4))
                        .text_color(muted())
                        .child("by ")
                        .child(div().font_weight(BOLD).child(post.author.clone()))
                        .child(" · ")
                        .child(
                            div()
                                .text_color(color(GRADIENT_END_RGB[*tag_color]))
                                .child(tag.clone()),
                        ),
                ),
        )
}

fn card_chips(post: &Post, s: f32) -> Div {
    div()
        .flex()
        .flex_wrap()
        .gap(px(4.0 * s))
        .children(post.tags.iter().map(|(tag, shade)| {
            div()
                .px(px(8.0 * s))
                .py(px(3.0 * s))
                .rounded(px(10.0 * s))
                .bg(color(CHIP_BACKGROUND_RGB[*shade]))
                .child(text(
                    tag.clone(),
                    10.0 * s,
                    REGULAR,
                    color(GRADIENT_END_RGB[*shade]),
                ))
        }))
}

fn card_progress(card: usize, frame: u32, s: f32) -> Div {
    let permille = progress_permille(card, frame);
    div()
        .flex()
        .items_center()
        .gap(px(6.0 * s))
        .child(
            div()
                .flex_1()
                .child(bar(permille as f32 / 1000.0, palette(card), 6.0 * s)),
        )
        .child(text(
            format!("{}%", permille / 10),
            10.0 * s,
            REGULAR,
            muted(),
        ))
}

/// A counter's value above its label.
fn counter(post: &Post, index: usize, s: f32) -> Div {
    div()
        .flex_1()
        .flex()
        .flex_col()
        .items_center()
        .child(text(post.stats[index].clone(), 12.0 * s, BOLD, ink()))
        .child(text(FOOTER_LABELS[index], 9.0 * s, REGULAR, muted()))
}

/// Three counters split by dividers as tall as the row.
fn card_footer(post: &Post, s: f32) -> Div {
    (1..FOOTER_LABELS.len()).fold(div().flex().child(counter(post, 0, s)), |footer, index| {
        footer
            .child(div().w(px(1.0)).bg(hairline()))
            .child(counter(post, index, s))
    })
}

/// The bar across the top of a screen, with its title.
pub(crate) fn top_bar(title: &str) -> Div {
    div()
        .w_full()
        .h(px(56.0))
        .flex_none()
        .px(px(16.0))
        .flex()
        .items_center()
        .bg(color([0x1E, 0x2A, 0x4A]))
        .child(text(title, 20.0, BOLD, white()))
}

/// A translucent tag tilting with the frame: turned, never laid out again.
fn card_badge(card: usize, frame: u32, s: f32) -> Div {
    div()
        .absolute()
        .top(px(6.0 * s))
        .right(px(6.0 * s))
        .css(
            "transform",
            format!("rotate({}deg)", badge_degrees(card, frame)),
        )
        .px(px(6.0 * s))
        .py(px(2.0 * s))
        .rounded(px(8.0 * s))
        .bg(Rgba {
            a: 0.9,
            ..palette(0)
        })
        .child(text("HOT", 9.0 * s, BOLD, Rgba { a: 0.9, ..white() }))
}

/// A card's 48-point sparkline: a line over a fill that fades to nothing.
fn sparkline(
    painter: &mut Painter,
    bounds: Bounds<f32>,
    card: usize,
    frame: u32,
    color: Rgba,
    stroke: f32,
) {
    let step = bounds.size.width / (GAUNTLET_SPARK_POINTS - 1) as f32;
    let line = (0..GAUNTLET_SPARK_POINTS)
        .map(|index| {
            (
                bounds.origin.x + index as f32 * step,
                bounds.origin.y + bounds.size.height * (1.0 - spark_value(card, index, frame)),
            )
        })
        .collect::<Vec<_>>();
    let base = bounds.origin.y + bounds.size.height;
    let mut area = vec![(bounds.origin.x, base)];
    area.extend(line.iter().copied());
    area.push((bounds.origin.x + bounds.size.width, base));
    // The fill fades from a quarter of the color at the top to nothing at
    // the bottom: bands of the area, each its own alpha.
    let bands = 6;
    for band in 0..bands {
        let (top, bottom) = (
            bounds.origin.y + bounds.size.height * band as f32 / bands as f32,
            bounds.origin.y + bounds.size.height * (band + 1) as f32 / bands as f32,
        );
        let alpha = 0.25 * (1.0 - (band as f32 + 0.5) / bands as f32);
        painter.with_clip(
            Bounds {
                origin: point(bounds.origin.x, top),
                size: size(bounds.size.width, bottom - top),
            },
            |painter| painter.polygon(&area, Rgba { a: alpha, ..color }),
        );
    }
    painter.line(&line, color, stroke);
}

/// Levels nested inside one another, each a row of three labels above the
/// next level.
fn level(cluster: usize, remaining: usize, s: f32) -> Div {
    let chips = (0..3).map(|chip| {
        div()
            .flex_1()
            .min_w(px(0.0))
            .rounded(px(2.0 * s))
            .bg(palette(remaining + chip + cluster))
            .child(text(
                format!("C{cluster}.L{remaining}.{chip}"),
                9.0 * s,
                REGULAR,
                white(),
            ))
    });
    let mut node = div()
        .flex()
        .flex_col()
        .gap(px(s))
        .pt(px(s))
        .pb(px(s))
        .pl(px(3.0 * s))
        .pr(px(s))
        .rounded(px(4.0 * s))
        .bg(color(LEVEL_BACKGROUND_RGB[remaining % 2]))
        .child(div().flex().gap(px(2.0 * s)).children(chips));
    if remaining > 0 {
        node = node.child(level(cluster, remaining - 1, s));
    }
    node
}

/// A translucent panel stacked over the list. Its content never changes: each
/// frame turns and moves its box, and turns its nested card back.
fn stacked_layer(layer: usize, frame: u32) -> impl IntoElement {
    let degrees = layer_degrees(layer, frame);
    let rows = desktop_gauntlet().tier.layer_rows;
    let cells =
        (0..rows).map(|line| {
            div().flex().gap(px(3.0)).children(
                (line * LAYER_COLUMNS..(line + 1) * LAYER_COLUMNS).map(|cell| {
                    div()
                        .flex_none()
                        .w(px(22.0))
                        .h(px(22.0))
                        .rounded(px(4.0))
                        .bg(palette(layer_cell_color(layer, cell)))
                        .flex()
                        .items_center()
                        .justify_center()
                        .child(text((cell + 1).to_string(), 9.0, REGULAR, white()))
                }),
            )
        });
    div()
        .absolute()
        .left(px(layer_x(layer, frame)))
        .top(px(layer_y(layer, frame)))
        .w(px(220.0))
        .css("transform", format!("rotate({degrees}deg)"))
        .flex()
        .flex_col()
        .gap(px(8.0))
        .p(px(10.0))
        .rounded(px(12.0))
        .bg(rgba(0x1E293BC0))
        .child(text(format!("Layer {}", layer + 1), 13.0, BOLD, white()))
        .child(div().flex().flex_col().gap(px(3.0)).children(cells))
        .child(
            div()
                .css("transform", format!("rotate({}deg)", -degrees))
                .flex()
                .flex_col()
                .gap(px(2.0))
                .p(px(8.0))
                .rounded(px(8.0))
                .bg(rgba(0xFFFFFFE6))
                .child(text(
                    format!("Nested in layer {}", layer + 1),
                    11.0,
                    BOLD,
                    ink(),
                ))
                .child(text("Tilts against its panel", 11.0, REGULAR, body_color())),
        )
}

impl Render for Gauntlet {
    fn render(&mut self, window: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        self.advance(window);
        if !self.frame.frozen() {
            window.request_animation_frame();
        }
        let tier = desktop_gauntlet().tier;
        let (width, _) = DESKTOP_WINDOW;
        let list = div()
            .relative()
            .flex_1()
            .min_h(px(0.0))
            .overflow_hidden()
            .child(self.list())
            .children((0..tier.layers).map(|layer| stacked_layer(layer, self.frame.get())));
        // `html` takes the window's size; a `body` alone is as tall as its content.
        html().size_full().child(
            body()
                .m_0()
                .size_full()
                .flex()
                .flex_col()
                .bg(rgb(0xEEF0F5))
                .child(top_bar("Gauntlet"))
                .child(
                    div()
                        .w(px((width as f32 * width_fraction(self.frame.get())).round()))
                        .flex_1()
                        .min_h(px(0.0))
                        .flex()
                        .flex_col()
                        .child(self.ticker_panel())
                        .child(list),
                ),
        )
    }
}

fn main() {
    perf_data::log_to_stdout();
    for (weight, bold, file) in [
        ("Roboto", false, "Roboto-Regular.ttf"),
        ("Roboto", true, "Roboto-Bold.ttf"),
        ("DejaVu Sans Mono", false, "DejaVuSansMono.ttf"),
    ] {
        match font_bytes(file) {
            Ok(bytes) => {
                blenderui::add_font(weight, bold, false, &bytes);
            }
            Err(error) => log::warn!("no {file}: {error}"),
        }
    }
    let launch = perf_data::launch();
    let (width, height) = DESKTOP_WINDOW;
    Application::new().run(move |cx: &mut App| {
        let bounds = Bounds {
            origin: point(px(0.0), px(0.0)),
            size: size(px(width as f32), px(height as f32)),
        };
        let options = WindowOptions {
            window_bounds: Some(WindowBounds::Windowed(bounds)),
            is_resizable: Some(false),
            ..Default::default()
        };
        match launch.scenario {
            Scenario::Gauntlet => {
                cx.open_window(options, |_, cx| cx.new(|_| Gauntlet::new()))
                    .unwrap();
            }
            Scenario::Wall => {
                cx.open_window(options, |_, cx| cx.new(|_| wall::Wall::new()))
                    .unwrap();
            }
            Scenario::Patchwork => {
                cx.open_window(options, |_, cx| cx.new(|_| patchwork::Patchwork::new()))
                    .unwrap();
            }
        }
    });
}
