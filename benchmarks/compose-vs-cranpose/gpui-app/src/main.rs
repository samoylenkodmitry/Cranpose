//! The gauntlet in GPUI: the screen `compose-app/.../Gauntlet.kt` and
//! `cranpose-app/src/screens/gauntlet.rs` draw, element for element, in
//! GPUI's own idiom. The benchmark's README describes it; `desktop.py` runs
//! it: `PERF_TIER=12 PERF_FONTS=../fonts cargo run --release`.
//!
//! Every frame advances a frame index and everything follows from it, never
//! from wall time. The view renders once per frame, advances the index and
//! asks for the next frame; the list is GPUI's own `list`, which lays out
//! only the rows on screen.

use std::{borrow::Cow, sync::Arc};

use gpui::{
    AnyElement, App, AppContext, Background, Bounds, BoxShadow, Context, FontWeight, Hsla,
    ImageSource, IntoElement, ListAlignment, ListOffset, ListState, ParentElement, PathBuilder,
    Pixels, Point, Render, RenderImage, Rgba, SharedString, Styled, TitlebarOptions, Window,
    WindowBounds, WindowOptions, canvas, div, img, linear_color_stop, linear_gradient, list, point,
    prelude::FluentBuilder, px, relative, size,
};
use perf_data::*;

/// Blocks of five card rows and a cluster: no measurement window reaches the end.
const ROWS: usize = 2000 * (CARD_ROWS_PER_CLUSTER + 1);
/// How far the list scrolls each frame, in logical pixels.
const SCROLL_PER_FRAME: f32 = 3.0;
const FOOTER_LABELS: [&str; 3] = ["likes", "replies", "shares"];

fn rgb(value: [u8; 3]) -> Hsla {
    Rgba {
        r: f32::from(value[0]) / 255.0,
        g: f32::from(value[1]) / 255.0,
        b: f32::from(value[2]) / 255.0,
        a: 1.0,
    }
    .into()
}

fn hex(value: u32) -> Hsla {
    rgb([(value >> 16) as u8, (value >> 8) as u8, value as u8])
}

/// `value` as RGB, at `alpha` of 255.
fn rgba(value: u32, alpha: u8) -> Hsla {
    hex(value).opacity(f32::from(alpha) / 255.0)
}

fn palette(index: usize) -> Hsla {
    rgb(PALETTE_RGB[index % PALETTE_RGB.len()])
}

fn ink() -> Hsla {
    hex(0x111827)
}
fn body() -> Hsla {
    hex(0x374151)
}
fn muted() -> Hsla {
    hex(0x6B7280)
}
fn hairline() -> Hsla {
    hex(0xE5E7EB)
}

/// A line of text as Roboto's outlines: GPUI draws paths turned, but no
/// text.
struct GlyphText {
    /// Outline commands in font units, the pen advanced glyph by glyph.
    commands: Vec<Command>,
    advance: f32,
    ascender: f32,
    descender: f32,
    units_per_em: f32,
}

#[derive(Clone, Copy)]
enum Command {
    Move(f32, f32),
    Line(f32, f32),
    Quad(f32, f32, f32, f32),
    Cubic(f32, f32, f32, f32, f32, f32),
    Close,
}

struct Outline<'a> {
    commands: &'a mut Vec<Command>,
    x: f32,
}

impl ttf_parser::OutlineBuilder for Outline<'_> {
    fn move_to(&mut self, x: f32, y: f32) {
        self.commands.push(Command::Move(self.x + x, y));
    }
    fn line_to(&mut self, x: f32, y: f32) {
        self.commands.push(Command::Line(self.x + x, y));
    }
    fn quad_to(&mut self, x1: f32, y1: f32, x: f32, y: f32) {
        self.commands
            .push(Command::Quad(self.x + x1, y1, self.x + x, y));
    }
    fn curve_to(&mut self, x1: f32, y1: f32, x2: f32, y2: f32, x: f32, y: f32) {
        self.commands.push(Command::Cubic(
            self.x + x1,
            y1,
            self.x + x2,
            y2,
            self.x + x,
            y,
        ));
    }
    fn close(&mut self) {
        self.commands.push(Command::Close);
    }
}

