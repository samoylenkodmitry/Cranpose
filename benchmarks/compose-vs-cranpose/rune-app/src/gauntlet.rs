//! The gauntlet in Rune (`../README.md`): a view of the frame that Rune runs
//! again each frame and diffs against the last one. The list builds only the
//! rows from the one at the top of the window to its bottom, as every app
//! without a lazy list does; each row reports its height once laid out. The
//! parts that never change draw as cached layers, which the panels turn and
//! move without drawing them again.

use std::collections::HashMap;

use perf_data::*;
use rune::{
    Color, Cx, El, Image, ImageCache, Shape, Span, Tx, Waker, col, el, memo, rich, row, text,
};

use super::{rgb, roboto};

const INK: [u8; 3] = [0x11, 0x18, 0x27];
const BODY: [u8; 3] = [0x37, 0x41, 0x51];
const MUTED: [u8; 3] = [0x6B, 0x72, 0x80];
const HAIRLINE: [u8; 3] = [0xE5, 0xE7, 0xEB];
const PANEL: [u8; 3] = [0xE2, 0xE8, 0xF0];
const BACKGROUND: [u8; 3] = [0xEE, 0xF0, 0xF5];
const TOP_BAR: [u8; 3] = [0x1E, 0x2A, 0x4A];
const UP: [u8; 3] = [0x16, 0xA3, 0x4A];
const DOWN: [u8; 3] = [0xDC, 0x26, 0x26];
const FOOTER_LABELS: [&str; 3] = ["likes", "replies", "shares"];

/// A color of sRGB `value` with straight alpha `alpha`.
fn tint(value: [u8; 3], alpha: f32) -> Color {
    rgb(value).alpha(alpha)
}

fn palette(index: usize) -> [u8; 3] {
    PALETTE_RGB[index % PALETTE_RGB.len()]
}

/// What the screen reads, which each frame advances.
pub(crate) struct App {
    frame: u32,
    frozen: bool,
    /// Where the list starts.
    anchor: ListAnchor,
    /// The heights rows reported, from the anchor down.
    heights: HashMap<usize, f32>,
    avatars: Vec<Image>,
    /// Keeps the decoder of the avatars alive.
    _images: Option<ImageCache>,
}

impl App {
    pub(crate) fn new() -> Self {
        let s = desktop_gauntlet().tier.scale;
        Self {
            frame: 0,
            frozen: false,
            anchor: ListAnchor {
                row: 0,
                top: 8.0 * s,
            },
            heights: HashMap::new(),
            avatars: Vec::new(),
            _images: None,
        }
    }

    /// How far the frame has scrolled the list.
    fn scroll_offset(&self) -> f32 {
        self.frame as f32 * SCROLL_PER_FRAME
    }

    /// The rows from the anchor down to the bottom of the window.
    fn visible_rows(&self) -> std::ops::Range<usize> {
        let s = desktop_gauntlet().tier.scale;
        let (_, window_height) = DESKTOP_WINDOW;
        self.anchor.rows(
            self.scroll_offset(),
            window_height as f32,
            8.0 * s,
            10.0 * s,
            |row| self.heights.get(&row).copied(),
        )
    }

    /// Decodes the avatars; `wake` redraws the window once they arrive.
    pub(crate) fn setup(&mut self, wake: Waker) {
        let images = ImageCache::new(wake);
        self.avatars = (0..AVATAR_COUNT)
            .map(|index| images.rgba(AVATAR_SIZE, AVATAR_SIZE, avatar_rgba(index)))
            .collect();
        self._images = Some(images);
    }
}

/// Advances the frame unless frozen.
pub(crate) fn poll(app: &mut App, _seconds: f64, tx: &mut Tx) -> bool {
    if app.frozen {
        return false;
    }
    app.frame += 1;
    if app.frame == 1 {
        log::info!("PERF first_frame");
    }
    let gap = 8.0 * desktop_gauntlet().tier.scale;
    let heights = &app.heights;
    let anchor = app
        .anchor
        .advanced(app.scroll_offset(), gap, |row| heights.get(&row).copied());
    if anchor != app.anchor {
        app.anchor = anchor;
        let first = anchor.row;
        app.heights.retain(|measured, _| *measured >= first);
    }
    let freeze = desktop_gauntlet().launch.freeze;
    if freeze > 0 && app.frame >= freeze {
        app.frozen = true;
        log::info!("PERF frozen frame={}", app.frame);
    }
    tx.instant();
    tx.wake_after(std::time::Duration::from_millis(1));
    true
}

