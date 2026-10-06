//! The gauntlet in Floem: the screen `compose-app/.../Gauntlet.kt` draws,
//! element for element, in Floem's own idiom: views built once, a signal for
//! the frame, and labels and style closures that run again when a signal they
//! read changes. Taffy lays the views out, flow rows wrap, and a view of its
//! own paints each sparkline. The benchmark's README describes it;
//! `desktop.py` runs it: `PERF_TIER=16 PERF_FONTS=../fonts cargo run
//! --release`.
//!
//! Every frame advances a frame index and everything follows from it, never
//! from wall time. The frame view schedules the next index each time it is
//! painted, so the index advances once per frame drawn. Floem's virtual list
//! wants every row's height up front, so the list renders the rows from the
//! first one on screen down to the window's bottom, and each row reports its
//! height once Taffy has laid it out.

use std::{collections::HashMap, io::Cursor, sync::OnceLock, time::Duration};

use floem::{
    IntoView, View, ViewId,
    action::exec_after,
    context::PaintCx,
    kurbo::{self, BezPath, Point, Stroke},
    peniko::{Color, Gradient},
    reactive::{RwSignal, SignalGet, SignalTrack, SignalUpdate, SignalWith, create_effect},
    style::{FlexWrap, TextOverflow},
    text::{Attrs, AttrsList, FONT_SYSTEM, FamilyOwned, LineHeightValue, TextLayout, Weight},
    views::{
        Decorators, clip, container, dyn_stack, empty, h_stack, h_stack_from_iter, img, label,
        rich_text, v_stack,
    },
    window::WindowConfig,
};
use floem_renderer::Renderer;
use perf_data::*;

const FOOTER_LABELS: [&str; 3] = ["likes", "replies", "shares"];
const ROBOTO: &str = "Roboto";

fn rgb(value: [u8; 3]) -> Color {
    Color::rgb8(value[0], value[1], value[2])
}

fn palette(index: usize) -> Color {
    rgb(PALETTE_RGB[index % PALETTE_RGB.len()])
}

/// The avatars as PNG, the form Floem's image view decodes, made once.
fn avatars() -> &'static [Vec<u8>] {
    static AVATARS: OnceLock<Vec<Vec<u8>>> = OnceLock::new();
    AVATARS.get_or_init(|| (0..AVATAR_COUNT).map(avatar_png).collect())
}

fn avatar_png(index: usize) -> Vec<u8> {
    let mut png = Vec::new();
    let encoded = image::RgbaImage::from_raw(AVATAR_SIZE, AVATAR_SIZE, avatar_rgba(index))
        .map(|avatar| avatar.write_to(&mut Cursor::new(&mut png), image::ImageFormat::Png));
    if !matches!(encoded, Some(Ok(()))) {
        log::warn!("avatar {index} did not encode");
    }
    png
}

fn s() -> f64 {
    f64::from(desktop_gauntlet().tier.scale)
}

/// The signals the screen reads, which the frame view advances.
#[derive(Clone, Copy)]
struct Shared {
    frame: RwSignal<u32>,
    /// Where the list starts.
    anchor: RwSignal<ListAnchor>,
    /// The heights rows reported, from the anchor down.
    heights: RwSignal<HashMap<usize, f32>>,
}

/// Text in Roboto at `size`, lines 1.4 em apart.
fn text(
    content: impl std::fmt::Display + 'static,
    size: f64,
    color: Color,
    bold: bool,
) -> impl IntoView {
    let content = content.to_string();
    label(move || content.clone()).style(move |style| {
        style
            .font_family(ROBOTO.to_string())
            .font_size(size as f32)
            .color(color)
            .font_weight(if bold { Weight::BOLD } else { Weight::NORMAL })
            .line_height(1.4)
            .text_overflow(TextOverflow::Clip)
    })
}