impl GlyphText {
    fn new(face: &ttf_parser::Face, text: &str) -> Option<Self> {
        let mut commands = Vec::new();
        let mut advance = 0.0;
        for character in text.chars() {
            let glyph = face.glyph_index(character)?;
            face.outline_glyph(
                glyph,
                &mut Outline {
                    commands: &mut commands,
                    x: advance,
                },
            );
            advance += f32::from(face.glyph_hor_advance(glyph)?);
        }
        Some(Self {
            commands,
            advance,
            ascender: f32::from(face.ascender()),
            descender: f32::from(face.descender()),
            units_per_em: f32::from(face.units_per_em()),
        })
    }

    /// The line's width and the font's height at `font_size`.
    fn size(&self, font_size: f32) -> gpui::Size<Pixels> {
        let scale = font_size / self.units_per_em;
        size(
            px(self.advance * scale),
            px((self.ascender - self.descender) * scale),
        )
    }

    /// Paints the line at `font_size` with its top left `corner` offset from
    /// `center`, turned `degrees` about `center`.
    fn paint(
        &self,
        window: &mut Window,
        font_size: f32,
        corner: Point<Pixels>,
        degrees: f32,
        center: Point<Pixels>,
        color: Hsla,
    ) {
        let scale = font_size / self.units_per_em;
        // Glyphs from the font's units, y up, onto the screen, y down.
        let origin = corner + point(px(0.0), px(self.ascender * scale));
        let at = |x: f32, y: f32| origin + point(px(x * scale), px(-y * scale));
        let mut glyphs = PathBuilder::fill();
        for command in &self.commands {
            match *command {
                Command::Move(x, y) => glyphs.move_to(at(x, y)),
                Command::Line(x, y) => glyphs.line_to(at(x, y)),
                Command::Quad(x1, y1, x, y) => glyphs.curve_to(at(x, y), at(x1, y1)),
                Command::Cubic(x1, y1, x2, y2, x, y) => {
                    glyphs.cubic_bezier_to(at(x, y), at(x1, y1), at(x2, y2))
                }
                Command::Close => glyphs.close(),
            }
        }
        glyphs.rotate(degrees);
        glyphs.translate(center);
        if let Ok(path) = glyphs.build() {
            window.paint_path(path, color);
        }
    }
}

/// The outlines of every turned text on screen: the badge's, and the stacked
/// panels' titles and cell numbers.
struct Glyphs {
    hot: GlyphText,
    titles: Vec<GlyphText>,
    numbers: Vec<GlyphText>,
}

impl Glyphs {
    fn new(regular: &[u8], bold: &[u8], tier: GauntletTier) -> Option<Self> {
        let regular = ttf_parser::Face::parse(regular, 0).ok()?;
        let bold = ttf_parser::Face::parse(bold, 0).ok()?;
        Some(Self {
            hot: GlyphText::new(&bold, "HOT")?,
            titles: (1..=tier.layers)
                .map(|layer| GlyphText::new(&bold, &format!("Layer {layer}")))
                .collect::<Option<_>>()?,
            numbers: (1..=tier.layer_rows * LAYER_COLUMNS)
                .map(|number| GlyphText::new(&regular, &number.to_string()))
                .collect::<Option<_>>()?,
        })
    }
}

/// Adds a rounded rectangle at `origin` of `size` with corner `radius`.
fn rounded_rectangle(
    path: &mut PathBuilder,
    origin: Point<Pixels>,
    size: gpui::Size<Pixels>,
    radius: Pixels,
) {
    let (left, top) = (origin.x, origin.y);
    let (right, bottom) = (left + size.width, top + size.height);
    path.move_to(point(left + radius, top));
    path.line_to(point(right - radius, top));
    path.curve_to(point(right, top + radius), point(right, top));
    path.line_to(point(right, bottom - radius));
    path.curve_to(point(right - radius, bottom), point(right, bottom));
    path.line_to(point(left + radius, bottom));
    path.curve_to(point(left, bottom - radius), point(left, bottom));
    path.line_to(point(left, top + radius));
    path.curve_to(point(left + radius, top), point(left, top));
    path.close();
}

struct Gauntlet {
    load: Launch,
    tier: GauntletTier,
    frame: u32,
    frozen: bool,
    first_frame_logged: bool,
    posts: Arc<Vec<Post>>,
    tickers: Vec<Ticker>,
    avatars: Arc<Vec<Arc<RenderImage>>>,
    glyphs: Arc<Option<Glyphs>>,
    list: ListState,
    /// The first row the list shows and its top in the list's content.
    anchor: (usize, Pixels),
}