pub(crate) fn view(app: &App, _: &mut Cx<App>) -> El {
    let tier = desktop_gauntlet().tier;
    let (width, _) = DESKTOP_WINDOW;
    let list = el()
        .w_full()
        .grow(1.0)
        .min_h(0.0)
        .clip_rounded(0.0)
        .child(list(app))
        .children((0..tier.layers).map(|layer| stacked_layer(layer, app.frame)));
    let content = col()
        .w((width as f32 * width_fraction(app.frame)).round())
        .grow(1.0)
        .min_h(0.0)
        .child(ticker_panel(app.frame))
        .child(list);
    col()
        .w_full()
        .h_full()
        .fill(rgb(BACKGROUND))
        .child(
            el().h(56.0)
                .w_full()
                .px(16.0)
                .fill(rgb(TOP_BAR))
                .center_y()
                .child(
                    text("Gauntlet")
                        .font(roboto(20.0, true))
                        .color(Color::WHITE),
                ),
        )
        .child(content)
}

fn ticker_panel(frame: u32) -> El {
    let s = desktop_gauntlet().tier.scale;
    el().w_full().padding(6.0 * s).fill(rgb(PANEL)).child(
        row().w_full().flex_wrap().gap(4.0 * s).children(
            desktop_gauntlet()
                .tickers
                .iter()
                .enumerate()
                .map(|(index, ticker)| ticker_tile(index, ticker, frame)),
        ),
    )
}

/// One quote: symbol, price, change and a bar, all from the frame.
fn ticker_tile(index: usize, ticker: &Ticker, frame: u32) -> El {
    let s = desktop_gauntlet().tier.scale;
    let cents = ticker_cents(ticker, frame);
    let share =
        (cents - ticker.base_cents + ticker.swing_cents) as f32 / (2 * ticker.swing_cents) as f32;
    row()
        .key(format!("tick{index}"))
        .gap(4.0 * s)
        .py(3.0 * s)
        .px(6.0 * s)
        .radius(6.0 * s)
        .fill(Color::WHITE)
        .center_y()
        .child(
            text(ticker.symbol.clone())
                .font(roboto(10.0 * s, true))
                .color(rgb(INK)),
        )
        .child(
            text(cents_text(cents))
                .font(roboto(10.0 * s, false))
                .color(rgb(BODY)),
        )
        .child(
            text(change_text(ticker, cents))
                .font(roboto(10.0 * s, false))
                .color(rgb(if cents >= ticker.base_cents { UP } else { DOWN })),
        )
        .child(bar(Some(20.0 * s), 4.0 * s, share, palette(index)))
}

/// A track with a fill over `share` of its width; `None` for the room left.
fn bar(width: Option<f32>, height: f32, share: f32, fill: [u8; 3]) -> El {
    let track = el().h(height).radius(height / 2.0).fill(rgb(HAIRLINE));
    let track = match width {
        Some(width) => track.w(width),
        None => track.w_full().grow(1.0),
    };
    track.clip_rounded(height / 2.0).child(
        el().h(height)
            .w_frac(share.clamp(0.0, 1.0))
            .radius(height / 2.0)
            .fill(rgb(fill)),
    )
}

/// The rows from the anchor down to the bottom of the window, moved up by
/// the frame's offset.
fn list(app: &App) -> El {
    let tier = desktop_gauntlet().tier;
    let s = tier.scale;
    let gap = 8.0 * s;
    let rows = app.visible_rows().map(|index| {
        let content = match gauntlet_row(index, tier.columns) {
            GauntletRow::Cards(first) => row()
                .w_full()
                .gap(gap)
                .children((first..first + tier.columns).map(|card| card_view(card, app))),
            GauntletRow::Cluster(cluster) => memo(
                format!("cluster{cluster}"),
                (tier.depth, s.to_bits()),
                move || level(cluster, tier.depth, s),
            ),
        };
        content
            .key(format!("row{index}"))
            .on_layout(move |app: &mut App, rect| {
                app.heights.insert(index, rect[3]);
            })
    });
    col()
        .pin(
            Some(0.0),
            Some(app.anchor.top - app.scroll_offset()),
            Some(0.0),
            None,
        )
        .px(gap)
        .gap(gap)
        .children(rows)
}

