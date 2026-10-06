//! The gauntlet in Freya: the screen `compose-app/.../Gauntlet.kt` draws,
//! element for element, in Freya's own idiom on Skia: components render
//! again when a state they read changes, Torin lays the elements out, flow
//! rows wrap, and a canvas draws each sparkline. The benchmark's README
//! describes it; `desktop.py` runs it: `PERF_TIER=16 PERF_FONTS=../fonts
//! cargo run --release`.
//!
//! Every frame advances a frame index and everything follows from it, never
//! from wall time. A task wakes on each frame of the renderer's ticker and
//! advances the frame state; the components that read it render again. The
//! list renders the rows from the first one on screen down to the window's
//! bottom, and each row reports its height once Torin has laid it out.

use std::{borrow::Cow, collections::HashMap, rc::Rc};

use freya::{
    elements::image::ImageHandle,
    engine::prelude::{AlphaType, Paint, PaintStyle, PathBuilder, SkPoint},
    prelude::*,
};
use perf_data::*;

const FOOTER_LABELS: [&str; 3] = ["likes", "replies", "shares"];

const fn rgb(value: [u8; 3]) -> Color {
    Color::from_rgb(value[0], value[1], value[2])
}

fn palette(index: usize) -> Color {
    rgb(PALETTE_RGB[index % PALETTE_RGB.len()])
}

/// The states the screen reads, which the frame task advances.
#[derive(Clone, Copy, PartialEq)]
struct Shared {
    frame: State<u32>,
    /// Where the list starts.
    anchor: State<ListAnchor>,
    /// The heights rows reported, from the anchor down.
    heights: State<HashMap<usize, f32>>,
}

/// The avatars, decoded once.
#[derive(Clone)]
struct Avatars(Rc<Vec<ImageHandle>>);

/// Text in Roboto at `size`: lines 1.4 em apart, without leading above the
/// first line or below the last, as Compose's default line height trims it.
fn text(content: impl Into<Cow<'static, str>>, size: f32, color: Color, bold: bool) -> Label {
    label()
        .text(content)
        .font_size(size)
        .color(color)
        .font_weight(if bold {
            FontWeight::BOLD
        } else {
            FontWeight::NORMAL
        })
        .line_height(1.4)
        .text_height(TextHeightBehavior::DisableAll)
}

/// A rounded track filled to `share` in `color`.
fn bar(share: f32, color: Color, width: Size, height: f32) -> Rect {
    rect()
        .width(width)
        .height(Size::px(height))
        .corner_radius(height / 2.0)
        .background(rgb(HAIRLINE_RGB))
        .child(
            rect()
                .width(Size::percent(share.clamp(0.0, 1.0) * 100.0))
                .height(Size::fill())
                .corner_radius(height / 2.0)
                .background(color),
        )
}

/// Moves the anchor past the rows that scrolled off above on `frame`.
fn advance(shared: Shared, frame: u32) {
    let gap = 8.0 * desktop_gauntlet().tier.scale;
    let first = *shared.anchor.peek();
    let anchor = {
        let heights = shared.heights.peek();
        first.advanced(frame as f32 * SCROLL_PER_FRAME, gap, |row| {
            heights.get(&row).copied()
        })
    };
    if anchor != first {
        let (mut current, mut heights) = (shared.anchor, shared.heights);
        current.set(anchor);
        heights
            .write()
            .retain(|measured, _| *measured >= anchor.row);
    }
}

