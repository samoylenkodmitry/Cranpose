//! The gauntlet in egui: the screen `compose-app/.../Gauntlet.kt` and
//! `cranpose-app/src/screens/gauntlet.rs` draw, element for element, in egui's
//! immediate-mode idiom. The benchmark's README describes it.
//!
//! Every frame advances a frame index and everything follows from it, never
//! from wall time. egui lays out and paints the whole screen each frame, as
//! immediate mode does: boxes reserve their background, lay out their content
//! and paint the background behind it; fixed-size pieces allocate their size
//! and paint. The list lays out only the rows on screen.

use std::sync::Arc;

use eframe::egui::{
    self, Align, Color32, ColorImage, FontData, FontDefinitions, FontFamily, FontId, Galley,
    Layout, Pos2, Rect, Sense, Shape, Stroke, StrokeKind, TextureHandle, TextureOptions, Ui,
    UiBuilder, Vec2, WidgetInfo, WidgetType,
    emath::Rot2,
    epaint::{Mesh, RectShape, TextShape},
    pos2,
    text::{LayoutJob, TextFormat, TextWrapping},
    vec2,
};
use perf_data::*;

/// Blocks of five card rows and a cluster: no measurement window reaches the end.
const ROWS: usize = 2000 * (CARD_ROWS_PER_CLUSTER + 1);
/// How far the list scrolls each frame, in points (dp).
const SCROLL_PER_FRAME: f32 = 3.0;

const BACKGROUND: Color32 = Color32::from_rgb(0xEE, 0xF0, 0xF5);
const TOP_BAR: Color32 = Color32::from_rgb(0x1E, 0x2A, 0x4A);
const INK: Color32 = Color32::from_rgb(0x11, 0x18, 0x27);
const BODY: Color32 = Color32::from_rgb(0x37, 0x41, 0x51);
const MUTED: Color32 = Color32::from_rgb(0x6B, 0x72, 0x80);
const HAIRLINE: Color32 = Color32::from_rgb(0xE5, 0xE7, 0xEB);
const PANEL: Color32 = Color32::from_rgb(0xE2, 0xE8, 0xF0);
const UP: Color32 = Color32::from_rgb(0x16, 0xA3, 0x4A);
const DOWN: Color32 = Color32::from_rgb(0xDC, 0x26, 0x26);
const LEVEL_BACKGROUND: [Color32; 2] = [
    Color32::from_rgb(0xF1, 0xF5, 0xF9),
    Color32::from_rgb(0xCB, 0xD5, 0xE1),
];
const LAYER_BACKGROUND: Color32 = Color32::from_rgba_unmultiplied_const(0x1E, 0x29, 0x3B, 0xC0);
const NESTED_BACKGROUND: Color32 = Color32::from_rgba_unmultiplied_const(0xFF, 0xFF, 0xFF, 0xE6);
const FOOTER_LABELS: [&str; 3] = ["likes", "replies", "shares"];

const fn colors(rgb: [[u8; 3]; 8]) -> [Color32; 8] {
    let mut out = [Color32::BLACK; 8];
    let mut index = 0;
    while index < 8 {
        out[index] = Color32::from_rgb(rgb[index][0], rgb[index][1], rgb[index][2]);
        index += 1;
    }
    out
}

const PALETTE: [Color32; 8] = colors(PALETTE_RGB);
const CHIP_BACKGROUND: [Color32; 8] = colors(CHIP_BACKGROUND_RGB);
const GRADIENT_END: [Color32; 8] = colors(GRADIENT_END_RGB);

/// The device's Roboto files, the ones the other apps load; egui's own fonts
/// stay behind them for anything a file does not hold.
pub fn roboto() -> FontDefinitions {
    let mut fonts = FontDefinitions::default();
    let fallback = fonts
        .families
        .get(&FontFamily::Proportional)
        .cloned()
        .unwrap_or_default();
    for (name, file, family) in [
        ("roboto", "Roboto-Regular.ttf", FontFamily::Proportional),
        ("roboto-bold", "Roboto-Bold.ttf", bold()),
    ] {
        match std::fs::read(perf_data::font_path(file)) {
            Ok(bytes) => {
                fonts
                    .font_data
                    .insert(name.to_owned(), Arc::new(FontData::from_owned(bytes)));
            }
            Err(error) => log::warn!("no {file}: {error}"),
        }
        let mut names = vec![name.to_owned()];
        names.extend(fallback.iter().cloned());
        names.retain(|font| fonts.font_data.contains_key(font));
        fonts.families.insert(family, names);
    }
    fonts
}

