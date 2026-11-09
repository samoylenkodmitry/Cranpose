//! The gauntlet in zgui (`../README.md`): every element is built once, and
//! what a frame changes follows one frame signal: the ticker text, the bar
//! widths, the sparklines, the tilts, the list's offset and the content's
//! width. The list holds the rows from the one at the top of the window to its
//! bottom, kept by key as the window moves; each row's node reports its
//! height once laid out.

use std::{cell::RefCell, collections::HashMap, ops::Range, rc::Rc};

use perf_data::*;
use zgui::{
    canvas::{Brush, ShapeBuilder, zgui_color::Color},
    elements::{
        kurbo, {self as e},
    },
    prelude::*,
    view::{AnyView, ForProps, NodeRef},
};
use zgui_image::ImageBytes;

use crate::run_frames;

const FOOTER_LABELS: [&str; 3] = ["likes", "replies", "shares"];
const UP: &str = "#16A34A";
const DOWN: &str = "#DC2626";

/// What every frame advances and the pieces read.
#[derive(Clone)]
struct Shared {
    frame: RwSignal<u32>,
    anchor: RwSignal<ListAnchor>,
    /// The rows the list holds: from the anchor down to the window's bottom.
    window: RwSignal<Range<usize>>,
    /// The nodes of the rows, which report their heights.
    rows: Rc<RefCell<HashMap<usize, NodeRef>>>,
    avatars: Rc<Vec<ImageBytes>>,
}

impl Shared {
    fn new() -> Self {
        Self {
            frame: RwSignal::new(0),
            anchor: RwSignal::new(ListAnchor {
                row: 0,
                top: 8.0 * desktop_gauntlet().tier.scale,
            }),
            window: RwSignal::new(0..10),
            rows: Rc::default(),
            avatars: Rc::new((0..AVATAR_COUNT).map(avatar).collect()),
        }
    }

    /// The height of `row` in points, once it is laid out.
    fn height(&self, row: usize) -> Option<f32> {
        let rows = self.rows.borrow();
        let node = rows.get(&row)?;
        let bounds = node.bounds()?;
        Some(bounds.size.height.0 / node.scale())
    }

    /// The average height of the rows laid out.
    fn average_height(&self, none: f32) -> f32 {
        let rows = self.rows.borrow();
        let (sum, count) = rows
            .keys()
            .filter_map(|row| self.height(*row))
            .fold((0.0, 0), |(sum, count), height| (sum + height, count + 1));
        if count == 0 { none } else { sum / count as f32 }
    }

    /// Moves to the next frame; `false` once the freeze frame is reached.
    fn step(&self, freeze: u32) -> bool {
        let s = desktop_gauntlet().tier.scale;
        let frame = self.frame.get_untracked() + 1;
        if frame == 1 {
            log::info!("PERF first_frame");
        }
        let offset = scroll_offset(frame);
        let anchor = self.anchor.get_untracked();
        let advanced = anchor.advanced(offset, 8.0 * s, |row| self.height(row));
        if advanced != anchor {
            self.rows
                .borrow_mut()
                .retain(|measured, _| *measured >= advanced.row);
            self.anchor.set(advanced);
        }
        // A row not laid out yet counts as the average of those that are.
        let (_, window_height) = DESKTOP_WINDOW;
        let needed = advanced.rows(
            offset,
            window_height as f32,
            8.0 * s,
            self.average_height(100.0 * s),
            |row| self.height(row),
        );
        if needed != self.window.get_untracked() {
            self.window.set(needed);
        }
        self.frame.set(frame);
        if freeze > 0 && frame >= freeze {
            log::info!("PERF frozen frame={frame}");
            return false;
        }
        true
    }
}

/// How far `frame` has scrolled the list.
fn scroll_offset(frame: u32) -> f32 {
    frame as f32 * SCROLL_PER_FRAME
}