impl Gauntlet {
    fn new(load: Launch, tier: GauntletTier, glyphs: Option<Glyphs>) -> Self {
        let avatars = (0..AVATAR_COUNT)
            .map(|index| {
                // GPUI's frames hold BGRA pixels.
                let mut pixels = avatar_rgba(index);
                for pixel in pixels.as_chunks_mut::<4>().0 {
                    pixel.swap(0, 2);
                }
                let buffer = image::RgbaImage::from_raw(AVATAR_SIZE, AVATAR_SIZE, pixels)
                    .unwrap_or_default();
                Arc::new(RenderImage::new([image::Frame::new(buffer)]))
            })
            .collect();
        let list = ListState::new(ROWS, ListAlignment::Top, px(0.0));
        Self {
            load,
            tier,
            frame: 0,
            frozen: false,
            first_frame_logged: false,
            posts: Arc::new(posts()),
            tickers: tickers(tier.tickers),
            avatars: Arc::new(avatars),
            glyphs: Arc::new(glyphs),
            list,
            anchor: (0, px(0.0)),
        }
    }

    fn ticker_panel(&self) -> impl IntoElement {
        let s = self.tier.scale;
        div()
            .flex()
            .flex_wrap()
            .gap(px(4.0 * s))
            .p(px(6.0 * s))
            .bg(hex(0xE2E8F0))
            .children(self.tickers.iter().enumerate().map(|(index, ticker)| {
                let cents = ticker_cents(ticker, self.frame);
                let change = if cents >= ticker.base_cents {
                    hex(0x16A34A)
                } else {
                    hex(0xDC2626)
                };
                let share = ((cents - ticker.base_cents + ticker.swing_cents) as f32
                    / (2 * ticker.swing_cents) as f32)
                    .clamp(0.0, 1.0);
                div()
                    .flex()
                    .items_center()
                    .gap(px(4.0 * s))
                    .px(px(6.0 * s))
                    .py(px(3.0 * s))
                    .rounded(px(6.0 * s))
                    .bg(gpui::white())
                    .text_size(px(10.0 * s))
                    .child(
                        div()
                            .font_weight(FontWeight::BOLD)
                            .text_color(ink())
                            .child(ticker.symbol.clone()),
                    )
                    .child(div().text_color(body()).child(cents_text(cents)))
                    .child(div().text_color(change).child(change_text(ticker, cents)))
                    .child(bar(share, palette(index), px(20.0 * s), 4.0 * s))
            }))
    }
}

/// A rounded track filled to `share` in `fill`.
fn bar(share: f32, fill: Hsla, width: impl Into<gpui::Length>, height: f32) -> impl IntoElement {
    div()
        .w(width.into())
        .h(px(height))
        .rounded(px(height / 2.0))
        .bg(hairline())
        .child(
            div()
                .h_full()
                .w(relative(share))
                .rounded(px(height / 2.0))
                .bg(fill),
        )
}

/// Lines 1.4 em apart, cut with an ellipsis after `lines`.
fn paragraph(text: impl Into<SharedString>, size: f32, color: Hsla, lines: usize) -> gpui::Div {
    div()
        .text_size(px(size))
        .line_height(px(size * 1.4))
        .text_color(color)
        .line_clamp(lines)
        .text_ellipsis()
        .child(text.into())
}

/// Everything a row needs, shared with the list's item renderer.
#[derive(Clone)]
struct Rows {
    tier: GauntletTier,
    frame: u32,
    posts: Arc<Vec<Post>>,
    avatars: Arc<Vec<Arc<RenderImage>>>,
    glyphs: Arc<Option<Glyphs>>,
}

impl Rows {
    fn row(&self, row: usize) -> AnyElement {
        let s = self.tier.scale;
        let content = match gauntlet_row(row, self.tier.columns) {
            GauntletRow::Cards(first) => div()
                .flex()
                .items_start()
                .gap(px(8.0 * s))
                .children((first..first + self.tier.columns).map(|card| self.card(card)))
                .into_any_element(),
            GauntletRow::Cluster(cluster) => {
                self.level(cluster, self.tier.depth).into_any_element()
            }
        };
        div()
            .px(px(8.0 * s))
            .pb(px(8.0 * s))
            .when(row == 0, |row| row.pt(px(8.0 * s)))
            .child(content)
            .into_any_element()
    }