fn bold() -> FontFamily {
    FontFamily::Name("bold".into())
}

/// One tier's scale and the text formats it draws with.
struct Look {
    s: f32,
    bold: FontFamily,
}

impl Look {
    fn format(&self, size: f32, color: Color32, bold: bool) -> TextFormat {
        let family = if bold {
            self.bold.clone()
        } else {
            FontFamily::Proportional
        };
        TextFormat {
            font_id: FontId::new(size * self.s, family),
            color,
            ..Default::default()
        }
    }

    /// Paragraph text: lines 1.4 em apart.
    fn paragraph(&self, size: f32, color: Color32, bold: bool) -> TextFormat {
        TextFormat {
            line_height: Some(size * self.s * 1.4),
            ..self.format(size, color, bold)
        }
    }
}

/// Text laid out to `width`, at most `max_rows` lines, ending in an ellipsis
/// when cut.
fn galley(ui: &Ui, sections: &[(&str, TextFormat)], width: f32, max_rows: usize) -> Arc<Galley> {
    let mut job = LayoutJob::default();
    for (text, format) in sections {
        job.append(text, 0.0, format.clone());
    }
    job.wrap = TextWrapping {
        max_width: width,
        max_rows,
        break_anywhere: false,
        overflow_character: Some('…'),
    };
    ui.ctx().fonts_mut(|fonts| fonts.layout_job(job))
}

/// Allocates `size` in the layout, read out as `label` to accessibility
/// services when one is on: what a toolkit's text widget gives them.
fn labeled(ui: &mut Ui, size: Vec2, label: impl Fn() -> String) -> Rect {
    let (rect, response) = ui.allocate_exact_size(size, Sense::hover());
    response.widget_info(|| WidgetInfo::labeled(WidgetType::Label, true, label()));
    rect
}

/// Allocates the galley's size in the layout and paints it there.
fn text(ui: &mut Ui, galley: Arc<Galley>) -> Rect {
    let rect = labeled(ui, galley.size(), || galley.text().to_owned());
    ui.painter().galley(rect.min, galley, Color32::PLACEHOLDER);
    rect
}

/// Android's elevation shadow as Compose draws it, for a box at `elevation`
/// points (dp) in a window covering `screen`: a faint ambient shadow all
/// around, and a spot shadow cast away from a light above the window's top
/// centre. Each is a rectangle whose edge fades over the shadow's penumbra.
fn elevation_shadows(
    rect: Rect,
    radius: f32,
    elevation: f32,
    screen: Rect,
    pixels: f32,
) -> [Shape; 2] {
    const AMBIENT_ALPHA: f32 = 0.039;
    const SPOT_ALPHA: f32 = 0.19;
    const LIGHT_RADIUS: f32 = 800.0;
    let alpha = |share: f32| Color32::from_black_alpha((share * 255.0).round() as u8);
    // The ambient shadow fades outward from the box's edge.
    let ambient_width = elevation / 2.0 * (1.0 + elevation * pixels / 128.0);
    let ambient = RectShape::filled(
        rect.expand(ambient_width / 2.0),
        radius,
        alpha(AMBIENT_ALPHA),
    )
    .with_blur_width(ambient_width);
    // The spot shadow falls away from the light, its penumbra centred on the
    // shifted edge.
    let light_z = 500.0 * (screen.width().min(screen.height()) / 450.0 + 2.0) / 3.0;
    let ratio = elevation / (light_z - elevation);
    let light = pos2(screen.center().x, screen.top());
    let spot = RectShape::filled(
        rect.translate((rect.center() - light) * ratio),
        radius,
        alpha(SPOT_ALPHA),
    )
    .with_blur_width(LIGHT_RADIUS * ratio);
    [Shape::Rect(ambient), Shape::Rect(spot)]
}