/// A paragraph cut after `lines` lines: Floem labels have no line limit, so
/// a box as tall as those lines clips the rest.
fn paragraph(content: String, size: f64, color: Color, bold: bool, lines: usize) -> impl IntoView {
    clip(label(move || content.clone()).style(move |style| {
        style
            .font_family(ROBOTO.to_string())
            .font_size(size as f32)
            .color(color)
            .font_weight(if bold { Weight::BOLD } else { Weight::NORMAL })
            .line_height(1.4)
            .text_overflow(TextOverflow::Wrap)
            .width_full()
    }))
    .style(move |style| style.width_full().max_height(size * 1.4 * lines as f64))
}

/// A rounded track filled to the share `share` reads, in `color`.
fn bar(
    share: impl Fn() -> f64 + 'static,
    color: Color,
    width: Option<f64>,
    height: f64,
) -> impl IntoView {
    container(empty().style(move |style| {
        style
            .width_pct(share().clamp(0.0, 1.0) * 100.0)
            .height_full()
            .border_radius(height / 2.0)
            .background(color)
    }))
    .style(move |style| {
        let style = style
            .height(height)
            .border_radius(height / 2.0)
            .background(rgb(HAIRLINE_RGB));
        match width {
            Some(width) => style.width(width),
            None => style.flex_grow(1.0_f32).flex_basis(0.0),
        }
    })
}

/// Moves the anchor past the rows that scrolled off above on `frame`.
fn advance(shared: Shared, frame: u32) {
    let gap = 8.0 * desktop_gauntlet().tier.scale;
    let first = shared.anchor.get_untracked();
    let anchor = shared.heights.with_untracked(|heights| {
        first.advanced(frame as f32 * SCROLL_PER_FRAME, gap, |row| {
            heights.get(&row).copied()
        })
    });
    if anchor != first {
        shared.anchor.set(anchor);
        shared
            .heights
            .update(|heights| heights.retain(|measured, _| *measured >= anchor.row));
    }
}

/// Advances the frame index each time it is painted: the next index waits for
/// this frame to be drawn.
struct FrameView {
    id: ViewId,
    shared: Shared,
}

impl View for FrameView {
    fn id(&self) -> ViewId {
        self.id
    }

    fn paint(&mut self, _cx: &mut PaintCx) {
        let shared = self.shared;
        let freeze = desktop_gauntlet().launch.freeze;
        let frame = shared.frame.get_untracked();
        if freeze > 0 && frame >= freeze {
            return;
        }
        exec_after(Duration::ZERO, move |_| {
            let next = shared.frame.get_untracked() + 1;
            if next == 1 {
                log::info!("PERF first_frame");
            }
            advance(shared, next);
            shared.frame.set(next);
            if freeze > 0 && next >= freeze {
                log::info!("PERF frozen frame={next}");
            }
        });
    }
}

fn frame_view(shared: Shared) -> FrameView {
    let id = ViewId::new();
    create_effect(move |_| {
        shared.frame.track();
        id.request_paint();
    });
    FrameView { id, shared }
}

/// A card's 48-point line over a fading fill, painted from the frame.
struct Sparkline {
    id: ViewId,
    card: usize,
    color: Color,
    frame: RwSignal<u32>,
}

impl View for Sparkline {
    fn id(&self) -> ViewId {
        self.id
    }

