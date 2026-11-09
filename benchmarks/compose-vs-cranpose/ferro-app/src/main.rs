//! The gauntlet in FerroUI: the screen `compose-app/.../Gauntlet.kt` draws, in
//! FerroUI's own idiom: retained controls from builders, whose per-frame
//! values follow one frame signal, so a frame sets only what changed. The
//! benchmark's README describes it; `desktop.py` runs it:
//! `PERF_TIER=16 PERF_FONTS=../fonts cargo run --release`.
//!
//! Every frame advances a frame index and everything follows from it, never
//! from wall time. The list holds the rows from the one at the top of the
//! window to its bottom, kept by key as the window moves; each row reports
//! its height once laid out.

mod wall;

use std::{cell::RefCell, collections::HashMap, io::Cursor, rc::Rc};

use ferroui_base::{
    PixelSize, Ref,
    layout::{HorizontalAlignment, Orientation, VerticalAlignment},
    media::{
        BoxShadow, BoxShadows, FontFamily, FontManager, FontWeight, IImage, ITransform, Points,
        RotateTransform, TextTrimming, TextWrapping, TransformGroup, TranslateTransform,
        fonts::{FontCollectionBase, FontCollectionBaseImpl, IFontCollection},
        imaging::Bitmap,
    },
    platform::{AlphaFormat, PixelFormats},
    utilities::Uri,
};
use ferroui_controls::{Border, ColumnDefinitions, Grid, StackPanel};
use ferroui_view::prelude::*;
use perf_data::*;

const FAMILY: &str = "fonts:Device#Roboto";
const FOOTER_LABELS: [&str; 3] = ["likes", "replies", "shares"];
const INK: &str = "#111827";
const BODY: &str = "#374151";
const MUTED: &str = "#6B7280";
const HAIRLINE: &str = "#E5E7EB";

fn hex(value: [u8; 3]) -> String {
    format!("#{:02X}{:02X}{:02X}", value[0], value[1], value[2])
}

fn palette(index: usize) -> String {
    hex(PALETTE_RGB[index % PALETTE_RGB.len()])
}

/// Roboto from the files the other apps load.
struct DeviceRoboto {
    base: FontCollectionBase,
    key: Uri,
}

impl FontCollectionBaseImpl for DeviceRoboto {
    fn base(this: &Self) -> &FontCollectionBase {
        &this.base
    }

    fn key(this: &Self) -> Uri {
        this.key.clone()
    }
}

fn register_fonts() {
    let collection = Rc::new(DeviceRoboto {
        base: FontCollectionBase::new(),
        key: Uri::absolute("fonts:Device").expect("a font collection key"),
    });
    for file in ["Roboto-Regular.ttf", "Roboto-Bold.ttf"] {
        match font_bytes(file) {
            Ok(bytes) => {
                FontCollectionBase::try_add_glyph_typeface_from_stream(
                    &*collection,
                    &mut Cursor::new(bytes),
                );
            }
            Err(error) => log::warn!("no {file}: {error}"),
        }
    }
    let collection: Rc<dyn IFontCollection> = collection;
    FontManager::current().add_font_collection(collection);
}

/// A line of text in Roboto at `size`.
fn txt(
    content: impl Into<String>,
    size: f64,
    color: &str,
    bold: bool,
) -> ferroui_view::El<ferroui_controls::TextBlock> {
    text_block()
        .text(content.into())
        .font_family(FontFamily::from(FAMILY))
        .font_size(size)
        .foreground(color)
        .font_weight(if bold {
            FontWeight::Bold
        } else {
            FontWeight::Normal
        })
}

/// Wrapped text cut after `lines` lines, lines 1.4 em apart.
fn paragraph(
    content: impl Into<String>,
    size: f64,
    color: &str,
    bold: bool,
    lines: i32,
) -> ferroui_view::El<ferroui_controls::TextBlock> {
    txt(content, size, color, bold)
        .line_height(size * 1.4)
        .max_lines(lines)
        .text_wrapping(TextWrapping::Wrap)
        .text_trimming(<dyn TextTrimming>::character_ellipsis())
}