/// A box as wide as the space it is in: `margin` (left, top, right, bottom)
/// around its top-down content, with the background painted behind once the
/// content's height is known.
fn boxed(
    ui: &mut Ui,
    margin: [f32; 4],
    add: impl FnOnce(&mut Ui),
    paint: impl FnOnce(Rect) -> Vec<Shape>,
) -> Rect {
    let background = ui.painter().add(Shape::Noop);
    let outer = ui.available_rect_before_wrap();
    let [left, top, right, bottom] = margin;
    let inner = Rect::from_min_max(
        pos2(outer.left() + left, outer.top() + top),
        pos2(outer.right() - right, outer.bottom()),
    );
    let mut child = ui.new_child(
        UiBuilder::new()
            .max_rect(inner)
            .layout(Layout::top_down(Align::Min)),
    );
    add(&mut child);
    let rect = Rect::from_min_max(
        outer.min,
        pos2(outer.right(), child.min_rect().bottom() + bottom),
    );
    ui.painter().set(background, Shape::Vec(paint(rect)));
    ui.allocate_rect(rect, Sense::hover());
    rect
}

/// A rounded track filled to `share` in `fill`.
/// A rounded rectangle's outline, each point passed through `place`: what
/// egui fills as a convex polygon for a box it turns.
fn rounded_outline(rect: Rect, radius: f32, place: impl Fn(Pos2) -> Pos2) -> Vec<Pos2> {
    let radius = radius.min(rect.height() / 2.0).min(rect.width() / 2.0);
    let corners = [
        (pos2(rect.right() - radius, rect.top() + radius), -90.0_f32),
        (pos2(rect.right() - radius, rect.bottom() - radius), 0.0),
        (pos2(rect.left() + radius, rect.bottom() - radius), 90.0),
        (pos2(rect.left() + radius, rect.top() + radius), 180.0),
    ];
    corners
        .iter()
        .flat_map(|&(corner, start)| {
            (0..=6).map(move |step| {
                let at = (start + step as f32 * 15.0).to_radians();
                corner + vec2(at.cos(), at.sin()) * radius
            })
        })
        .map(place)
        .collect()
}

/// Reports `rect` as the text `label` to accessibility services, as a
/// toolkit's text widget does.
fn announce(ui: &Ui, rect: Rect, id: impl std::hash::Hash + std::fmt::Debug, label: &str) {
    ui.interact(rect, ui.id().with(id), Sense::hover())
        .widget_info(|| WidgetInfo::labeled(WidgetType::Label, true, label));
}

fn bar(rect: Rect, radius: f32, share: f32, fill: Color32) -> [Shape; 2] {
    let filled = Rect::from_min_size(rect.min, vec2(rect.width() * share, rect.height()));
    [
        Shape::rect_filled(rect, radius, HAIRLINE),
        Shape::rect_filled(filled, radius, fill),
    ]
}

pub struct Gauntlet {
    load: Launch,
    tier: GauntletTier,
    look: Look,
    frame: u32,
    frozen: bool,
    posts: Vec<Post>,
    tickers: Vec<Ticker>,
    avatars: Vec<TextureHandle>,
    /// The first row that reaches the screen and its top in the list's content.
    anchor: (usize, f32),
    /// The window this frame draws in, and its pixels per point.
    screen: Rect,
    pixels: f32,
    first_frame_logged: bool,
}

impl Gauntlet {
    pub fn new(ctx: &egui::Context, load: Launch) -> Self {
        ctx.set_fonts(roboto());
        // A light screen: egui's light theme also renders dark text on light
        // backgrounds the way it means to.
        ctx.set_theme(egui::Theme::Light);
        // The device runs an accessibility service, so the Android toolkits
        // keep their accessibility trees current every frame: egui does too.
        ctx.enable_accesskit();
        // Rows as tall as their content: nothing here is a button to keep
        // tall enough to touch.
        ctx.all_styles_mut(|style| style.spacing.interact_size = Vec2::ZERO);
        let tier = gauntlet_tier(load.tier);
        let avatars = (0..AVATAR_COUNT)
            .map(|index| {
                let size = AVATAR_SIZE as usize;
                let image = ColorImage::from_rgba_unmultiplied([size, size], &avatar_rgba(index));
                ctx.load_texture(format!("avatar{index}"), image, TextureOptions::LINEAR)
            })
            .collect();
        Self {
            load,
            tier,
            look: Look {
                s: tier.scale,
                bold: bold(),
            },
            frame: 0,
            frozen: false,
            posts: posts(),
            tickers: tickers(tier.tickers),
            avatars,
            anchor: (0, 8.0 * tier.scale),
            screen: Rect::ZERO,
            pixels: 1.0,
            first_frame_logged: false,
        }
    }