    fn paint(&mut self, cx: &mut PaintCx) {
        let Some(size) = self.id.get_size() else {
            return;
        };
        let frame = self.frame.get_untracked();
        let (width, height) = (size.width, size.height);
        let step = width / (GAUNTLET_SPARK_POINTS - 1) as f64;
        let point = |index: usize| {
            Point::new(
                index as f64 * step,
                height * f64::from(1.0 - spark_value(self.card, index, frame)),
            )
        };
        let mut area = BezPath::new();
        area.move_to((0.0, height));
        let mut line = BezPath::new();
        line.move_to(point(0));
        for index in 0..GAUNTLET_SPARK_POINTS {
            area.line_to(point(index));
            if index > 0 {
                line.line_to(point(index));
            }
        }
        area.line_to((width, height));
        area.close_path();
        // vger takes a linear gradient's points in window pixels, each
        // multiplied by its stop's offset: both stops sit at 1.
        let top = self.id.layout_rect().y0 * cx.scale();
        let fade = Gradient::new_linear((0.0, top), (0.0, top + height * cx.scale())).with_stops([
            (1.0, self.color.multiply_alpha(0.25)),
            (1.0, self.color.multiply_alpha(0.0)),
        ]);
        cx.fill(&area, &fade, 0.0);
        cx.stroke(&line, self.color, &Stroke::new(1.5 * s()));
    }
}

fn sparkline(card: usize, color: Color, frame: RwSignal<u32>) -> Sparkline {
    let id = ViewId::new();
    create_effect(move |_| {
        frame.track();
        id.request_paint();
    });
    Sparkline {
        id,
        card,
        color,
        frame,
    }
}

/// The card's white over an avatar's corners: vger clips only to rectangles,
/// so this rounds the avatar.
struct Corners {
    id: ViewId,
}

impl View for Corners {
    fn id(&self) -> ViewId {
        self.id
    }

    fn paint(&mut self, cx: &mut PaintCx) {
        let Some(size) = self.id.get_size() else {
            return;
        };
        // vger fills no holes: each corner is a shape of its own, from the
        // corner along the circle's quarter between its two edges.
        let radius = size.width / 2.0;
        let center = Point::new(radius, size.height / 2.0);
        for (corner, start) in [
            (Point::new(0.0, 0.0), std::f64::consts::PI),
            (Point::new(size.width, 0.0), 1.5 * std::f64::consts::PI),
            (Point::new(size.width, size.height), 0.0),
            (Point::new(0.0, size.height), 0.5 * std::f64::consts::PI),
        ] {
            let mut shape = BezPath::new();
            shape.move_to(corner);
            for step in 0..=8 {
                let angle = start + std::f64::consts::FRAC_PI_2 * f64::from(step) / 8.0;
                shape.line_to(center + radius * kurbo::Vec2::new(angle.cos(), angle.sin()));
            }
            shape.close_path();
            cx.fill(&shape, Color::WHITE, 0.0);
        }
    }
}

fn app_view() -> impl IntoView {
    let s = s();
    let shared = Shared {
        frame: RwSignal::new(0),
        anchor: RwSignal::new(ListAnchor {
            row: 0,
            top: (8.0 * s) as f32,
        }),
        heights: RwSignal::new(HashMap::new()),
    };
    let (window_width, _) = DESKTOP_WINDOW;
    let content = v_stack((ticker_panel(shared), list(shared))).style(move |style| {
        style
            .width(
                (f64::from(window_width) * f64::from(width_fraction(shared.frame.get()))).round(),
            )
            .flex_grow(1.0_f32)
            .min_height(0.0)
    });
    let top_bar = container(text("Gauntlet", 20.0, Color::WHITE, true)).style(|style| {
        style
            .width_full()
            .height(56.0)
            .padding_horiz(16.0)
            .items_center()
            .background(rgb(TOP_BAR_RGB))
    });
    v_stack((
        top_bar,
        content,
        frame_view(shared).style(|style| style.size(1.0, 1.0)),
    ))
    .style(|style| style.size_full().background(rgb(BACKGROUND_RGB)))
}

fn ticker_panel(shared: Shared) -> impl IntoView {
    let s = s();
    h_stack_from_iter(
        (0..desktop_gauntlet().tickers.len()).map(move |index| ticker_tile(index, shared)),
    )
    .style(move |style| {
        style
            .width_full()
            .flex_wrap(FlexWrap::Wrap)
            .gap(4.0 * s)
            .padding(6.0 * s)
            .background(rgb(PANEL_RGB))
    })
}