/// The avatar as the PNG file the `image` element names it by.
fn avatar(index: usize) -> ImageBytes {
    let mut file = Vec::new();
    let mut encoder = png::Encoder::new(&mut file, AVATAR_SIZE, AVATAR_SIZE);
    encoder.set_color(png::ColorType::Rgba);
    encoder.set_depth(png::BitDepth::Eight);
    let mut writer = encoder.write_header().expect("a PNG header");
    writer
        .write_image_data(&avatar_rgba(index))
        .expect("the avatar's pixels");
    writer.finish().expect("a finished PNG");
    ImageBytes::new(file)
}

pub(crate) fn screen() -> AnyView {
    let tier = desktop_gauntlet().tier;
    let (width, _) = DESKTOP_WINDOW;
    let shared = Shared::new();
    let frame = shared.frame;
    let freeze = perf_data::launch().freeze;
    let step = shared.clone();
    run_frames(move || step.step(freeze));
    let anchor = shared.anchor;
    let list = e::column()
        .class("list")
        .style_property("transform", move || {
            let offset = scroll_offset(frame.get());
            Some(format!("translateY({}px)", anchor.get().top - offset))
        })
        .child(rows(shared));
    let area = e::r#box()
        .class("area")
        .child(list)
        .children((0..tier.layers).map(|layer| AnyView::new(stacked_layer(layer, frame))));
    let content = e::column()
        .class("content")
        .style_property("width", move || {
            let width = (width as f32 * width_fraction(frame.get())).round();
            Some(format!("{width}px"))
        })
        .child(ticker_panel(frame))
        .child(area);
    AnyView::new(
        e::column()
            .class("page")
            .child(
                e::column()
                    .class("top")
                    .child(e::text().class("top-title").child("Gauntlet")),
            )
            .child(content),
    )
}

fn rows(shared: Shared) -> impl IntoView {
    let window = shared.window;
    ForProps::builder()
        .each(move || window.get().collect::<Vec<_>>())
        .key(|row: &usize| *row)
        .children(move |row| row_view(row, &shared))
        .build()
        .render()
}

fn row_view(row: usize, shared: &Shared) -> impl IntoView + use<> {
    let tier = desktop_gauntlet().tier;
    let content = match gauntlet_row(row, tier.columns) {
        GauntletRow::Cards(first) => AnyView::new(
            e::row()
                .class("cards")
                .children((first..first + tier.columns).map(|card| card_view(card, shared))),
        ),
        GauntletRow::Cluster(cluster) => level(cluster, tier.depth),
    };
    let node = NodeRef::new();
    shared.rows.borrow_mut().insert(row, node.clone());
    e::column().node_ref(node).child(content)
}

fn card_view(card: usize, shared: &Shared) -> AnyView {
    let frame = shared.frame;
    let post = &desktop_gauntlet().posts[card % POST_COUNT];
    let body = e::column()
        .class("card")
        .child(card_header(card, post, shared))
        .child(e::text().class("body").child(post.body.clone()))
        .child(card_chips(post))
        .child(card_progress(card, frame))
        .child(sparkline(card, post.color, frame))
        .child(card_footer(post));
    let cell = e::column().class("cell").child(body);
    if card.is_multiple_of(5) {
        // A translucent tag tilting with the frame: turned, never laid out
        // again.
        AnyView::new(cell.child(card_badge(card, frame)))
    } else {
        AnyView::new(cell)
    }
}

/// The avatar, the title and the subtitle.
fn card_header(card: usize, post: &Post, shared: &Shared) -> impl IntoView + use<> {
    let (tag, tag_color) = &post.tags[0];
    // `by Ada Lovelace · #rust`: the author bold, the tag in its color.
    let subtitle = e::text()
        .class("subtitle")
        .child("by ")
        .child(e::text().class("bold").child(post.author.clone()))
        .child(" · ")
        .child(e::text().class(format!("g{tag_color}")).child(tag.clone()));
    let avatar = e::image()
        .class("avatar")
        .src(Some(shared.avatars[card % AVATAR_COUNT].url()));
    e::row().class("head").child(avatar).child(
        e::column()
            .class("head-text")
            .child(e::text().class("title").child(post.title.clone()))
            .child(subtitle),
    )
}