    fn top_bar(&self, ui: &mut Ui) {
        let (rect, _) = ui.allocate_exact_size(vec2(ui.available_width(), 56.0), Sense::hover());
        ui.painter().rect_filled(rect, 0.0, TOP_BAR);
        let title = Look {
            s: 1.0,
            bold: bold(),
        }
        .format(20.0, Color32::WHITE, true);
        let title = galley(ui, &[("Gauntlet", title)], rect.width(), 1);
        let at = pos2(rect.left() + 16.0, rect.center().y - title.size().y / 2.0);
        ui.painter().galley(at, title, Color32::PLACEHOLDER);
    }

    fn ticker_panel(&self, ui: &mut Ui) {
        let s = self.look.s;
        boxed(
            ui,
            [6.0 * s; 4],
            |ui| {
                ui.spacing_mut().item_spacing = vec2(4.0 * s, 4.0 * s);
                ui.horizontal_wrapped(|ui| {
                    for (index, ticker) in self.tickers.iter().enumerate() {
                        self.ticker_tile(ui, ticker, index);
                    }
                });
            },
            |rect| vec![Shape::rect_filled(rect, 0.0, PANEL)],
        );
    }

    fn ticker_tile(&self, ui: &mut Ui, ticker: &Ticker, index: usize) {
        let (s, look) = (self.look.s, &self.look);
        let cents = ticker_cents(ticker, self.frame);
        let change_color = if cents >= ticker.base_cents { UP } else { DOWN };
        let texts = [
            galley(
                ui,
                &[(&ticker.symbol, look.format(10.0, INK, true))],
                f32::INFINITY,
                1,
            ),
            galley(
                ui,
                &[(&cents_text(cents), look.format(10.0, BODY, false))],
                f32::INFINITY,
                1,
            ),
            galley(
                ui,
                &[(
                    &change_text(ticker, cents),
                    look.format(10.0, change_color, false),
                )],
                f32::INFINITY,
                1,
            ),
        ];
        let (gap, pad) = (4.0 * s, vec2(6.0 * s, 3.0 * s));
        let bar_size = vec2(20.0 * s, 4.0 * s);
        let height = texts
            .iter()
            .map(|text| text.size().y)
            .fold(bar_size.y, f32::max);
        let width = texts.iter().map(|text| text.size().x + gap).sum::<f32>() + bar_size.x;
        let rect = labeled(ui, vec2(width, height) + pad * 2.0, || {
            let mut label = String::new();
            for text in &texts {
                if !label.is_empty() {
                    label.push(' ');
                }
                label.push_str(text.text());
            }
            label
        });
        let painter = ui.painter();
        painter.rect_filled(rect, 6.0 * s, Color32::WHITE);
        let mut x = rect.left() + pad.x;
        for text in texts {
            let size = text.size();
            painter.galley(
                pos2(x, rect.center().y - size.y / 2.0),
                text,
                Color32::PLACEHOLDER,
            );
            x += size.x + gap;
        }
        let share = ((cents - ticker.base_cents + ticker.swing_cents) as f32
            / (2 * ticker.swing_cents) as f32)
            .clamp(0.0, 1.0);
        let track = Rect::from_min_size(pos2(x, rect.center().y - bar_size.y / 2.0), bar_size);
        painter.extend(bar(track, 2.0 * s, share, PALETTE[index % PALETTE.len()]));
    }

    /// Lays out the rows on screen from the anchor down, and moves the anchor
    /// past rows that scrolled off the top.
    fn list(&mut self, ui: &mut Ui) {
        let s = self.look.s;
        let area = ui.available_rect_before_wrap();
        ui.allocate_rect(area, Sense::hover());
        let offset = self.frame as f32 * SCROLL_PER_FRAME;
        let (pad, gap) = (8.0 * s, 8.0 * s);
        let mut list = ui.new_child(UiBuilder::new().max_rect(area));
        list.set_clip_rect(area);
        let (mut row, mut top) = self.anchor;
        while row < ROWS {
            let y = area.top() + top - offset;
            if y > area.bottom() {
                break;
            }
            let max = Rect::from_min_max(
                pos2(area.left() + pad, y),
                pos2(area.right() - pad, y + area.height()),
            );
            let height = list
                .scope_builder(UiBuilder::new().max_rect(max), |ui| self.row(ui, row))
                .response
                .rect
                .height();
            if (row, top) == self.anchor && top + height + gap <= offset {
                self.anchor = (row + 1, top + height + gap);
            }
            top += height + gap;
            row += 1;
        }
        self.layers(&list, area);
    }