fn app() -> impl IntoElement {
    let s = desktop_gauntlet().tier.scale;
    let shared = use_hook(|| Shared {
        frame: State::create(0),
        anchor: State::create(ListAnchor {
            row: 0,
            top: 8.0 * s,
        }),
        heights: State::create(HashMap::new()),
    });
    use_provide_context(|| shared);
    use_provide_context(|| {
        Avatars(Rc::new(
            (0..AVATAR_COUNT)
                .filter_map(|index| {
                    ImageHandle::from_rgba(
                        AVATAR_SIZE,
                        AVATAR_SIZE,
                        avatar_rgba(index).into(),
                        AlphaType::Opaque,
                    )
                })
                .collect(),
        ))
    });
    use_hook(|| {
        let mut ticker = RenderingTicker::get();
        let platform = Platform::get();
        let freeze = desktop_gauntlet().launch.freeze;
        spawn(async move {
            platform.send(UserEvent::RequestRedraw);
            loop {
                ticker.tick().await;
                let frame = *shared.frame.peek() + 1;
                if frame == 1 {
                    log::info!("PERF first_frame");
                }
                advance(shared, frame);
                let mut current = shared.frame;
                current.set(frame);
                if freeze > 0 && frame >= freeze {
                    log::info!("PERF frozen frame={frame}");
                    break;
                }
                platform.send(UserEvent::RequestRedraw);
            }
        });
    });
    rect()
        .expanded()
        .background(rgb(BACKGROUND_RGB))
        .child(
            rect()
                .width(Size::fill())
                .height(Size::px(56.0))
                .padding(Gaps::new(0.0, 16.0, 0.0, 16.0))
                .main_align(Alignment::Center)
                .background(rgb(TOP_BAR_RGB))
                .child(text("Gauntlet", 20.0, Color::WHITE, true)),
        )
        .child(Screen {
            frame: shared.frame,
        })
}

/// The ticker strip and the list, in a column whose width follows the frame.
#[derive(PartialEq)]
struct Screen {
    frame: State<u32>,
}

impl Component for Screen {
    fn render(&self) -> impl IntoElement {
        let (width, _) = DESKTOP_WINDOW;
        rect()
            .width(Size::px(
                (width as f32 * width_fraction(*self.frame.read())).round(),
            ))
            .height(Size::fill())
            .child(TickerPanel)
            .child(RowList)
    }
}

#[derive(PartialEq)]
struct TickerPanel;

impl Component for TickerPanel {
    fn render(&self) -> impl IntoElement {
        let s = desktop_gauntlet().tier.scale;
        rect()
            .width(Size::fill())
            .horizontal()
            .content(Content::Wrap {
                wrap_spacing: Some(4.0 * s),
            })
            .spacing(4.0 * s)
            .padding(Gaps::new_all(6.0 * s))
            .background(rgb(PANEL_RGB))
            .children(
                (0..desktop_gauntlet().tickers.len()).map(|index| TickerTile { index }.into()),
            )
    }
}

/// One quote: symbol, price, change and a bar, all from the frame.
#[derive(PartialEq)]
struct TickerTile {
    index: usize,
}

impl Component for TickerTile {
    fn render_key(&self) -> DiffKey {
        DiffKey::from(&self.index)
    }

    fn render(&self) -> impl IntoElement {
        let shared = use_consume::<Shared>();
        let s = desktop_gauntlet().tier.scale;
        let ticker = &desktop_gauntlet().tickers[self.index];
        let cents = ticker_cents(ticker, *shared.frame.read());
        let share = (cents - ticker.base_cents + ticker.swing_cents) as f32
            / (2 * ticker.swing_cents) as f32;
        rect()
            .horizontal()
            .cross_align(Alignment::Center)
            .spacing(4.0 * s)
            .padding(Gaps::new(3.0 * s, 6.0 * s, 3.0 * s, 6.0 * s))
            .corner_radius(6.0 * s)
            .background(Color::WHITE)
            .child(text(ticker.symbol.clone(), 10.0 * s, rgb(INK_RGB), true))
            .child(text(cents_text(cents), 10.0 * s, rgb(BODY_RGB), false))
            .child(text(
                change_text(ticker, cents),
                10.0 * s,
                if cents >= ticker.base_cents {
                    rgb(UP_RGB)
                } else {
                    rgb(DOWN_RGB)
                },
                false,
            ))
            .child(bar(share, palette(self.index), Size::px(20.0 * s), 4.0 * s))
    }
}

/// The rows from the anchor down to the bottom of the window, moved up by
/// the frame's offset.
#[derive(PartialEq)]
struct RowList;