/// A rounded track filled to `share`, a fraction of its width. A track of a
/// width of its own sets the fill's width in points; one that takes the room
/// there is splits it between the fill and what is left, as star columns.
fn bar(width: Option<f64>, height: f64, share: impl Fn() -> f64 + 'static, fill: &str) -> Element {
    let track = border()
        .height(height)
        .background(HAIRLINE)
        .corner_radius(height / 2.0)
        .clip_to_bounds(true);
    let fill = border()
        .height(height)
        .background(fill)
        .corner_radius(height / 2.0);
    let share = Rc::new(share);
    match width {
        Some(width) => track
            .width(width)
            .child(
                fill.horizontal_alignment(HorizontalAlignment::Left)
                    .width(move || width * share().clamp(0.0, 1.0)),
            )
            .into(),
        None => track
            .child(
                grid()
                    .with(move |control: &Ref<Grid>| {
                        let (control, share) = (control.clone(), share.clone());
                        effect(move || {
                            let share = share().clamp(0.0, 1.0);
                            let columns =
                                ColumnDefinitions::parse(&format!("{share}*,{}*", 1.0 - share))
                                    .expect("two star columns");
                            control.set_column_definitions(columns);
                        });
                    })
                    .child(fill),
            )
            .into(),
    }
}

/// `child` in column `column` of a grid.
fn cell(column: usize, child: impl IntoElement) -> ferroui_view::El<Border> {
    border().grid_column(column as i32).child(child)
}

/// `count` equal columns `gap` apart: child `n` goes in column `2n`.
fn columns(count: usize, gap: f64) -> String {
    (0..count)
        .map(|_| "*".to_owned())
        .collect::<Vec<_>>()
        .join(&format!(",{gap},"))
}

/// A control's render transform: the rotation and the translation it follows.
fn tilt_and_shift(
    control: &Ref<Border>,
    degrees: impl Fn() -> f64 + 'static,
    shift: impl Fn() -> (f64, f64) + 'static,
) {
    let rotate = RotateTransform::new();
    let translate = TranslateTransform::new();
    let group = TransformGroup::new();
    group.children().add(rotate.clone().upcast());
    group.children().add(translate.clone().upcast());
    let transform: Rc<dyn ITransform> = group.into();
    control.set_render_transform(Some(transform));
    effect(move || rotate.set_angle(degrees()));
    effect(move || {
        let (x, y) = shift();
        translate.set_x(x);
        translate.set_y(y);
    });
}

/// What every frame advances and the pieces read.
#[derive(Clone)]
struct Shared {
    frame: Signal<u32>,
    anchor: Signal<ListAnchor>,
    /// The rows the list holds: from the anchor down to the window's bottom.
    window: Signal<std::ops::Range<usize>>,
    heights: Rc<RefCell<HashMap<usize, f32>>>,
    avatars: Rc<Vec<Rc<dyn IImage>>>,
}

/// How far `frame` has scrolled the list.
fn scroll_offset(frame: u32) -> f32 {
    frame as f32 * SCROLL_PER_FRAME
}

fn avatar(index: usize) -> Rc<dyn IImage> {
    let bitmap = Bitmap::from_pixels(
        PixelFormats::RGBA8888,
        AlphaFormat::Opaque,
        &avatar_rgba(index),
        PixelSize::new(AVATAR_SIZE as i32, AVATAR_SIZE as i32),
        ferroui_base::Vector::new(96.0, 96.0),
        (AVATAR_SIZE * 4) as i32,
    );
    Rc::new(bitmap)
}