    /// The translucent panels stacked over the list, laid out and painted
    /// again each frame as immediate mode does: every shape turned about its
    /// panel's center, and the nested card's turned back about its own.
    fn layers(&self, ui: &Ui, area: Rect) {
        let plain = Look {
            s: 1.0,
            bold: bold(),
        };
        let rows = self.tier.layer_rows;
        for layer in 0..self.tier.layers {
            let title = format!("Layer {}", layer + 1);
            let title = galley(
                ui,
                &[(&title, plain.format(13.0, Color32::WHITE, true))],
                f32::INFINITY,
                1,
            );
            let heading = format!("Nested in layer {}", layer + 1);
            let heading = galley(
                ui,
                &[(&heading, plain.format(11.0, INK, true))],
                f32::INFINITY,
                1,
            );
            let line = galley(
                ui,
                &[("Tilts against its panel", plain.format(11.0, BODY, false))],
                f32::INFINITY,
                1,
            );
            let cells_top = 10.0 + title.size().y + 8.0;
            let nested_top = cells_top + rows as f32 * 25.0 - 3.0 + 8.0;
            let nested_height = 8.0 + heading.size().y + 2.0 + line.size().y + 8.0;
            let origin = area.min + vec2(layer_x(layer, self.frame), layer_y(layer, self.frame));
            let panel = Rect::from_min_size(origin, vec2(220.0, nested_top + nested_height + 10.0));
            let nested =
                Rect::from_min_size(origin + vec2(10.0, nested_top), vec2(200.0, nested_height));
            let angle = layer_degrees(layer, self.frame).to_radians();
            let (center, turn) = (panel.center(), Rot2::from_angle(angle));
            let place = |point: Pos2| center + turn * (point - center);
            // Turned back about its own center, the nested card stays upright
            // where its panel moved that center.
            let shift = place(nested.center()) - nested.center();
            let place_nested = |point: Pos2| point + shift;
            let painter = ui.painter();
            painter.add(Shape::convex_polygon(
                rounded_outline(panel, 12.0, place),
                LAYER_BACKGROUND,
                Stroke::NONE,
            ));
            let at = origin + vec2(10.0, 10.0);
            announce(
                ui,
                Rect::from_min_size(at, title.size()),
                ("layer", layer),
                title.text(),
            );
            painter.add(TextShape::new(place(at), title, Color32::PLACEHOLDER).with_angle(angle));
            for cell in 0..rows * LAYER_COLUMNS {
                let at = origin
                    + vec2(
                        10.0 + (cell % LAYER_COLUMNS) as f32 * 25.0,
                        cells_top + (cell / LAYER_COLUMNS) as f32 * 25.0,
                    );
                let rect = Rect::from_min_size(at, Vec2::splat(22.0));
                painter.add(Shape::convex_polygon(
                    rounded_outline(rect, 4.0, place),
                    PALETTE[layer_cell_color(layer, cell)],
                    Stroke::NONE,
                ));
                let number = (cell + 1).to_string();
                let label = galley(
                    ui,
                    &[(&number, plain.format(9.0, Color32::WHITE, false))],
                    f32::INFINITY,
                    1,
                );
                announce(ui, rect, ("cell", layer, cell), &number);
                let at = rect.center() - label.size() / 2.0;
                painter
                    .add(TextShape::new(place(at), label, Color32::PLACEHOLDER).with_angle(angle));
            }
            painter.add(Shape::convex_polygon(
                rounded_outline(nested, 8.0, place_nested),
                NESTED_BACKGROUND,
                Stroke::NONE,
            ));
            let mut at = nested.min + vec2(8.0, 8.0);
            for (index, text) in [heading, line].into_iter().enumerate() {
                let rect = Rect::from_min_size(at, text.size());
                announce(ui, rect, ("nested", layer, index), text.text());
                at.y += text.size().y + 2.0;
                painter.add(TextShape::new(
                    place_nested(rect.min),
                    text,
                    Color32::PLACEHOLDER,
                ));
            }
        }
    }