    fn card(&self, card: usize) -> impl IntoElement {
        let (s, frame) = (self.tier.scale, self.frame);
        let post = &self.posts[card % POST_COUNT];
        let (tag, tag_color) = &post.tags[0];
        let permille = progress_permille(card, frame);
        let header = div()
            .flex()
            .items_center()
            .gap(px(8.0 * s))
            .child(
                img(ImageSource::Render(
                    self.avatars[card % AVATAR_COUNT].clone(),
                ))
                .flex_none()
                .size(px(32.0 * s))
                .rounded_full(),
            )
            .child(
                div()
                    .flex_1()
                    .min_w(px(0.0))
                    .child(
                        paragraph(post.title.clone(), 13.0 * s, ink(), 2)
                            .font_weight(FontWeight::BOLD),
                    )
                    // `by Ada Lovelace · #rust`: the author bold, the tag in its color.
                    .child(
                        div()
                            .flex()
                            .overflow_hidden()
                            .whitespace_nowrap()
                            .text_size(px(11.0 * s))
                            .text_color(muted())
                            .child("by ")
                            .child(
                                div()
                                    .font_weight(FontWeight::BOLD)
                                    .child(post.author.clone()),
                            )
                            .child(" · ")
                            .child(
                                div()
                                    .text_color(rgb(GRADIENT_END_RGB[*tag_color]))
                                    .child(tag.clone()),
                            ),
                    ),
            );
        let progress = div()
            .flex()
            .items_center()
            .gap(px(6.0 * s))
            .child(div().flex_1().child(bar(
                permille as f32 / 1000.0,
                palette(card),
                relative(1.0),
                6.0 * s,
            )))
            .child(
                div()
                    .text_size(px(10.0 * s))
                    .text_color(muted())
                    .child(format!("{}%", permille / 10)),
            );
        let color = palette(post.color);
        let sparkline = canvas(
            |_, _, _| {},
            move |bounds, _, window, _| sparkline(window, bounds, card, frame, color, 1.5 * s),
        )
        .w_full()
        .h(px(36.0 * s));
        let chips = div()
            .flex()
            .flex_wrap()
            .gap(px(4.0 * s))
            .children(post.tags.iter().map(|(tag, color)| {
                div()
                    .px(px(8.0 * s))
                    .py(px(3.0 * s))
                    .rounded(px(10.0 * s))
                    .bg(rgb(CHIP_BACKGROUND_RGB[*color]))
                    .text_size(px(10.0 * s))
                    .text_color(rgb(GRADIENT_END_RGB[*color]))
                    .child(tag.clone())
            }));
        // Three counters split by dividers as tall as the row.
        let mut footer = div().flex();
        for (index, label) in FOOTER_LABELS.iter().enumerate() {
            if index > 0 {
                footer = footer.child(div().w(px(1.0)).bg(hairline()));
            }
            footer = footer.child(
                div()
                    .flex_1()
                    .flex()
                    .flex_col()
                    .items_center()
                    .child(
                        div()
                            .text_size(px(12.0 * s))
                            .font_weight(FontWeight::BOLD)
                            .text_color(ink())
                            .child(post.stats[index].clone()),
                    )
                    .child(
                        div()
                            .text_size(px(9.0 * s))
                            .text_color(muted())
                            .child(*label),
                    ),
            );
        }
        // Compose draws its border over the padding: the content is inset by
        // the padding alone, as a GPUI border is.
        let body_text = paragraph(post.body.clone(), 12.0 * s, body(), 4);
        div()
            .relative()
            .flex_1()
            .min_w(px(0.0))
            .flex()
            .flex_col()
            .gap(px(6.0 * s))
            .p(px(10.0 * s))
            .rounded(px(12.0 * s))
            .border_1()
            .border_color(hairline())
            .bg(gpui::white())
            .shadow(vec![BoxShadow {
                color: Hsla {
                    h: 0.0,
                    s: 0.0,
                    l: 0.0,
                    a: 0.24,
                },
                offset: point(px(0.0), px(s)),
                blur_radius: px(3.0 * s),
                spread_radius: px(0.0),
                inset: false,
            }])
            .child(header)
            .child(body_text)
            .child(progress)
            .child(sparkline)
            .child(chips)
            .child(footer)
            .when(card.is_multiple_of(5), |card_box| {
                let glyphs = self.glyphs.clone();
                let degrees = badge_degrees(card, frame);
                // A translucent tag tilting with the frame: drawn rotated
                // over the card, never laid out again.
                card_box.child(
                    canvas(
                        |_, _, _| {},
                        move |bounds, _, window, _| {
                            if let Some(glyphs) = glyphs.as_ref() {
                                badge_paint(window, bounds, &glyphs.hot, degrees, s);
                            }
                        },
                    )
                    .absolute()
                    .top_0()
                    .left_0()
                    .size_full(),
                )
            })
    }