fn gauntlet(shared: Shared) -> Element {
    let tier = desktop_gauntlet().tier;
    let s = f64::from(tier.scale);
    let frame = shared.frame;
    let (width, _) = DESKTOP_WINDOW;
    let rows = {
        let window = shared.window;
        for_each(
            move || window.get().collect::<Vec<_>>(),
            |row| format!("row{row}"),
            {
                let shared = shared.clone();
                move |row| row_view(row, shared.clone())
            },
        )
    };
    let list = {
        let anchor = shared.anchor;
        stack_panel()
            .spacing(8.0 * s)
            .margin((8.0 * s, 0.0, 8.0 * s, 0.0))
            .horizontal_alignment(HorizontalAlignment::Stretch)
            .vertical_alignment(VerticalAlignment::Top)
            .with(move |control: &Ref<StackPanel>| {
                let translate = TranslateTransform::new();
                let transform: Rc<dyn ITransform> = translate.clone().into();
                control.set_render_transform(Some(transform));
                effect(move || {
                    let offset = scroll_offset(frame.get());
                    translate.set_y(f64::from(anchor.get().top - offset));
                });
            })
            .child(rows)
    };
    let area = grid().clip_to_bounds(true).child(list).children(
        (0..tier.layers)
            .map(|layer| stacked_layer(layer, frame))
            .collect::<Vec<_>>(),
    );
    let content = grid()
        .row_definitions("Auto,*")
        .horizontal_alignment(HorizontalAlignment::Left)
        .width(move || (f64::from(width) * f64::from(width_fraction(frame.get()))).round())
        .child(ticker_panel(frame, s))
        .child(area.grid_row(1));
    grid()
        .row_definitions("56,*")
        .background("#EEF0F5")
        .child(
            border()
                .background("#1E2A4A")
                .padding((16.0, 0.0, 16.0, 0.0))
                .child(
                    txt("Gauntlet", 20.0, "#FFFFFF", true)
                        .vertical_alignment(VerticalAlignment::Center),
                ),
        )
        .child(content.grid_row(1))
        .into()
}

fn ticker_panel(frame: Signal<u32>, s: f64) -> Element {
    let tiles = desktop_gauntlet()
        .tickers
        .iter()
        .enumerate()
        .map(|(index, ticker)| {
            border()
                .background("#FFFFFF")
                .corner_radius(6.0 * s)
                .padding((6.0 * s, 3.0 * s, 6.0 * s, 3.0 * s))
                .child(
                    stack_panel()
                        .orientation(Orientation::Horizontal)
                        .spacing(4.0 * s)
                        .child(
                            txt(ticker.symbol.clone(), 10.0 * s, INK, true)
                                .vertical_alignment(VerticalAlignment::Center),
                        )
                        .child(
                            txt("", 10.0 * s, BODY, false)
                                .text(move || cents_text(ticker_cents(ticker, frame.get())))
                                .vertical_alignment(VerticalAlignment::Center),
                        )
                        .child(
                            txt("", 10.0 * s, UP_HEX, false)
                                .text(move || {
                                    change_text(ticker, ticker_cents(ticker, frame.get()))
                                })
                                .foreground(move || {
                                    if ticker_cents(ticker, frame.get()) >= ticker.base_cents {
                                        UP_HEX
                                    } else {
                                        DOWN_HEX
                                    }
                                })
                                .vertical_alignment(VerticalAlignment::Center),
                        )
                        .child(bar(
                            Some(20.0 * s),
                            4.0 * s,
                            move || {
                                let cents = ticker_cents(ticker, frame.get());
                                f64::from(cents - ticker.base_cents + ticker.swing_cents)
                                    / f64::from(2 * ticker.swing_cents)
                            },
                            palette(index).as_str(),
                        )),
                )
        })
        .collect::<Vec<_>>();
    border()
        .background("#E2E8F0")
        .padding(6.0 * s)
        .child(
            wrap_panel()
                .item_spacing(4.0 * s)
                .line_spacing(4.0 * s)
                .children(tiles),
        )
        .into()
}

