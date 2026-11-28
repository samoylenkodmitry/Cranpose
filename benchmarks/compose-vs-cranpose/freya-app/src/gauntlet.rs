use std::{borrow::Cow, collections::BTreeMap, rc::Rc};

use freya::{
    elements::image::ImageHandle,
    engine::prelude::{AlphaType, Paint, PaintStyle, PathBuilder, SkPoint},
    prelude::*,
};
use perf_data::*;

const FOOTER_LABELS: [&str; 3] = ["likes", "replies", "shares"];
const INK_RGB: [u8; 3] = [0x11, 0x18, 0x27];
const BODY_RGB: [u8; 3] = [0x37, 0x41, 0x51];
const MUTED_RGB: [u8; 3] = [0x6B, 0x72, 0x80];
const HAIRLINE_RGB: [u8; 3] = [0xE5, 0xE7, 0xEB];
const PANEL_RGB: [u8; 3] = [0xE2, 0xE8, 0xF0];
const BACKGROUND_RGB: [u8; 3] = [0xEE, 0xF0, 0xF5];
const TOP_BAR_RGB: [u8; 3] = [0x1E, 0x2A, 0x4A];
const UP_RGB: [u8; 3] = [0x16, 0xA3, 0x4A];
const DOWN_RGB: [u8; 3] = [0xDC, 0x26, 0x26];
const LAYER_BACKGROUND: Color = Color::from_argb(0xC0, 0x1E, 0x29, 0x3B);
const NESTED_BACKGROUND: Color = Color::from_argb(0xE6, 0xFF, 0xFF, 0xFF);

pub(super) const fn rgb(value: [u8; 3]) -> Color {
    Color::from_rgb(value[0], value[1], value[2])
}

fn palette(index: usize) -> Color {
    rgb(PALETTE_RGB[index % PALETTE_RGB.len()])
}

#[derive(Clone, Copy, PartialEq)]
struct Shared {
    frame: State<u32>,
    anchor: State<ListAnchor>,
    heights: State<BTreeMap<usize, f32>>,
}

#[derive(Clone)]
struct Avatars(Rc<Vec<ImageHandle>>);

pub(super) fn text(
    content: impl Into<Cow<'static, str>>,
    size: f32,
    color: Color,
    bold: bool,
) -> Label {
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

pub(super) fn app() -> impl IntoElement {
    let s = desktop_gauntlet().tier.scale;
    let shared = use_hook(|| Shared {
        frame: State::create(0),
        anchor: State::create(ListAnchor {
            row: 0,
            top: 8.0 * s,
        }),
        heights: State::create(BTreeMap::new()),
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
        let ticker = RenderingTicker::get();
        let platform = Platform::get().current_window();
        let freeze = desktop_gauntlet().launch.freeze;
        spawn(async move {
            platform.request_redraw();
            loop {
                ticker.tick().await;
                let frame = *shared.frame.peek() + 1;
                if frame == 1 {
                    println!("PERF first_frame");
                }
                advance(shared, frame);
                let mut current = shared.frame;
                current.set(frame);
                perf_data::updates::shown(frame);
                if freeze > 0 && frame >= freeze {
                    println!("PERF frozen frame={frame}");
                    break;
                }
                platform.request_redraw();
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
            // The panels stack over the list and show only over it, as every
            // app clips them.
            .child(
                rect()
                    .width(Size::fill())
                    .height(Size::fill())
                    .overflow(Overflow::Clip)
                    .child(RowList)
                    .children((0..desktop_gauntlet().tier.layers).map(|layer| LayerTilt { layer })),
            )
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
            .children((0..desktop_gauntlet().tickers.len()).map(|index| TickerTile { index }))
    }
}

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
                    .children(rows.map(|row| ListRow { row })),
            )
    }
}

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
                .children((first..first + tier.columns).map(|card| PostCard { card })),
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
        rect()
            .background(RenderCallback::new(move |context| {
                let RenderTarget::Background { canvas, .. } = &context.target else {
                    return;
                };
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
                canvas.draw_path(&area.detach(), &paint);
                let mut stroke = Paint::default();
                stroke.set_anti_alias(true);
                stroke.set_style(PaintStyle::Stroke);
                stroke.set_stroke_width(1.5 * s);
                stroke.set_color(color);
                canvas.draw_path(&line.detach(), &stroke);
            }))
            .width(Size::fill())
            .height(Size::px(36.0 * s))
    }
}

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

#[derive(PartialEq)]
struct LayerTilt {
    layer: usize,
}

impl Component for LayerTilt {
    fn render_key(&self) -> DiffKey {
        DiffKey::from(&self.layer)
    }

    fn render(&self) -> impl IntoElement {
        let shared = use_consume::<Shared>();
        let (layer, frame) = (self.layer, *shared.frame.read());
        rect()
            .layer(Layer::OverlayLevel((layer + 1) as u8))
            .position(
                Position::new_absolute()
                    .left(layer_x(layer, frame))
                    .top(layer_y(layer, frame)),
            )
            .rotation(layer_degrees(layer, frame))
            .width(Size::px(220.0))
            .padding(Gaps::new_all(10.0))
            .corner_radius(12.0)
            .background(LAYER_BACKGROUND)
            .child(LayerContent { layer })
    }
}

#[derive(PartialEq)]
struct LayerContent {
    layer: usize,
}

impl Component for LayerContent {
    fn render(&self) -> impl IntoElement {
        let layer = self.layer;
        let rows = (0..desktop_gauntlet().tier.layer_rows).map(|row| {
            rect().horizontal().spacing(3.0).children(
                (row * LAYER_COLUMNS..(row + 1) * LAYER_COLUMNS).map(|cell| {
                    rect()
                        .width(Size::px(22.0))
                        .height(Size::px(22.0))
                        .main_align(Alignment::Center)
                        .cross_align(Alignment::Center)
                        .corner_radius(4.0)
                        .background(palette(layer_cell_color(layer, cell)))
                        .child(text((cell + 1).to_string(), 9.0, Color::WHITE, false))
                }),
            )
        });
        rect()
            .width(Size::fill())
            .spacing(8.0)
            .child(text(
                format!("Layer {}", layer + 1),
                13.0,
                Color::WHITE,
                true,
            ))
            .child(rect().spacing(3.0).children(rows))
            .child(NestedTilt { layer })
    }
}

#[derive(PartialEq)]
struct NestedTilt {
    layer: usize,
}

impl Component for NestedTilt {
    fn render(&self) -> impl IntoElement {
        let shared = use_consume::<Shared>();
        let layer = self.layer;
        rect()
            .width(Size::fill())
            .rotation(-layer_degrees(layer, *shared.frame.read()))
            .padding(Gaps::new_all(8.0))
            .spacing(2.0)
            .corner_radius(8.0)
            .background(NESTED_BACKGROUND)
            .child(text(
                format!("Nested in layer {}", layer + 1),
                11.0,
                rgb(INK_RGB),
                true,
            ))
            .child(text("Tilts against its panel", 11.0, rgb(BODY_RGB), false))
    }
}

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