fn card_chips(post: &Post) -> impl IntoView + use<> {
    e::row()
        .class("chips")
        .children(post.tags.iter().map(|(value, shade)| {
            AnyView::new(
                e::text()
                    .class(format!("chip chip{shade}"))
                    .child(value.clone()),
            )
        }))
}

/// A track with a fill over `share` of its width.
fn bar(track: &'static str, fill: String, share: impl Fn() -> f32 + 'static) -> impl IntoView {
    e::r#box()
        .class(track)
        .child(e::r#box().class(fill).style_property("width", move || {
            Some(format!("{}%", share().clamp(0.0, 1.0) * 100.0))
        }))
}

fn card_progress(card: usize, frame: RwSignal<u32>) -> impl IntoView {
    let permille = move || progress_permille(card, frame.get());
    e::row()
        .class("progress")
        .child(bar(
            "track track-card",
            format!("fill-card p{}", card % PALETTE_RGB.len()),
            move || permille() as f32 / 1000.0,
        ))
        .child(
            e::text()
                .class("percent")
                .child(move || format!("{}%", permille() / 10)),
        )
}

/// A card's 48-point sparkline: the fading fill is a gradient under a white
/// polygon above the line.
fn sparkline(card: usize, color: usize, frame: RwSignal<u32>) -> impl IntoView {
    let s = f64::from(desktop_gauntlet().tier.scale);
    let index = color % PALETTE_RGB.len();
    let [red, green, blue] = PALETTE_RGB[index];
    let stroke = Brush::Solid(Color::srgb_u8(red, green, blue, 255));
    e::canvas()
        .class(format!("spark s{index}"))
        .draw(move |cx| {
            let (width, height) = (f64::from(cx.size.width.0), f64::from(cx.size.height.0));
            let frame = frame.get();
            let step = width / (GAUNTLET_SPARK_POINTS - 1) as f64;
            let line = (0..GAUNTLET_SPARK_POINTS)
                .map(|point| {
                    let value = f64::from(spark_value(card, point, frame));
                    kurbo::Point::new(point as f64 * step, height * (1.0 - value))
                })
                .collect::<Vec<_>>();
            let mut above = kurbo::BezPath::new();
            above.move_to((0.0, 0.0));
            above.line_to((width, 0.0));
            for point in line.iter().rev() {
                above.line_to(*point);
            }
            above.close_path();
            let mut stroked = kurbo::BezPath::new();
            stroked.move_to(line[0]);
            for point in &line[1..] {
                stroked.line_to(*point);
            }
            cx.scene.push(
                ShapeBuilder::new(above)
                    .fill(Brush::Solid(Color::WHITE))
                    .build(),
            );
            cx.scene.push(
                ShapeBuilder::new(stroked)
                    .stroke(stroke.clone(), 1.5 * s)
                    .build(),
            );
        })
}