const UP_HEX: &str = "#16A34A";
const DOWN_HEX: &str = "#DC2626";

fn row_view(row: usize, shared: Shared) -> Element {
    let tier = desktop_gauntlet().tier;
    let s = f64::from(tier.scale);
    let content: Element = match gauntlet_row(row, tier.columns) {
        GauntletRow::Cards(first) => grid()
            .column_definitions(&columns(tier.columns, 8.0 * s))
            .children(
                (first..first + tier.columns)
                    .enumerate()
                    .map(|(column, card)| cell(2 * column, card_view(card, shared.clone(), s)))
                    .collect::<Vec<_>>(),
            )
            .into(),
        GauntletRow::Cluster(cluster) => level(cluster, tier.depth, s),
    };
    let heights = shared.heights.clone();
    border()
        .child(content)
        .on_size_changed(move |args| {
            heights
                .borrow_mut()
                .insert(row, args.new_size().height as f32);
        })
        .into()
}

fn card_view(card: usize, shared: Shared, s: f64) -> Element {
    let frame = shared.frame;
    let post = &desktop_gauntlet().posts[card % POST_COUNT];
    let body = border()
        .background("#FFFFFF")
        .corner_radius(12.0 * s)
        .border_thickness(1.0)
        .border_brush(HAIRLINE)
        .box_shadow(BoxShadows::new(
            BoxShadow::parse(&format!("0 {s} {} 0 #3D000000", 3.0 * s)).expect("a shadow"),
        ))
        .padding(10.0 * s)
        .child(
            stack_panel()
                .spacing(6.0 * s)
                .child(card_header(card, &shared, s))
                .child(paragraph(post.body.clone(), 12.0 * s, BODY, false, 4))
                .child(card_chips(post, s))
                .child(card_progress(card, frame, s))
                .child(sparkline(card, frame, palette(post.color), s))
                .child(card_footer(post, s)),
        );
    if card.is_multiple_of(5) {
        grid().child(body).child(card_badge(card, frame, s)).into()
    } else {
        body.into()
    }
}

/// The avatar, the title and the subtitle.
fn card_header(card: usize, shared: &Shared, s: f64) -> ferroui_view::El<ferroui_controls::Grid> {
    let post = &desktop_gauntlet().posts[card % POST_COUNT];
    let (tag, tag_color) = &post.tags[0];
    // `by Ada Lovelace · #rust`: the author bold, the tag in its color.
    let subtitle = stack_panel()
        .orientation(Orientation::Horizontal)
        .child(txt("by ", 11.0 * s, MUTED, false))
        .child(txt(post.author.clone(), 11.0 * s, MUTED, true))
        .child(txt(" · ", 11.0 * s, MUTED, false))
        .child(txt(
            tag.clone(),
            11.0 * s,
            hex(GRADIENT_END_RGB[*tag_color]).as_str(),
            false,
        ));
    let picture = border()
        .width(32.0 * s)
        .height(32.0 * s)
        .corner_radius(16.0 * s)
        .clip_to_bounds(true)
        .child(
            image()
                .source(shared.avatars.get(card % AVATAR_COUNT).cloned())
                .stretch(ferroui_base::media::Stretch::Fill),
        );
    grid()
        .column_definitions(&format!("{},{},*", 32.0 * s, 8.0 * s))
        .child(picture.vertical_alignment(VerticalAlignment::Center))
        .child(
            stack_panel()
                .child(paragraph(post.title.clone(), 13.0 * s, INK, true, 2))
                .child(subtitle)
                .vertical_alignment(VerticalAlignment::Center)
                .grid_column(2),
        )
}