    /// Levels nested inside one another, each a row of three labels above the
    /// next level. A level stacks its two children in block layout: nested
    /// flex columns make taffy measure each level again for its parent.
    fn level(&self, cluster: usize, remaining: usize) -> gpui::Div {
        let s = self.tier.scale;
        let chips = div()
            .flex()
            .items_start()
            .gap(px(2.0 * s))
            .children((0..3).map(|chip| {
                div()
                    .flex_1()
                    .min_w(px(0.0))
                    .rounded(px(2.0 * s))
                    .bg(palette(remaining + chip + cluster))
                    .text_size(px(9.0 * s))
                    .text_color(gpui::white())
                    .child(format!("C{cluster}.L{remaining}.{chip}"))
            }));
        let levels = [hex(0xF1F5F9), hex(0xCBD5E1)];
        div()
            .pl(px(3.0 * s))
            .pt(px(s))
            .pr(px(s))
            .pb(px(s))
            .rounded(px(4.0 * s))
            .bg(levels[remaining % 2])
            .child(chips)
            .when(remaining > 0, |level| {
                level.child(self.level(cluster, remaining - 1).mt(px(s)))
            })
    }
}

/// A card's 48-point line over a fading fill, drawn from the frame.
fn sparkline(
    window: &mut Window,
    bounds: Bounds<Pixels>,
    card: usize,
    frame: u32,
    color: Hsla,
    stroke: f32,
) {
    let (width, height) = (f32::from(bounds.size.width), f32::from(bounds.size.height));
    let step = width / (GAUNTLET_SPARK_POINTS - 1) as f32;
    let at = |index: usize| {
        bounds.origin
            + point(
                px(index as f32 * step),
                px(height * (1.0 - spark_value(card, index, frame))),
            )
    };
    let mut area = PathBuilder::fill();
    area.move_to(bounds.origin + point(px(0.0), px(height)));
    let mut line = PathBuilder::stroke(px(stroke));
    line.move_to(at(0));
    for index in 0..GAUNTLET_SPARK_POINTS {
        area.line_to(at(index));
        if index > 0 {
            line.line_to(at(index));
        }
    }
    area.line_to(bounds.origin + point(px(width), px(height)));
    area.close();
    let fade: Background = linear_gradient(
        180.0,
        linear_color_stop(color.opacity(0.25), 0.0),
        linear_color_stop(color.opacity(0.0), 1.0),
    );
    if let Ok(path) = area.build() {
        window.paint_path(path, fade);
    }
    if let Ok(path) = line.build() {
        window.paint_path(path, color);
    }
}

/// The `HOT` tag in the card's top right corner, rotated about its centre.
fn badge_paint(
    window: &mut Window,
    bounds: Bounds<Pixels>,
    text: &GlyphText,
    degrees: f32,
    s: f32,
) {
    let label = text.size(9.0 * s);
    let badge = size(label.width + px(12.0 * s), label.height + px(4.0 * s));
    let center = point(
        bounds.origin.x + bounds.size.width - px(6.0 * s) - badge.width / 2.0,
        bounds.origin.y + px(6.0 * s) + badge.height / 2.0,
    );
    let corner = point(-badge.width / 2.0, -badge.height / 2.0);
    let mut background = PathBuilder::fill();
    rounded_rectangle(
        &mut background,
        corner,
        badge,
        px(8.0 * s).min(badge.height / 2.0),
    );
    background.rotate(degrees);
    background.translate(center);
    if let Ok(path) = background.build() {
        window.paint_path(path, palette(0).opacity(0.9));
    }
    text.paint(
        window,
        9.0 * s,
        corner + point(px(6.0 * s), px(2.0 * s)),
        degrees,
        center,
        gpui::white().opacity(0.9),
    );
}