    fn row(&self, ui: &mut Ui, row: usize) {
        match gauntlet_row(row, self.tier.columns) {
            GauntletRow::Cards(first) => {
                ui.spacing_mut().item_spacing.x = 8.0 * self.look.s;
                ui.columns(self.tier.columns, |columns| {
                    for (column, ui) in columns.iter_mut().enumerate() {
                        self.card(ui, first + column);
                    }
                });
            }
            GauntletRow::Cluster(cluster) => self.level(ui, cluster, self.tier.depth),
        }
    }

    fn card(&self, ui: &mut Ui, card: usize) {
        let (s, look, frame) = (self.look.s, &self.look, self.frame);
        let post = &self.posts[card % POST_COUNT];
        let radius = 12.0 * s;
        // Compose draws its border over the padding: the content is inset by
        // the padding alone and the border is painted inside the box.
        let rect = boxed(
            ui,
            [10.0 * s; 4],
            |ui| {
                ui.spacing_mut().item_spacing = vec2(0.0, 6.0 * s);
                let width = ui.available_width();
                self.card_header(ui, post, card, width);
                let body = galley(
                    ui,
                    &[(&post.body, look.paragraph(12.0, BODY, false))],
                    width,
                    4,
                );
                text(ui, body);
                self.progress(ui, card, width);
                self.sparkline(ui, card, PALETTE[post.color], width);
                self.chips(ui, post);
                self.footer(ui, post, width);
            },
            |rect| {
                let [ambient, spot] =
                    elevation_shadows(rect, radius, 3.0 * s, self.screen, self.pixels);
                vec![
                    ambient,
                    spot,
                    Shape::rect_filled(rect, radius, Color32::WHITE),
                    Shape::rect_stroke(
                        rect,
                        radius,
                        Stroke::new(1.0, HAIRLINE),
                        StrokeKind::Inside,
                    ),
                ]
            },
        );
        if card.is_multiple_of(5) {
            self.badge(ui, rect, card, badge_degrees(card, frame));
        }
    }