fn card_chips(post: &Post, s: f64) -> ferroui_view::El<ferroui_controls::WrapPanel> {
    wrap_panel()
        .item_spacing(4.0 * s)
        .line_spacing(4.0 * s)
        .children(
            post.tags
                .iter()
                .map(|(value, shade)| {
                    border()
                        .background(hex(CHIP_BACKGROUND_RGB[*shade]).as_str())
                        .corner_radius(10.0 * s)
                        .padding((8.0 * s, 3.0 * s, 8.0 * s, 3.0 * s))
                        .child(txt(
                            value.clone(),
                            10.0 * s,
                            hex(GRADIENT_END_RGB[*shade]).as_str(),
                            false,
                        ))
                })
                .collect::<Vec<_>>(),
        )
}

fn card_progress(
    card: usize,
    frame: Signal<u32>,
    s: f64,
) -> ferroui_view::El<ferroui_controls::Grid> {
    let permille = move || progress_permille(card, frame.get());
    grid()
        .column_definitions(&format!("*,{},Auto", 6.0 * s))
        .child(cell(
            0,
            bar(
                None,
                6.0 * s,
                move || f64::from(permille()) / 1000.0,
                palette(card).as_str(),
            ),
        ))
        .child(
            txt("", 10.0 * s, MUTED, false)
                .text(move || format!("{}%", permille() / 10))
                .vertical_alignment(VerticalAlignment::Center)
                .grid_column(2),
        )
}

/// Three counters split by dividers as tall as the tallest counter.
fn card_footer(post: &Post, s: f64) -> ferroui_view::El<ferroui_controls::Grid> {
    let mut footer = grid().column_definitions("*,1,*,1,*");
    for (index, label) in FOOTER_LABELS.iter().enumerate() {
        if index > 0 {
            footer = footer.child(
                border()
                    .background(HAIRLINE)
                    .grid_column((2 * index - 1) as i32),
            );
        }
        footer = footer.child(
            stack_panel()
                .horizontal_alignment(HorizontalAlignment::Center)
                .child(
                    txt(post.stats[index].clone(), 12.0 * s, INK, true)
                        .horizontal_alignment(HorizontalAlignment::Center),
                )
                .child(
                    txt(*label, 9.0 * s, MUTED, false)
                        .horizontal_alignment(HorizontalAlignment::Center),
                )
                .grid_column((2 * index) as i32),
        );
    }
    footer
}

/// A translucent tag tilting with the frame: turned, never laid out again.
fn card_badge(card: usize, frame: Signal<u32>, s: f64) -> ferroui_view::El<Border> {
    border()
        .background("#E6EF4444")
        .corner_radius(8.0 * s)
        .padding((6.0 * s, 2.0 * s, 6.0 * s, 2.0 * s))
        .margin(6.0 * s)
        .horizontal_alignment(HorizontalAlignment::Right)
        .vertical_alignment(VerticalAlignment::Top)
        .child(txt("HOT", 9.0 * s, "#E6FFFFFF", true))
        .with(move |control: &Ref<Border>| {
            let rotate = RotateTransform::new();
            let transform: Rc<dyn ITransform> = rotate.clone().into();
            control.set_render_transform(Some(transform));
            effect(move || rotate.set_angle(f64::from(badge_degrees(card, frame.get()))));
        })
}