/// One quote: symbol, price, change and a bar, all from the frame.
fn ticker_tile(index: usize, shared: Shared) -> impl IntoView {
    let s = s();
    let ticker = &desktop_gauntlet().tickers[index];
    let frame = shared.frame;
    let cents = move || ticker_cents(ticker, frame.get());
    let style = move |color: Color| {
        move |style: floem::style::Style| {
            style
                .font_family(ROBOTO.to_string())
                .font_size((10.0 * s) as f32)
                .color(color)
                .line_height(1.4)
        }
    };
    let up = rgb(UP_RGB);
    let down = rgb(DOWN_RGB);
    h_stack((
        text(ticker.symbol.clone(), 10.0 * s, rgb(INK_RGB), true),
        label(move || cents_text(cents())).style(style(rgb(BODY_RGB))),
        label(move || change_text(ticker, cents())).style(move |style_value| {
            let color = if cents() >= ticker.base_cents {
                up
            } else {
                down
            };
            style(color)(style_value)
        }),
        bar(
            move || {
                f64::from(cents() - ticker.base_cents + ticker.swing_cents)
                    / f64::from(2 * ticker.swing_cents)
            },
            palette(index),
            Some(20.0 * s),
            4.0 * s,
        ),
    ))
    .style(move |style| {
        style
            .items_center()
            .gap(4.0 * s)
            .padding_vert(3.0 * s)
            .padding_horiz(6.0 * s)
            .border_radius(6.0 * s)
            .background(Color::WHITE)
    })
}

/// The rows from the anchor down to the bottom of the window, moved up by
/// the frame's offset.
fn list(shared: Shared) -> impl IntoView {
    let s = s();
    let gap = 8.0 * s;
    let rows = move || {
        let offset = shared.frame.get() as f32 * SCROLL_PER_FRAME;
        let anchor = shared.anchor.get();
        let (_, window_height) = DESKTOP_WINDOW;
        shared.heights.with_untracked(|heights| {
            anchor.rows(
                offset,
                window_height as f32,
                gap as f32,
                (10.0 * s) as f32,
                |row| heights.get(&row).copied(),
            )
        })
    };
    clip(
        dyn_stack(rows, |row| *row, move |row| list_row(row, shared)).style(move |style| {
            let offset = shared.frame.get() as f32 * SCROLL_PER_FRAME;
            style
                .flex_col()
                .width_full()
                .gap(gap)
                .padding_horiz(gap)
                .margin_top(f64::from(shared.anchor.get().top - offset))
        }),
    )
    .style(|style| style.width_full().flex_grow(1.0_f32).min_height(0.0))
}

/// A list row: cards side by side, or a cluster. It reports its height.
fn list_row(row: usize, shared: Shared) -> impl IntoView {
    let tier = desktop_gauntlet().tier;
    let content = match gauntlet_row(row, tier.columns) {
        GauntletRow::Cards(first) => h_stack_from_iter(
            (first..first + tier.columns).map(move |card| post_card(card, shared)),
        )
        .style(move |style| {
            style
                .width_full()
                .items_start()
                .gap(8.0 * f64::from(tier.scale))
        })
        .into_any(),
        GauntletRow::Cluster(cluster) => level(cluster, tier.depth).into_any(),
    };
    container(content)
        .style(|style| style.width_full())
        .on_resize(move |rect| {
            shared.heights.update(|heights| {
                heights.insert(row, rect.height() as f32);
            });
        })
}