fn card_view(card: usize, app: &App) -> El {
    let s = desktop_gauntlet().tier.scale;
    let post = &desktop_gauntlet().posts[card % POST_COUNT];
    let permille = progress_permille(card, app.frame);
    let body = col()
        .w_full()
        .gap(6.0 * s)
        .padding(10.0 * s)
        .fill(Color::WHITE)
        .radius(12.0 * s)
        .border(1.0, rgb(HAIRLINE))
        .shadow(s, 3.0 * s, Color::BLACK.alpha(0.24))
        .child(memo(
            format!("head{card}"),
            (s.to_bits(), app.avatars.len()),
            || card_header(card, app, s),
        ))
        .child(
            row()
                .w_full()
                .gap(6.0 * s)
                .center_y()
                .child(bar(None, 6.0 * s, permille as f32 / 1000.0, palette(card)))
                .child(
                    text(format!("{}%", permille / 10))
                        .font(roboto(10.0 * s, false))
                        .color(rgb(MUTED)),
                ),
        )
        .child(sparkline(card, app.frame, rgb(palette(post.color)), s))
        .child(memo(format!("foot{card}"), s.to_bits(), || {
            card_footer(card, s)
        }));
    let card_el = el()
        .key(format!("card{card}"))
        .grow(1.0)
        .min_w(0.0)
        .child(body);
    if card.is_multiple_of(5) {
        // A translucent tag tilting with the frame: turned, never laid out
        // again.
        card_el.child(
            el().pin(None, Some(6.0 * s), Some(6.0 * s), None).child(
                el().py(2.0 * s)
                    .px(6.0 * s)
                    .radius(8.0 * s)
                    .fill(tint(palette(0), 0.9))
                    .rotate(badge_degrees(card, app.frame).to_radians())
                    .child(
                        text("HOT")
                            .font(roboto(9.0 * s, true))
                            .color(Color::WHITE.alpha(0.9)),
                    ),
            ),
        )
    } else {
        card_el
    }
}

/// The avatar, the title and the subtitle, then the body text and the chips:
/// what a card never changes.
fn card_header(card: usize, app: &App, s: f32) -> El {
    let post = &desktop_gauntlet().posts[card % POST_COUNT];
    let (tag, tag_color) = &post.tags[0];
    // `by Ada Lovelace · #rust`: the author bold, the tag in its color.
    let by = "by ";
    let dot = " · ";
    let subtitle = format!("{by}{}{dot}{tag}", post.author);
    let author = by.len()..by.len() + post.author.len();
    let tag_start = author.end + dot.len();
    let end = subtitle.len();
    let subtitle = rich(
        subtitle,
        vec![
            Span::new(author.start, author.end).weight(700.0),
            Span::new(tag_start, end).color(rgb(GRADIENT_END_RGB[*tag_color])),
        ],
    )
    .font(roboto(11.0 * s, false))
    .color(rgb(MUTED));
    let avatar = match app.avatars.get(card % AVATAR_COUNT) {
        Some(image) => el()
            .size(32.0 * s, 32.0 * s)
            .radius(16.0 * s)
            .pixels(image.pixels()),
        None => el().size(32.0 * s, 32.0 * s),
    };
    col()
        .w_full()
        .gap(6.0 * s)
        .child(
            row().w_full().gap(8.0 * s).center_y().child(avatar).child(
                col()
                    .grow(1.0)
                    .min_w(0.0)
                    .child(
                        text(post.title.clone())
                            .font(roboto(13.0 * s, true))
                            .color(rgb(INK))
                            .wrap()
                            .max_lines(2),
                    )
                    .child(subtitle),
            ),
        )
        .child(
            text(post.body.clone())
                .font(roboto(12.0 * s, false))
                .color(rgb(BODY))
                .wrap()
                .max_lines(4),
        )
        .child(
            row()
                .w_full()
                .flex_wrap()
                .gap(4.0 * s)
                .children(post.tags.iter().map(|(value, color)| {
                    el().py(3.0 * s)
                        .px(8.0 * s)
                        .radius(10.0 * s)
                        .fill(rgb(CHIP_BACKGROUND_RGB[*color]))
                        .child(
                            text(value.clone())
                                .font(roboto(10.0 * s, false))
                                .color(rgb(GRADIENT_END_RGB[*color])),
                        )
                })),
        )
}