/// A card's 48-point sparkline in the card's width at the frame's width: a
/// line over a translucent fill.
fn sparkline(card: usize, frame: Signal<u32>, color: String, s: f64) -> Element {
    let tier = desktop_gauntlet().tier;
    let height = 36.0 * s;
    let width = move || {
        let (window, _) = DESKTOP_WINDOW;
        let content =
            (f64::from(window) * f64::from(width_fraction(frame.get_untracked()))).round();
        let gap = 8.0 * s;
        let column =
            (content - 2.0 * gap - (tier.columns as f64 - 1.0) * gap) / tier.columns as f64;
        column - 2.0 * 10.0 * s - 2.0
    };
    let line = move || {
        let (width, frame) = (width(), frame.get());
        let step = width / (GAUNTLET_SPARK_POINTS - 1) as f64;
        Some(Points::from_items((0..GAUNTLET_SPARK_POINTS).map(
            |index| {
                ferroui_base::Point::new(
                    index as f64 * step,
                    height * (1.0 - f64::from(spark_value(card, index, frame))),
                )
            },
        )))
    };
    let area = move || {
        let (width, frame) = (width(), frame.get());
        let step = width / (GAUNTLET_SPARK_POINTS - 1) as f64;
        let mut points = vec![ferroui_base::Point::new(0.0, height)];
        points.extend((0..GAUNTLET_SPARK_POINTS).map(|index| {
            ferroui_base::Point::new(
                index as f64 * step,
                height * (1.0 - f64::from(spark_value(card, index, frame))),
            )
        }));
        points.push(ferroui_base::Point::new(width, height));
        Some(Points::from_items(points))
    };
    let faded = format!("#40{}", &color[1..]);
    canvas()
        .height(height)
        .clip_to_bounds(true)
        .child(polygon().points(area).fill(faded.as_str()))
        .child(
            polyline()
                .points(line)
                .stroke(color.as_str())
                .stroke_thickness(1.5 * s),
        )
        .into()
}

/// Levels nested inside one another, each a row of three labels above the
/// next level.
fn level(cluster: usize, remaining: usize, s: f64) -> Element {
    let chips = (0..3)
        .map(|chip| {
            border()
                .background(palette(remaining + chip + cluster).as_str())
                .corner_radius(2.0 * s)
                .child(txt(
                    format!("C{cluster}.L{remaining}.{chip}"),
                    9.0 * s,
                    "#FFFFFF",
                    false,
                ))
                .grid_column((2 * chip) as i32)
        })
        .collect::<Vec<_>>();
    let mut stack = stack_panel().spacing(s).child(
        grid()
            .column_definitions(&columns(3, 2.0 * s))
            .children(chips),
    );
    if remaining > 0 {
        stack = stack.child(level(cluster, remaining - 1, s));
    }
    border()
        .background(hex(LEVEL_BACKGROUND_RGB[remaining % 2]).as_str())
        .corner_radius(4.0 * s)
        .padding((3.0 * s, s, s, s))
        .child(stack)
        .into()
}

/// A translucent panel stacked over the list. Its controls never change: each
/// frame sets only the panel's translation and tilt and its nested card's
/// tilt.
fn stacked_layer(layer: usize, frame: Signal<u32>) -> Element {
    let rows = desktop_gauntlet().tier.layer_rows;
    let cells = (0..rows)
        .map(|line| {
            stack_panel()
                .orientation(Orientation::Horizontal)
                .spacing(3.0)
                .children(
                    (0..LAYER_COLUMNS)
                        .map(|column| {
                            let cell = line * LAYER_COLUMNS + column;
                            border()
                                .width(22.0)
                                .height(22.0)
                                .corner_radius(4.0)
                                .background(palette(layer_cell_color(layer, cell)).as_str())
                                .child(
                                    txt((cell + 1).to_string(), 9.0, "#FFFFFF", false)
                                        .horizontal_alignment(HorizontalAlignment::Center)
                                        .vertical_alignment(VerticalAlignment::Center),
                                )
                        })
                        .collect::<Vec<_>>(),
                )
        })
        .collect::<Vec<_>>();
    let nested = border()
        .background("#E6FFFFFF")
        .corner_radius(8.0)
        .padding(8.0)
        .child(
            stack_panel()
                .spacing(2.0)
                .child(txt(
                    format!("Nested in layer {}", layer + 1),
                    11.0,
                    INK,
                    true,
                ))
                .child(txt("Tilts against its panel", 11.0, BODY, false)),
        )
        .with(move |control: &Ref<Border>| {
            let rotate = RotateTransform::new();
            let transform: Rc<dyn ITransform> = rotate.clone().into();
            control.set_render_transform(Some(transform));
            effect(move || rotate.set_angle(-f64::from(layer_degrees(layer, frame.get()))));
        });
    border()
        .width(220.0)
        .background("#C01E293B")
        .corner_radius(12.0)
        .padding(10.0)
        .horizontal_alignment(HorizontalAlignment::Left)
        .vertical_alignment(VerticalAlignment::Top)
        .with(move |control: &Ref<Border>| {
            tilt_and_shift(
                control,
                move || f64::from(layer_degrees(layer, frame.get())),
                move || {
                    let frame = frame.get();
                    (
                        f64::from(layer_x(layer, frame)),
                        f64::from(layer_y(layer, frame)),
                    )
                },
            );
        })
        .child(
            stack_panel()
                .spacing(8.0)
                .child(txt(format!("Layer {}", layer + 1), 13.0, "#FFFFFF", true))
                .child(stack_panel().spacing(3.0).children(cells))
                .child(nested),
        )
        .into()
}