impl Component for RowList {
    fn render(&self) -> impl IntoElement {
        let shared = use_consume::<Shared>();
        let s = desktop_gauntlet().tier.scale;
        let gap = 8.0 * s;
        let offset = *shared.frame.read() as f32 * SCROLL_PER_FRAME;
        let anchor = *shared.anchor.read();
        let (_, window_height) = DESKTOP_WINDOW;
        let rows = {
            let heights = shared.heights.peek();
            anchor.rows(offset, window_height as f32, gap, 10.0 * s, |row| {
                heights.get(&row).copied()
            })
        };
        rect()
            .width(Size::fill())
            .height(Size::fill())
            .overflow(Overflow::Clip)
            .child(
                rect()
                    .width(Size::fill())
                    .offset_y(anchor.top - offset)
                    .padding(Gaps::new(0.0, gap, 0.0, gap))
                    .spacing(gap)
                    .children(rows.map(|row| ListRow { row }.into())),
            )
    }
}

/// A list row: cards side by side, or a cluster. It reports its height.
#[derive(PartialEq)]
struct ListRow {
    row: usize,
}

impl Component for ListRow {
    fn render_key(&self) -> DiffKey {
        DiffKey::from(&self.row)
    }

    fn render(&self) -> impl IntoElement {
        let shared = use_consume::<Shared>();
        let row = self.row;
        let tier = desktop_gauntlet().tier;
        let measured = rect()
            .width(Size::fill())
            .on_sized(move |event: Event<SizedEventData>| {
                let mut heights = shared.heights;
                heights.write().insert(row, event.area.height());
            });
        match gauntlet_row(row, tier.columns) {
            GauntletRow::Cards(first) => measured
                .horizontal()
                .content(Content::Flex)
                .spacing(8.0 * tier.scale)
                .children((first..first + tier.columns).map(|card| PostCard { card }.into())),
            GauntletRow::Cluster(cluster) => measured.child(ClusterLevel {
                cluster,
                remaining: tier.depth,
            }),
        }
    }
}

#[derive(PartialEq)]
struct PostCard {
    card: usize,
}

impl Component for PostCard {
    fn render_key(&self) -> DiffKey {
        DiffKey::from(&self.card)
    }

    fn render(&self) -> impl IntoElement {
        let avatars = use_consume::<Avatars>();
        let s = desktop_gauntlet().tier.scale;
        let card = self.card;
        let post = &desktop_gauntlet().posts[card % POST_COUNT];
        let (tag, tag_color) = &post.tags[0];
        // `by Ada Lovelace · #rust`: the author bold, the tag in its color.
        let subtitle = paragraph()
            .span(Span::new("by "))
            .span(Span::new(post.author.clone()).font_weight(FontWeight::BOLD))
            .span(Span::new(" · "))
            .span(Span::new(tag.clone()).color(rgb(GRADIENT_END_RGB[*tag_color])))
            .font_size(11.0 * s)
            .color(rgb(MUTED_RGB))
            .max_lines(1)
            .text_overflow(TextOverflow::Ellipsis)
            .line_height(1.4)
            .text_height(TextHeightBehavior::DisableAll);
        let header = rect()
            .width(Size::fill())
            .horizontal()
            .content(Content::Flex)
            .cross_align(Alignment::Center)
            .spacing(8.0 * s)
            .maybe_child(avatars.0.get(card % AVATAR_COUNT).map(|avatar| {
                image(avatar.clone())
                    .width(Size::px(32.0 * s))
                    .height(Size::px(32.0 * s))
                    .corner_radius(16.0 * s)
            }))
            .child(
                rect()
                    .width(Size::flex(1.0))
                    .child(
                        text(post.title.clone(), 13.0 * s, rgb(INK_RGB), true)
                            .max_lines(2)
                            .text_overflow(TextOverflow::Ellipsis),
                    )
                    .child(subtitle),
            );
        let chips = post.tags.iter().map(|(text_value, color)| {
            rect()
                .padding(Gaps::new(3.0 * s, 8.0 * s, 3.0 * s, 8.0 * s))
                .corner_radius(10.0 * s)
                .background(rgb(CHIP_BACKGROUND_RGB[*color]))
                .child(text(
                    text_value.clone(),
                    10.0 * s,
                    rgb(GRADIENT_END_RGB[*color]),
                    false,
                ))
                .into()
        });
        let body = rect()
            .width(Size::fill())
            .spacing(6.0 * s)
            .padding(Gaps::new_all(10.0 * s))
            .corner_radius(12.0 * s)
            .background(Color::WHITE)
            .border(
                Border::new()
                    .fill(rgb(HAIRLINE_RGB))
                    .width(1.0)
                    .alignment(BorderAlignment::Inner),
            )
            .shadow(
                Shadow::new()
                    .y(s)
                    .blur(3.0 * s)
                    .color(Color::from_af32rgb(0.24, 0, 0, 0)),
            )
            .child(header)
            .child(
                text(post.body.clone(), 12.0 * s, rgb(BODY_RGB), false)
                    .max_lines(4)
                    .text_overflow(TextOverflow::Ellipsis),
            )
            .child(CardProgress { card })
            .child(CardSparkline {
                card,
                color: post.color,
            })
            .child(
                rect()
                    .width(Size::fill())
                    .horizontal()
                    .content(Content::Wrap {
                        wrap_spacing: Some(4.0 * s),
                    })
                    .spacing(4.0 * s)
                    .children(chips),
            )
            .child(CardFooter { card });
        rect()
            .width(Size::flex(1.0))
            .child(body)
            .maybe_child(card.is_multiple_of(5).then_some(HotBadge { card }))
    }
}