    fn card_header(&self, ui: &mut Ui, post: &Post, card: usize, width: f32) {
        let (s, look) = (self.look.s, &self.look);
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = 8.0 * s;
            let avatar = &self.avatars[card % AVATAR_COUNT];
            ui.add(
                egui::Image::new(avatar)
                    .fit_to_exact_size(Vec2::splat(32.0 * s))
                    .corner_radius(16.0 * s),
            );
            let column = width - 40.0 * s;
            ui.vertical(|ui| {
                ui.spacing_mut().item_spacing.y = 0.0;
                text(
                    ui,
                    galley(
                        ui,
                        &[(&post.title, look.paragraph(13.0, INK, true))],
                        column,
                        2,
                    ),
                );
                // `by Ada Lovelace · #rust`: the author bold, the tag in its color.
                let (tag, color) = &post.tags[0];
                let subtitle = [
                    ("by ", look.format(11.0, MUTED, false)),
                    (post.author.as_str(), look.format(11.0, MUTED, true)),
                    (" · ", look.format(11.0, MUTED, false)),
                    (tag.as_str(), look.format(11.0, GRADIENT_END[*color], false)),
                ];
                text(ui, galley(ui, &subtitle, column, 1));
            });
        });
    }

    fn progress(&self, ui: &mut Ui, card: usize, width: f32) {
        let (s, look) = (self.look.s, &self.look);
        let permille = progress_permille(card, self.frame);
        let label = galley(
            ui,
            &[(
                &format!("{}%", permille / 10),
                look.format(10.0, MUTED, false),
            )],
            width,
            1,
        );
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = 6.0 * s;
            let (track, _) = ui.allocate_exact_size(
                vec2(width - label.size().x - 6.0 * s, 6.0 * s),
                Sense::hover(),
            );
            ui.painter().extend(bar(
                track,
                3.0 * s,
                permille as f32 / 1000.0,
                PALETTE[card % PALETTE.len()],
            ));
            text(ui, label);
        });
    }

    /// A card's 48-point line over a fading fill: the fill is a mesh whose
    /// vertices carry the vertical gradient.
    fn sparkline(&self, ui: &mut Ui, card: usize, color: Color32, width: f32) {
        let (rect, _) = ui.allocate_exact_size(vec2(width, 36.0 * self.look.s), Sense::hover());
        let step = rect.width() / (GAUNTLET_SPARK_POINTS - 1) as f32;
        let points: Vec<Pos2> = (0..GAUNTLET_SPARK_POINTS)
            .map(|index| {
                let y = rect.top() + rect.height() * (1.0 - spark_value(card, index, self.frame));
                pos2(rect.left() + index as f32 * step, y)
            })
            .collect();
        let mut mesh = Mesh::default();
        for (index, point) in points.iter().enumerate() {
            let alpha = 0.25 * (1.0 - (point.y - rect.top()) / rect.height());
            mesh.colored_vertex(*point, color.gamma_multiply(alpha));
            mesh.colored_vertex(pos2(point.x, rect.bottom()), Color32::TRANSPARENT);
            if index > 0 {
                let at = (index * 2) as u32;
                mesh.add_triangle(at - 2, at - 1, at);
                mesh.add_triangle(at - 1, at + 1, at);
            }
        }
        let painter = ui.painter();
        painter.add(Shape::mesh(mesh));
        painter.add(Shape::line(points, Stroke::new(1.5 * self.look.s, color)));
    }

    fn chips(&self, ui: &mut Ui, post: &Post) {
        let (s, look) = (self.look.s, &self.look);
        ui.scope(|ui| {
            ui.spacing_mut().item_spacing = vec2(4.0 * s, 4.0 * s);
            ui.horizontal_wrapped(|ui| {
                for (tag, color) in &post.tags {
                    let label = galley(
                        ui,
                        &[(tag, look.format(10.0, GRADIENT_END[*color], false))],
                        f32::INFINITY,
                        1,
                    );
                    let pad = vec2(8.0 * s, 3.0 * s);
                    let rect = labeled(ui, label.size() + pad * 2.0, || tag.clone());
                    ui.painter()
                        .rect_filled(rect, 10.0 * s, CHIP_BACKGROUND[*color]);
                    ui.painter()
                        .galley(rect.min + pad, label, Color32::PLACEHOLDER);
                }
            });
        });
    }

    /// Three counters split by dividers as tall as the tallest counter: the
    /// counters are laid out first, then the dividers painted to their height.
    fn footer(&self, ui: &mut Ui, post: &Post, width: f32) {
        let look = &self.look;
        let column = (width - 2.0) / 3.0;
        let response = ui.horizontal_top(|ui| {
            ui.spacing_mut().item_spacing = Vec2::ZERO;
            let mut dividers = [0.0; 2];
            for (index, label) in FOOTER_LABELS.iter().enumerate() {
                if index > 0 {
                    dividers[index - 1] = ui.cursor().left();
                    ui.add_space(1.0);
                }
                ui.allocate_ui_with_layout(
                    vec2(column, 0.0),
                    Layout::top_down(Align::Center),
                    |ui| {
                        ui.set_width(column);
                        text(
                            ui,
                            galley(
                                ui,
                                &[(&post.stats[index], look.format(12.0, INK, true))],
                                column,
                                1,
                            ),
                        );
                        text(
                            ui,
                            galley(ui, &[(label, look.format(9.0, MUTED, false))], column, 1),
                        );
                    },
                );
            }
            dividers
        });
        let row = response.response.rect;
        for x in response.inner {
            ui.painter().rect_filled(
                Rect::from_min_max(pos2(x, row.top()), pos2(x + 1.0, row.bottom())),
                0.0,
                HAIRLINE,
            );
        }
    }

    /// A translucent tag tilting with the frame, in the card's top right
    /// corner: a rotated rounded box and rotated text.
    fn badge(&self, ui: &Ui, card: Rect, index: usize, degrees: f32) {
        let s = self.look.s;
        let label = galley(
            ui,
            &[("HOT", self.look.format(9.0, Color32::WHITE, true))],
            f32::INFINITY,
            1,
        );
        let pad = vec2(6.0 * s, 2.0 * s);
        let size = label.size() + pad * 2.0;
        let rect = Rect::from_min_size(
            pos2(card.right() - 6.0 * s - size.x, card.top() + 6.0 * s),
            size,
        );
        announce(ui, rect, ("badge", index), "HOT");
        let (center, angle) = (rect.center(), degrees.to_radians());
        let rotate = |point: Pos2| center + Rot2::from_angle(angle) * (point - center);
        let painter = ui.painter();
        painter.add(Shape::convex_polygon(
            rounded_outline(rect, 8.0 * s, rotate),
            PALETTE[0].gamma_multiply(0.9),
            Stroke::NONE,
        ));
        let text = TextShape::new(rotate(rect.min + pad), label, Color32::PLACEHOLDER)
            .with_angle(angle)
            .with_opacity_factor(0.9);
        painter.add(text);
    }

    /// `depth` levels nested inside one another, each a row of three labels
    /// above the next level.
    fn level(&self, ui: &mut Ui, cluster: usize, remaining: usize) {
        let (s, look) = (self.look.s, &self.look);
        boxed(
            ui,
            [3.0 * s, s, s, s],
            |ui| {
                ui.spacing_mut().item_spacing = vec2(2.0 * s, s);
                ui.columns(3, |columns| {
                    for (chip, ui) in columns.iter_mut().enumerate() {
                        let width = ui.available_width();
                        let label = format!("C{cluster}.L{remaining}.{chip}");
                        let label = galley(
                            ui,
                            &[(&label, look.format(9.0, Color32::WHITE, false))],
                            width,
                            usize::MAX,
                        );
                        let rect =
                            labeled(ui, vec2(width, label.size().y), || label.text().to_owned());
                        let color = PALETTE[(remaining + chip + cluster) % PALETTE.len()];
                        ui.painter().rect_filled(rect, 2.0 * s, color);
                        ui.painter().galley(rect.min, label, Color32::PLACEHOLDER);
                    }
                });
                if remaining > 0 {
                    self.level(ui, cluster, remaining - 1);
                }
            },
            |rect| {
                vec![Shape::rect_filled(
                    rect,
                    4.0 * s,
                    LEVEL_BACKGROUND[remaining % 2],
                )]
            },
        );
    }
}