fn main() {
    perf_data::log_to_stdout();
    let load = perf_data::launch();
    let shared: Rc<RefCell<Option<Shared>>> = Rc::new(RefCell::new(None));
    let for_view = shared.clone();
    launch_themed(
        || {
            register_fonts();
            ferroui_themes_simple::SimpleTheme::new().as_style()
        },
        move || {
            let tier = desktop_gauntlet().tier;
            let state = Shared {
                frame: signal(0u32),
                anchor: signal(ListAnchor {
                    row: 0,
                    top: 8.0 * tier.scale,
                }),
                window: signal(0..10),
                heights: Rc::new(RefCell::new(HashMap::new())),
                avatars: Rc::new((0..AVATAR_COUNT).map(avatar).collect()),
            };
            *for_view.borrow_mut() = Some(state.clone());
            let (width, height) = DESKTOP_WINDOW;
            let screen = match load.scenario {
                Scenario::Gauntlet => gauntlet(state),
                Scenario::Wall => wall::wall(state.frame),
            };
            window()
                .title(load.scenario.title())
                .width(f64::from(width))
                .height(f64::from(height))
                .can_resize(false)
                .child(screen)
                .into()
        },
        move |window| {
            let shared = shared.borrow().clone().expect("the view was made");
            let window = window.clone();
            let request =
                move |action: Box<dyn FnOnce()>| window.request_animation_frame(move |_| action());
            tick(request, shared, load);
        },
    );
}

/// Advances the frame once per animation frame until the freeze frame.
fn tick<R: Fn(Box<dyn FnOnce()>) + Clone + 'static>(request: R, shared: Shared, load: Launch) {
    let again = request.clone();
    request(Box::new(move || {
        let frame = shared.frame.get_untracked() + 1;
        if frame == 1 {
            log::info!("PERF first_frame");
        }
        if load.scenario == Scenario::Gauntlet {
            let s = desktop_gauntlet().tier.scale;
            let anchor = shared.anchor.get_untracked();
            let offset = scroll_offset(frame);
            let advanced = {
                let heights = shared.heights.borrow();
                anchor.advanced(offset, 8.0 * s, |row| heights.get(&row).copied())
            };
            if advanced != anchor {
                shared
                    .heights
                    .borrow_mut()
                    .retain(|measured, _| *measured >= advanced.row);
                shared.anchor.set(advanced);
            }
            // A row not laid out yet counts as the average of those that are.
            let needed = {
                let heights = shared.heights.borrow();
                let estimate = if heights.is_empty() {
                    100.0 * s
                } else {
                    heights.values().sum::<f32>() / heights.len() as f32
                };
                let (_, window_height) = DESKTOP_WINDOW;
                advanced.rows(offset, window_height as f32, 8.0 * s, estimate, |row| {
                    heights.get(&row).copied()
                })
            };
            if needed != shared.window.get_untracked() {
                shared.window.set(needed);
            }
        }
        shared.frame.set(frame);
        if load.freeze > 0 && frame >= load.freeze {
            log::info!("PERF frozen frame={frame}");
            return;
        }
        tick(again, shared, load);
    }));
}