/// `by Ada Lovelace · #rust`: the author bold, the tag in its color.
fn subtitle(post: &Post) -> impl IntoView {
    let s = s();
    let (tag, tag_color) = &post.tags[0];
    let line = format!("by {} · {tag}", post.author);
    let family = [FamilyOwned::Name(ROBOTO.to_string())];
    let base = Attrs::new()
        .family(&family)
        .font_size((11.0 * s) as f32)
        .color(rgb(MUTED_RGB))
        .line_height(LineHeightValue::Normal(1.4));
    let mut spans = AttrsList::new(base);
    let author_start = "by ".len();
    spans.add_span(
        author_start..author_start + post.author.len(),
        base.weight(Weight::BOLD),
    );
    spans.add_span(
        line.len() - tag.len()..line.len(),
        base.color(rgb(GRADIENT_END_RGB[*tag_color])),
    );
    let mut layout = TextLayout::new();
    layout.set_text(&line, spans);
    clip(rich_text(move || layout.clone()))
        .style(move |style| style.width_full().height(11.0 * s * 1.4))
}

/// The avatar beside the title and the subtitle.
fn card_header(card: usize, post: &'static Post) -> impl IntoView {
    let s = s();
    let avatar = container((
        img({
            let png = avatars()[card % AVATAR_COUNT].clone();
            move || png.clone()
        })
        .style(move |style| style.size(32.0 * s, 32.0 * s)),
        Corners { id: ViewId::new() }.style(move |style| style.absolute().size(32.0 * s, 32.0 * s)),
    ))
    .style(move |style| style.size(32.0 * s, 32.0 * s));
    h_stack((
        avatar,
        v_stack((
            paragraph(post.title.clone(), 13.0 * s, rgb(INK_RGB), true, 2),
            subtitle(post),
        ))
        .style(|style| style.flex_grow(1.0_f32).flex_basis(0.0).min_width(0.0)),
    ))
    .style(move |style| style.width_full().items_center().gap(8.0 * s))
}

/// The bar and the percent, from the frame.
fn card_progress(card: usize, frame: RwSignal<u32>) -> impl IntoView {
    let s = s();
    h_stack((
        bar(
            move || f64::from(progress_permille(card, frame.get())) / 1000.0,
            palette(card),
            None,
            6.0 * s,
        ),
        label(move || format!("{}%", progress_permille(card, frame.get()) / 10)).style(
            move |style| {
                style
                    .font_family(ROBOTO.to_string())
                    .font_size((10.0 * s) as f32)
                    .color(rgb(MUTED_RGB))
                    .line_height(1.4)
            },
        ),
    ))
    .style(move |style| style.width_full().items_center().gap(6.0 * s))
}

fn card_chips(post: &'static Post) -> impl IntoView {
    let s = s();
    h_stack_from_iter(post.tags.iter().map(move |(chip, color)| {
        container(text(
            chip.clone(),
            10.0 * s,
            rgb(GRADIENT_END_RGB[*color]),
            false,
        ))
        .style(move |style| {
            style
                .padding_vert(3.0 * s)
                .padding_horiz(8.0 * s)
                .border_radius(10.0 * s)
                .background(rgb(CHIP_BACKGROUND_RGB[*color]))
        })
    }))
    .style(move |style| style.width_full().flex_wrap(FlexWrap::Wrap).gap(4.0 * s))
}

/// Three counters split by dividers as tall as the row: a flex row stretches
/// its items to its height.
fn card_footer(post: &'static Post) -> impl IntoView {
    let s = s();
    h_stack_from_iter(FOOTER_LABELS.iter().enumerate().map(move |(index, name)| {
        let counter = v_stack((
            text(post.stats[index].clone(), 12.0 * s, rgb(INK_RGB), true),
            text(*name, 9.0 * s, rgb(MUTED_RGB), false),
        ))
        .style(|style| style.flex_grow(1.0_f32).flex_basis(0.0).items_center());
        if index == 0 {
            counter.into_any()
        } else {
            h_stack((
                empty().style(|style| style.width(1.0).background(rgb(HAIRLINE_RGB))),
                counter,
            ))
            .style(|style| style.flex_grow(1.0_f32).flex_basis(0.0))
            .into_any()
        }
    }))
    .style(|style| style.width_full())
}