/// Three counters split by dividers as tall as the tallest counter.
fn card_footer(post: &Post) -> impl IntoView + use<> {
    let mut footer = e::row().class("stats");
    for (index, label) in FOOTER_LABELS.iter().enumerate() {
        if index > 0 {
            footer = footer.child(e::r#box().class("divider"));
        }
        footer = footer.child(
            e::column()
                .class("stat")
                .child(
                    e::text()
                        .class("stat-value")
                        .child(post.stats[index].clone()),
                )
                .child(e::text().class("stat-label").child(*label)),
        );
    }
    footer
}

fn card_badge(card: usize, frame: RwSignal<u32>) -> impl IntoView {
    e::r#box()
        .class("badge")
        .style_property("transform", move || {
            Some(format!("rotate({}deg)", badge_degrees(card, frame.get())))
        })
        .child(e::text().class("badge-text").child("HOT"))
}

/// Levels nested inside one another, each a row of three labels above the
/// next level.
fn level(cluster: usize, remaining: usize) -> AnyView {
    let chips = (0..3).map(|chip| {
        let shade = (remaining + chip + cluster) % PALETTE_RGB.len();
        AnyView::new(
            e::r#box()
                .class(format!("level-chip p{shade}"))
                .child(format!("C{cluster}.L{remaining}.{chip}")),
        )
    });
    let mut level_box = e::column()
        .class(format!("level lv{}", remaining % 2))
        .child(e::row().class("level-row").children(chips));
    if remaining > 0 {
        level_box = level_box.child(level(cluster, remaining - 1));
    }
    AnyView::new(level_box)
}

fn ticker_panel(frame: RwSignal<u32>) -> impl IntoView {
    e::row().class("tickers").children(
        desktop_gauntlet()
            .tickers
            .iter()
            .enumerate()
            .map(|(index, ticker)| AnyView::new(ticker_tile(index, ticker, frame))),
    )
}

/// One quote: symbol, price, change and a bar, all from the frame.
fn ticker_tile(index: usize, ticker: &'static Ticker, frame: RwSignal<u32>) -> impl IntoView {
    let cents = move || ticker_cents(ticker, frame.get());
    let share = move || {
        (cents() - ticker.base_cents + ticker.swing_cents) as f32 / (2 * ticker.swing_cents) as f32
    };
    e::row()
        .class("tick")
        .child(e::text().class("tick-symbol").child(ticker.symbol.clone()))
        .child(
            e::text()
                .class("tick-text")
                .child(move || cents_text(cents())),
        )
        .child(
            e::text()
                .class("tick-change")
                .style_property("color", move || {
                    let color = if cents() >= ticker.base_cents {
                        UP
                    } else {
                        DOWN
                    };
                    Some(color.to_owned())
                })
                .child(move || change_text(ticker, cents())),
        )
        .child(bar(
            "track track-tick",
            format!("fill-tick p{}", index % PALETTE_RGB.len()),
            share,
        ))
}

/// A translucent panel stacked over the list. Its content never changes: each
/// frame turns and moves its box, and turns its nested card back.
fn stacked_layer(layer: usize, frame: RwSignal<u32>) -> impl IntoView {
    let rows = desktop_gauntlet().tier.layer_rows;
    let cells = (0..rows).map(|line| {
        AnyView::new(e::row().class("layer-line").children(
            (line * LAYER_COLUMNS..(line + 1) * LAYER_COLUMNS).map(move |cell| {
                let shade = layer_cell_color(layer, cell) % PALETTE_RGB.len();
                AnyView::new(
                    e::r#box()
                        .class(format!("layer-cell p{shade}"))
                        .child((cell + 1).to_string()),
                )
            }),
        ))
    });
    let nested = e::column()
        .class("nested")
        .style_property("transform", move || {
            Some(format!("rotate({}deg)", -layer_degrees(layer, frame.get())))
        })
        .child(
            e::text()
                .class("nested-title")
                .child(format!("Nested in layer {}", layer + 1)),
        )
        .child(
            e::text()
                .class("nested-text")
                .child("Tilts against its panel"),
        );
    e::column()
        .class("layer")
        .style_property("transform", move || {
            let frame = frame.get();
            Some(format!(
                "translate({}px, {}px) rotate({}deg)",
                layer_x(layer, frame),
                layer_y(layer, frame),
                layer_degrees(layer, frame),
            ))
        })
        .child(
            e::text()
                .class("layer-title")
                .child(format!("Layer {}", layer + 1)),
        )
        .child(e::column().class("layer-cells").children(cells))
        .child(nested)
}