/// Fills a rounded rectangle at `corner` offset from `center`, turned
/// `degrees` about `center`.
fn turned_rectangle(
    window: &mut Window,
    corner: Point<Pixels>,
    extent: gpui::Size<Pixels>,
    radius: f32,
    degrees: f32,
    center: Point<Pixels>,
    color: Hsla,
) {
    let mut path = PathBuilder::fill();
    rounded_rectangle(&mut path, corner, extent, px(radius));
    path.rotate(degrees);
    path.translate(center);
    if let Ok(path) = path.build() {
        window.paint_path(path, color);
    }
}

/// The translucent panels stacked over the list, painted again each frame as
/// paths turned about each panel's centre, as GPUI turns no element; the
/// nested card's text stays upright, where its panel moved its centre.
fn layers_paint(
    window: &mut Window,
    cx: &mut App,
    bounds: Bounds<Pixels>,
    glyphs: &Glyphs,
    tier: GauntletTier,
    frame: u32,
) {
    let line = glyphs.hot.size(11.0).height;
    let title = glyphs.hot.size(13.0).height;
    let cells_top = 10.0 + f32::from(title) + 8.0;
    let nested_top = cells_top + tier.layer_rows as f32 * 25.0 - 3.0 + 8.0;
    let nested = size(px(200.0), px(8.0 + 2.0 * f32::from(line) + 2.0 + 8.0));
    let panel = size(px(220.0), px(nested_top + 10.0) + nested.height);
    let half = point(panel.width / 2.0, panel.height / 2.0);
    let font = |weight| gpui::Font {
        weight,
        ..gpui::font("Roboto")
    };
    for (layer, title) in glyphs.titles.iter().enumerate() {
        let degrees = layer_degrees(layer, frame);
        let center =
            bounds.origin + point(px(layer_x(layer, frame)), px(layer_y(layer, frame))) + half;
        let background = rgba(0x1E293B, 0xC0);
        let top_left = point(-half.x, -half.y);
        turned_rectangle(window, top_left, panel, 12.0, degrees, center, background);
        let at = top_left + point(px(10.0), px(10.0));
        title.paint(window, 13.0, at, degrees, center, gpui::white());
        for (cell, number) in glyphs.numbers.iter().enumerate() {
            let corner = top_left
                + point(
                    px(10.0 + (cell % LAYER_COLUMNS) as f32 * 25.0),
                    px(cells_top + (cell / LAYER_COLUMNS) as f32 * 25.0),
                );
            let color = palette(layer_cell_color(layer, cell));
            turned_rectangle(
                window,
                corner,
                size(px(22.0), px(22.0)),
                4.0,
                degrees,
                center,
                color,
            );
            let label = number.size(9.0);
            let at = corner + point(px(11.0) - label.width / 2.0, px(11.0) - label.height / 2.0);
            number.paint(window, 9.0, at, degrees, center, gpui::white());
        }
        // The nested card's centre, turned with its panel.
        let offset = top_left + point(px(110.0), px(nested_top) + nested.height / 2.0);
        let (sin, cos) = degrees.to_radians().sin_cos();
        let (x, y) = (f32::from(offset.x), f32::from(offset.y));
        let middle = center + point(px(x * cos - y * sin), px(x * sin + y * cos));
        let corner = point(-nested.width / 2.0, -nested.height / 2.0);
        turned_rectangle(
            window,
            corner,
            nested,
            8.0,
            0.0,
            middle,
            rgba(0xFFFFFF, 0xE6),
        );
        let mut top = middle + corner + point(px(8.0), px(8.0));
        let lines = [
            (
                format!("Nested in layer {}", layer + 1),
                FontWeight::BOLD,
                ink(),
            ),
            (
                "Tilts against its panel".to_owned(),
                FontWeight::NORMAL,
                body(),
            ),
        ];
        for (text, weight, color) in lines {
            let run = gpui::TextRun {
                len: text.len(),
                font: font(weight),
                color,
                background_color: None,
                underline: None,
                strikethrough: None,
            };
            let shaped = window
                .text_system()
                .shape_line(text.into(), px(11.0), &[run], None);
            if let Err(error) = shaped.paint(top, line, gpui::TextAlign::Left, None, window, cx) {
                log::warn!("nested text not painted: {error}");
            }
            top.y += line + px(2.0);
        }
    }
}