/// A translucent tag tilting with the frame: rotated, never laid out again.
fn hot_badge(card: usize, frame: RwSignal<u32>) -> impl IntoView {
    let s = s();
    container(text("HOT", 9.0 * s, Color::WHITE, true)).style(move |style| {
        style
            .absolute()
            .inset_top(6.0 * s)
            .inset_right(6.0 * s)
            .padding_vert(2.0 * s)
            .padding_horiz(6.0 * s)
            .border_radius(8.0 * s)
            .background(palette(0).multiply_alpha(0.9))
            .rotate(f64::from(badge_degrees(card, frame.get())).to_radians())
    })
}

fn post_card(card: usize, shared: Shared) -> impl IntoView {
    let s = s();
    let post = &desktop_gauntlet().posts[card % POST_COUNT];
    let frame = shared.frame;
    let body_view = v_stack((
        card_header(card, post),
        paragraph(post.body.clone(), 12.0 * s, rgb(BODY_RGB), false, 4),
        card_progress(card, frame),
        sparkline(card, palette(post.color), frame)
            .style(move |style| style.width_full().height(36.0 * s)),
        card_chips(post),
        card_footer(post),
    ))
    .style(move |style| {
        style
            .width_full()
            .gap(6.0 * s)
            .padding(10.0 * s)
            .border_radius(12.0 * s)
            .background(Color::WHITE)
            .border(1.0)
            .border_color(rgb(HAIRLINE_RGB))
            .box_shadow_blur(3.0 * s)
            .box_shadow_v_offset(s)
            .box_shadow_color(Color::rgba8(0, 0, 0, 61))
    });
    // The card takes its share of the row; the badge sits over its corner.
    let card_style =
        |style: floem::style::Style| style.flex_grow(1.0_f32).flex_basis(0.0).min_width(0.0);
    if card.is_multiple_of(5) {
        h_stack((body_view, hot_badge(card, frame)))
            .style(card_style)
            .into_any()
    } else {
        h_stack((body_view,)).style(card_style).into_any()
    }
}

/// Levels nested inside one another, each a row of three labels above the
/// next level.
fn level(cluster: usize, remaining: usize) -> impl IntoView {
    let s = s();
    let labels = h_stack_from_iter((0..3).map(move |chip| {
        container(text(
            format!("C{cluster}.L{remaining}.{chip}"),
            9.0 * s,
            Color::WHITE,
            false,
        ))
        .style(move |style| {
            style
                .flex_grow(1.0_f32)
                .flex_basis(0.0)
                .min_width(0.0)
                .border_radius(2.0 * s)
                .background(palette(remaining + chip + cluster))
        })
    }))
    .style(move |style| style.width_full().items_start().gap(2.0 * s));
    let background = rgb(LEVEL_BACKGROUND_RGB[remaining % 2]);
    let children = if remaining > 0 {
        v_stack((labels, level(cluster, remaining - 1)))
            .style(move |style| style.width_full().gap(s))
            .into_any()
    } else {
        v_stack((labels,))
            .style(|style| style.width_full())
            .into_any()
    };
    container(children).style(move |style| {
        style
            .width_full()
            .padding_top(s)
            .padding_right(s)
            .padding_bottom(s)
            .padding_left(3.0 * s)
            .border_radius(4.0 * s)
            .background(background)
    })
}

fn main() {
    perf_data::log_to_stdout();
    {
        let mut fonts = FONT_SYSTEM.lock();
        for file in ["Roboto-Regular.ttf", "Roboto-Bold.ttf"] {
            match std::fs::read(font_path(file)) {
                Ok(bytes) => fonts.db_mut().load_font_data(bytes),
                Err(error) => log::warn!("no {file}: {error}"),
            }
        }
    }
    let (width, height) = DESKTOP_WINDOW;
    floem::Application::new()
        .window(
            |_| app_view(),
            Some(
                WindowConfig::default()
                    .size((f64::from(width), f64::from(height)))
                    .title("Gauntlet")
                    .resizable(false),
            ),
        )
        .run();
}