/// Three counters split by dividers as tall as the row. Torin has no
/// intrinsic height, so a divider is the left border of the counter after it,
/// and the counters are as tall as the row.
#[derive(PartialEq)]
struct CardFooter {
    card: usize,
}

impl Component for CardFooter {
    fn render(&self) -> impl IntoElement {
        let s = desktop_gauntlet().tier.scale;
        let post = &desktop_gauntlet().posts[self.card % POST_COUNT];
        rect()
            .width(Size::fill())
            .horizontal()
            .content(Content::Flex)
            .children(FOOTER_LABELS.iter().enumerate().map(|(index, label_text)| {
                rect()
                    .width(Size::flex(1.0))
                    .cross_align(Alignment::Center)
                    .border((index > 0).then(|| {
                        Border::new().fill(rgb(HAIRLINE_RGB)).width(BorderWidth {
                            left: 1.0,
                            ..BorderWidth::default()
                        })
                    }))
                    .child(text(
                        post.stats[index].clone(),
                        12.0 * s,
                        rgb(INK_RGB),
                        true,
                    ))
                    .child(text(*label_text, 9.0 * s, rgb(MUTED_RGB), false))
                    .into()
            }))
    }
}

#[derive(PartialEq)]
struct CardProgress {
    card: usize,
}

impl Component for CardProgress {
    fn render(&self) -> impl IntoElement {
        let shared = use_consume::<Shared>();
        let s = desktop_gauntlet().tier.scale;
        let permille = progress_permille(self.card, *shared.frame.read());
        rect()
            .width(Size::fill())
            .horizontal()
            .content(Content::Flex)
            .cross_align(Alignment::Center)
            .spacing(6.0 * s)
            .child(bar(
                permille as f32 / 1000.0,
                palette(self.card),
                Size::flex(1.0),
                6.0 * s,
            ))
            .child(text(
                format!("{}%", permille / 10),
                10.0 * s,
                rgb(MUTED_RGB),
                false,
            ))
    }
}

/// A card's 48-point line over a fading fill, drawn from the frame.
#[derive(PartialEq)]
struct CardSparkline {
    card: usize,
    color: usize,
}