impl Render for Gauntlet {
    fn render(&mut self, window: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        if !self.first_frame_logged {
            self.first_frame_logged = true;
            log::info!("PERF first_frame");
        }
        if !self.frozen {
            self.frame += 1;
            // The list scrolls to the frame's offset from the top: rows the
            // last frame laid out above it move the anchor down.
            let offset = px(self.frame as f32 * SCROLL_PER_FRAME);
            while let Some(bounds) = self.list.bounds_for_item(self.anchor.0) {
                let (row, top) = self.anchor;
                if top + bounds.size.height > offset {
                    break;
                }
                self.anchor = (row + 1, top + bounds.size.height);
            }
            self.list.scroll_to(ListOffset {
                item_ix: self.anchor.0,
                offset_in_item: offset - self.anchor.1,
            });
            if self.load.freeze > 0 && self.frame >= self.load.freeze {
                self.frozen = true;
                log::info!("PERF frozen frame={}", self.frame);
            } else {
                window.request_animation_frame();
            }
        }
        let s = self.tier.scale;
        let rows = Rows {
            tier: self.tier,
            frame: self.frame,
            posts: self.posts.clone(),
            avatars: self.avatars.clone(),
            glyphs: self.glyphs.clone(),
        };
        let (glyphs, tier, frame) = (self.glyphs.clone(), self.tier, self.frame);
        let (width, _) = perf_data::DESKTOP_WINDOW;
        div()
            .size_full()
            .flex()
            .flex_col()
            .font_family("Roboto")
            // Lines 1.4 em apart, as the other apps set them: GPUI's own
            // default is 1.618 em.
            .line_height(relative(1.4))
            .bg(hex(0xEEF0F5))
            .child(
                div()
                    .flex_none()
                    .h(px(56.0))
                    .px(px(16.0))
                    .flex()
                    .items_center()
                    .bg(hex(0x1E2A4A))
                    .text_size(px(20.0))
                    .font_weight(FontWeight::BOLD)
                    .text_color(gpui::white())
                    .child("Gauntlet"),
            )
            .child(
                div()
                    .flex_1()
                    .min_h(px(0.0))
                    .w(px(width as f32 * width_fraction(self.frame)))
                    .flex()
                    .flex_col()
                    .child(self.ticker_panel())
                    // The panels stack over the list and show only over it,
                    // as every app clips them.
                    .child(
                        div()
                            .flex_1()
                            .min_h(px(0.0))
                            .w_full()
                            .relative()
                            .overflow_hidden()
                            .child(
                                list(self.list.clone(), move |row, _, _| rows.row(row))
                                    .size_full()
                                    .text_size(px(10.0 * s)),
                            )
                            .child(
                                canvas(
                                    |_, _, _| {},
                                    move |bounds, _, window, cx| {
                                        if let Some(glyphs) = glyphs.as_ref() {
                                            layers_paint(window, cx, bounds, glyphs, tier, frame);
                                        }
                                    },
                                )
                                .absolute()
                                .top_0()
                                .left_0()
                                .size_full(),
                            ),
                    ),
            )
    }
}

fn main() {
    perf_data::log_to_stdout();
    let load = Launch::from_env();
    gpui_platform::application().run(move |cx: &mut App| {
        let tier = gauntlet_tier(load.tier);
        let mut fonts: Vec<Cow<'static, [u8]>> = Vec::new();
        for file in ["Roboto-Regular.ttf", "Roboto-Bold.ttf"] {
            match std::fs::read(perf_data::font_path(file)) {
                Ok(bytes) => fonts.push(Cow::Owned(bytes)),
                Err(error) => log::warn!("no {file}: {error}"),
            }
        }
        let glyphs = match fonts.as_slice() {
            [regular, bold] => Glyphs::new(regular, bold, tier),
            _ => None,
        };
        if let Err(error) = cx.text_system().add_fonts(fonts) {
            log::warn!("fonts not added: {error}");
        }
        let (width, height) = perf_data::DESKTOP_WINDOW;
        let bounds = Bounds::centered(None, size(px(width as f32), px(height as f32)), cx);
        let options = WindowOptions {
            window_bounds: Some(WindowBounds::Windowed(bounds)),
            titlebar: Some(TitlebarOptions {
                title: Some("Gauntlet".into()),
                ..Default::default()
            }),
            is_resizable: false,
            // The window draws every frame even when another window is in
            // front, as the other frameworks' windows do.
            inactive_frame_interval: None,
            ..Default::default()
        };
        if let Err(error) = cx.open_window(options, |_, cx| {
            cx.new(|_| Gauntlet::new(load, tier, glyphs))
        }) {
            log::error!("no window: {error}");
        }
        cx.activate(true);
    });
}