impl eframe::App for Gauntlet {
    fn ui(&mut self, ui: &mut Ui, _frame: &mut eframe::Frame) {
        if !self.frozen {
            self.frame += 1;
        }
        let screen = ui.max_rect();
        self.screen = screen;
        self.pixels = ui.ctx().pixels_per_point();
        ui.painter().rect_filled(screen, 0.0, BACKGROUND);
        ui.spacing_mut().item_spacing = Vec2::ZERO;
        self.top_bar(ui);
        // The content's width follows the frame, in whole pixels.
        let pixels = ui.ctx().pixels_per_point();
        let width = (screen.width() * pixels * width_fraction(self.frame)).round() / pixels;
        let rest = ui.available_rect_before_wrap();
        let content = Rect::from_min_size(rest.min, vec2(width, rest.height()));
        ui.scope_builder(
            UiBuilder::new()
                .max_rect(content)
                .layout(Layout::top_down(Align::Min)),
            |ui| {
                ui.spacing_mut().item_spacing = Vec2::ZERO;
                self.ticker_panel(ui);
                self.list(ui);
            },
        );
        if !self.first_frame_logged {
            self.first_frame_logged = true;
            log::info!("PERF first_frame");
        }
        if self.frozen {
            return;
        }
        if self.load.freeze > 0 && self.frame >= self.load.freeze {
            self.frozen = true;
            log::info!("PERF frozen frame={}", self.frame);
        } else {
            ui.ctx().request_repaint();
        }
    }
}

/// Runs the gauntlet in eframe's window with `options`.
pub fn run(options: eframe::NativeOptions, load: Launch) {
    let result = eframe::run_native(
        "Gauntlet",
        options,
        Box::new(move |creation| Ok(Box::new(Gauntlet::new(&creation.egui_ctx, load)))),
    );
    if let Err(error) = result {
        log::error!("eframe stopped: {error}");
    }
}

/// Runs the gauntlet the launch activity asked for.
#[cfg(target_os = "android")]
#[unsafe(no_mangle)]
fn android_main(app: winit::platform::android::activity::AndroidApp) {
    android_logger::init_once(
        android_logger::Config::default()
            .with_max_level(log::LevelFilter::Info)
            .with_tag("PerfCompare"),
    );
    let load = Launch::read(app.internal_data_path());
    run(
        eframe::NativeOptions {
            android_app: Some(app),
            ..Default::default()
        },
        load,
    );
}