impl Component for CardSparkline {
    fn render(&self) -> impl IntoElement {
        let shared = use_consume::<Shared>();
        let s = desktop_gauntlet().tier.scale;
        let frame = *shared.frame.read();
        let (card, color) = (self.card, palette(self.color));
        let fade = LinearGradient::new()
            .angle(0.0)
            .stop((color.with_a(64), 0.0))
            .stop((color.with_a(0), 100.0));
        canvas(RenderCallback::new(move |context| {
            let (width, height) = (context.size.width, context.size.height);
            let step = width / (GAUNTLET_SPARK_POINTS - 1) as f32;
            let point = |index: usize| {
                SkPoint::new(
                    index as f32 * step,
                    height * (1.0 - spark_value(card, index, frame)),
                )
            };
            let mut area = PathBuilder::new();
            area.move_to((0.0, height));
            for index in 0..GAUNTLET_SPARK_POINTS {
                area.line_to(point(index));
            }
            area.line_to((width, height));
            area.close();
            let mut line = PathBuilder::new();
            line.move_to(point(0));
            for index in 1..GAUNTLET_SPARK_POINTS {
                line.line_to(point(index));
            }
            let mut paint = Paint::default();
            paint.set_anti_alias(true);
            if let Some(shader) =
                fade.prepare_shader(Area::new((0.0, 0.0).into(), (width, height).into()))
            {
                paint.set_shader(shader);
            }
            context.canvas.draw_path(&area.detach(), &paint);
            let mut stroke = Paint::default();
            stroke.set_anti_alias(true);
            stroke.set_style(PaintStyle::Stroke);
            stroke.set_stroke_width(1.5 * s);
            stroke.set_color(color);
            context.canvas.draw_path(&line.detach(), &stroke);
        }))
        .width(Size::fill())
        .height(Size::px(36.0 * s))
    }
}

/// A translucent tag tilting with the frame: rotated, never laid out again.
#[derive(PartialEq)]
struct HotBadge {
    card: usize,
}

impl Component for HotBadge {
    fn render(&self) -> impl IntoElement {
        let shared = use_consume::<Shared>();
        let s = desktop_gauntlet().tier.scale;
        rect()
            .position(Position::new_absolute().top(6.0 * s).right(6.0 * s))
            .rotation(badge_degrees(self.card, *shared.frame.read()))
            .opacity(0.9)
            .padding(Gaps::new(2.0 * s, 6.0 * s, 2.0 * s, 6.0 * s))
            .corner_radius(8.0 * s)
            .background(palette(0))
            .child(text("HOT", 9.0 * s, Color::WHITE, true))
    }
}

/// Levels nested inside one another, each a row of three labels above the
/// next level.
#[derive(PartialEq)]
struct ClusterLevel {
    cluster: usize,
    remaining: usize,
}

impl Component for ClusterLevel {
    fn render(&self) -> impl IntoElement {
        let s = desktop_gauntlet().tier.scale;
        let (cluster, remaining) = (self.cluster, self.remaining);
        let chips = rect()
            .width(Size::fill())
            .horizontal()
            .content(Content::Flex)
            .spacing(2.0 * s)
            .children((0..3).map(|chip| {
                rect()
                    .width(Size::flex(1.0))
                    .corner_radius(2.0 * s)
                    .background(palette(remaining + chip + cluster))
                    .child(text(
                        format!("C{cluster}.L{remaining}.{chip}"),
                        9.0 * s,
                        Color::WHITE,
                        false,
                    ))
                    .into()
            }));
        rect()
            .width(Size::fill())
            .spacing(s)
            .padding(Gaps::new(s, s, s, 3.0 * s))
            .corner_radius(4.0 * s)
            .background(rgb(LEVEL_BACKGROUND_RGB[remaining % 2]))
            .child(chips)
            .maybe_child((remaining > 0).then(|| ClusterLevel {
                cluster,
                remaining: remaining - 1,
            }))
    }
}

fn main() {
    perf_data::log_to_stdout();
    let (width, height) = DESKTOP_WINDOW;
    let mut config = LaunchConfig::new();
    for file in ["Roboto-Regular.ttf", "Roboto-Bold.ttf"] {
        match std::fs::read(font_path(file)) {
            Ok(bytes) => config = config.with_font("Roboto", Bytes::from(bytes)),
            Err(error) => log::warn!("no {file}: {error}"),
        }
    }
    launch(
        config.with_default_font("Roboto").with_window(
            WindowConfig::new(app)
                .with_title("Gauntlet")
                .with_size(f64::from(width), f64::from(height))
                .with_resizable(false),
        ),
    );
}