/// A card's 48-point sparkline: the fading fill is a gradient box under a
/// white polygon above the line.
fn sparkline(card: usize, frame: u32, color: Color, s: f32) -> El {
    let (width, height) = (200.0_f32, 36.0 * s);
    let step = width / (GAUNTLET_SPARK_POINTS - 1) as f32;
    let line = (0..GAUNTLET_SPARK_POINTS)
        .map(|index| {
            [
                index as f32 * step,
                height * (1.0 - spark_value(card, index, frame)),
            ]
        })
        .collect::<Vec<_>>();
    let mut above = vec![[0.0, 0.0], [width, 0.0]];
    above.extend(line.iter().rev().copied());
    let shape = Shape::new()
        .fill(above, Color::WHITE)
        .stroke(line, 1.5 * s, color)
        .view_box(width, height);
    el().w_full()
        .h(height)
        .gradient(color.alpha(0.25), color.alpha(0.0))
        .child(rune::canvas(shape).w_full().h(height))
}

/// Three counters split by dividers as tall as the tallest counter.
fn card_footer(card: usize, s: f32) -> El {
    let post = &desktop_gauntlet().posts[card % POST_COUNT];
    let counter = |index: usize| {
        col()
            .grow(1.0)
            .min_w(0.0)
            .items_center()
            .child(
                text(post.stats[index].clone())
                    .font(roboto(12.0 * s, true))
                    .color(rgb(INK)),
            )
            .child(
                text(FOOTER_LABELS[index])
                    .font(roboto(9.0 * s, false))
                    .color(rgb(MUTED)),
            )
    };
    let divider = || el().w(1.0).fill(rgb(HAIRLINE));
    row()
        .w_full()
        .child(counter(0))
        .child(divider())
        .child(counter(1))
        .child(divider())
        .child(counter(2))
}

/// Levels nested inside one another, each a row of three labels above the
/// next level.
fn level(cluster: usize, remaining: usize, s: f32) -> El {
    let chips = (0..3).map(|chip| {
        el().grow(1.0)
            .min_w(0.0)
            .radius(2.0 * s)
            .fill(rgb(palette(remaining + chip + cluster)))
            .child(
                text(format!("C{cluster}.L{remaining}.{chip}"))
                    .font(roboto(9.0 * s, false))
                    .color(Color::WHITE),
            )
    });
    let inner = (remaining > 0).then(|| level(cluster, remaining - 1, s));
    let mut level = col()
        .w_full()
        .gap(s)
        .pt(s)
        .pb(s)
        .pl(3.0 * s)
        .pr(s)
        .radius(4.0 * s)
        .fill(rgb(LEVEL_BACKGROUND_RGB[remaining % 2]))
        .child(row().w_full().gap(2.0 * s).children(chips));
    if let Some(inner) = inner {
        level = level.child(inner);
    }
    level
}

/// A translucent panel stacked over the list. Its content never changes: each
/// frame turns and moves its box, and turns its nested card back.
fn stacked_layer(layer: usize, frame: u32) -> El {
    let radians = layer_degrees(layer, frame).to_radians();
    let rows = desktop_gauntlet().tier.layer_rows;
    let cells = (0..rows).map(|line| {
        row().gap(3.0).children(
            (line * LAYER_COLUMNS..(line + 1) * LAYER_COLUMNS).map(|cell| {
                el().size(22.0, 22.0)
                    .radius(4.0)
                    .fill(rgb(palette(layer_cell_color(layer, cell))))
                    .center()
                    .child(
                        text((cell + 1).to_string())
                            .font(roboto(9.0, false))
                            .color(Color::WHITE),
                    )
            }),
        )
    });
    el().key(format!("layer{layer}"))
        .pin(
            Some(layer_x(layer, frame)),
            Some(layer_y(layer, frame)),
            None,
            None,
        )
        .w(220.0)
        .rotate(radians)
        .child(
            col()
                .w_full()
                .gap(8.0)
                .padding(10.0)
                .radius(12.0)
                .fill(tint([0x1E, 0x29, 0x3B], 0xC0 as f32 / 255.0))
                .child(
                    text(format!("Layer {}", layer + 1))
                        .font(roboto(13.0, true))
                        .color(Color::WHITE),
                )
                .child(col().gap(3.0).children(cells))
                .child(
                    col()
                        .w_full()
                        .gap(2.0)
                        .padding(8.0)
                        .radius(8.0)
                        .fill(Color::WHITE.alpha(0xE6 as f32 / 255.0))
                        .rotate(-radians)
                        .child(
                            text(format!("Nested in layer {}", layer + 1))
                                .font(roboto(11.0, true))
                                .color(rgb(INK)),
                        )
                        .child(
                            text("Tilts against its panel")
                                .font(roboto(11.0, false))
                                .color(rgb(BODY)),
                        ),
                ),
        )
}
