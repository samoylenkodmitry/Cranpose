//! The gauntlet in Dioxus: the screen `compose-app/.../Gauntlet.kt` draws,
//! element for element, in Dioxus's own idiom on its desktop renderer:
//! components in RSX over the web page's CSS (`web-app/www/style.css`), in
//! the system's WKWebView. The benchmark's README describes it; `desktop.py`
//! runs it: `PERF_TIER=16 PERF_FONTS=../fonts cargo run --release`.
//!
//! Every frame advances a frame index and everything follows from it, never
//! from wall time. The index is a global signal: each animation frame of the
//! page, which the app hears through `eval`, advances it; the components that
//! read it render again, and Dioxus sends the webview what changed. The list
//! renders the rows from the first one on screen down to the window's bottom,
//! and each row reports its height once the page has laid it out.

use std::{borrow::Cow, collections::HashMap, fmt::Write, sync::OnceLock};

use dioxus::{
    desktop::{
        Config, LogicalSize, WindowBuilder,
        wry::http::{Request, Response},
    },
    prelude::*,
};
use perf_data::*;

/// The page's layout, which the web page shares.
const STYLE: &str = include_str!("../../web-app/www/style.css");
const FOOTER_LABELS: [&str; 3] = ["likes", "replies", "shares"];
/// Sends a tick on each animation frame of the page and waits for the app's
/// answer, so the frame index advances once per frame the page draws.
const FRAMES: &str = "while (true) { await new Promise(requestAnimationFrame); dioxus.send(0); await dioxus.recv(); }";
/// Where the avatars and the Roboto files are served from.
const ASSETS: &str = "gauntlet";

static FRAME: GlobalSignal<u32> = Signal::global(|| 0);
/// Where the list starts, its first row's top below the list's padding.
static ANCHOR: GlobalSignal<ListAnchor> = Signal::global(|| ListAnchor {
    row: 0,
    top: 8.0 * desktop_gauntlet().tier.scale,
});
/// The heights rows reported, from the anchor down.
static HEIGHTS: GlobalSignal<HashMap<usize, f32>> = Signal::global(HashMap::new);

/// The colors as CSS takes them, made once.
struct Colors {
    palette: Vec<String>,
    chip_background: Vec<String>,
    gradient_end: Vec<String>,
    level_background: Vec<String>,
}

fn hex(rgb: &[u8; 3]) -> String {
    format!("#{:02X}{:02X}{:02X}", rgb[0], rgb[1], rgb[2])
}

fn colors() -> &'static Colors {
    static COLORS: OnceLock<Colors> = OnceLock::new();
    COLORS.get_or_init(|| Colors {
        palette: PALETTE_RGB.iter().map(hex).collect(),
        chip_background: CHIP_BACKGROUND_RGB.iter().map(hex).collect(),
        gradient_end: GRADIENT_END_RGB.iter().map(hex).collect(),
        level_background: LEVEL_BACKGROUND_RGB.iter().map(hex).collect(),
    })
}

/// The tier's scale, as CSS lengths take it.
fn scale() -> f64 {
    f64::from(desktop_gauntlet().tier.scale)
}

/// Avatar `index` as a BMP: 32-bit pixels, rows top down.
fn avatar_bmp(index: usize) -> Vec<u8> {
    let rgba = avatar_rgba(index);
    let side = AVATAR_SIZE as i32;
    let pixels = rgba.len() as u32;
    let mut bmp = Vec::with_capacity(54 + rgba.len());
    bmp.extend_from_slice(b"BM");
    bmp.extend_from_slice(&(54 + pixels).to_le_bytes());
    bmp.extend_from_slice(&[0; 4]);
    bmp.extend_from_slice(&54u32.to_le_bytes());
    bmp.extend_from_slice(&40u32.to_le_bytes());
    bmp.extend_from_slice(&side.to_le_bytes());
    bmp.extend_from_slice(&(-side).to_le_bytes());
    bmp.extend_from_slice(&1u16.to_le_bytes());
    bmp.extend_from_slice(&32u16.to_le_bytes());
    bmp.extend_from_slice(&0u32.to_le_bytes());
    bmp.extend_from_slice(&pixels.to_le_bytes());
    bmp.extend_from_slice(&[0; 16]);
    for pixel in rgba.chunks_exact(4) {
        bmp.extend_from_slice(&[pixel[2], pixel[1], pixel[0], pixel[3]]);
    }
    bmp
}

/// Serves `/avatar/N` and `/font/FILE` to the page.
fn serve(request: &Request<Vec<u8>>) -> Response<Cow<'static, [u8]>> {
    let path = request.uri().path();
    let found = if let Some(index) = path.strip_prefix("/avatar/") {
        index
            .parse::<usize>()
            .ok()
            .map(|index| ("image/bmp", avatar_bmp(index)))
    } else if let Some(file) = path.strip_prefix("/font/") {
        std::fs::read(font_path(file))
            .ok()
            .map(|bytes| ("font/ttf", bytes))
    } else {
        None
    };
    let response = Response::builder().header("Access-Control-Allow-Origin", "*");
    let built = match found {
        Some((kind, bytes)) => response
            .header("Content-Type", kind)
            .body(Cow::Owned(bytes)),
        None => response.status(404).body(Cow::Borrowed(&[][..])),
    };
    built.unwrap_or_else(|_| Response::new(Cow::Borrowed(&[][..])))
}

/// The page's head: its CSS, Roboto from the files every desktop app loads,
/// and the tier's scale.
fn head() -> String {
    let font = |weight: u32, file: &str| {
        format!(
            "@font-face {{ font-family: PerfRoboto; font-weight: {weight}; \
             src: url(\"{ASSETS}://localhost/font/{file}\"); }}"
        )
    };
    format!(
        "<style>{STYLE}\n{}\n{}\n#main {{ display: contents; }}\n:root {{ --s: {}; }}</style>",
        font(400, "Roboto-Regular.ttf"),
        font(700, "Roboto-Bold.ttf"),
        scale()
    )
}

/// Moves the anchor past the rows that scrolled off above on `frame`.
fn advance(frame: u32) {
    let gap = 8.0 * desktop_gauntlet().tier.scale;
    let first = *ANCHOR.peek();
    let anchor = {
        let heights = HEIGHTS.peek();
        first.advanced(frame as f32 * SCROLL_PER_FRAME, gap, |row| {
            heights.get(&row).copied()
        })
    };
    if anchor != first {
        *ANCHOR.write() = anchor;
        HEIGHTS
            .write()
            .retain(|measured, _| *measured >= anchor.row);
    }
}

#[component]
fn App() -> Element {
    use_future(|| async {
        let mut frames = document::eval(FRAMES);
        let freeze = desktop_gauntlet().launch.freeze;
        loop {
            if frames.recv::<u32>().await.is_err() {
                return;
            }
            let frame = *FRAME.peek() + 1;
            if frame == 1 {
                log::info!("PERF first_frame");
            }
            advance(frame);
            *FRAME.write() = frame;
            if freeze > 0 && frame >= freeze {
                log::info!("PERF frozen frame={frame}");
                return;
            }
            if frames.send(0).is_err() {
                return;
            }
        }
    });
    let palette = &colors().palette;
    rsx! {
        // Sparkline fills: one vertical gradient per palette color.
        svg { class: "gradients",
            defs {
                for (index , color) in palette.iter().enumerate() {
                    linearGradient {
                        key: "{index}",
                        id: "spark-{index}",
                        gradient_units: "userSpaceOnUse",
                        x1: "0",
                        y1: "0",
                        x2: "0",
                        y2: "1",
                        stop { offset: "0", stop_color: "{color}", stop_opacity: "0.25" }
                        stop { offset: "1", stop_color: "{color}", stop_opacity: "0" }
                    }
                }
            }
        }
        header { class: "top-bar", "Gauntlet" }
        Content {}
    }
}

/// The ticker strip and the list, in a column whose width follows the frame.
#[component]
fn Content() -> Element {
    let scale = use_hook(|| dioxus::desktop::window().scale_factor());
    let (window_width, _) = DESKTOP_WINDOW;
    let width =
        (f64::from(window_width) * scale * f64::from(width_fraction(FRAME()))).round() / scale;
    rsx! {
        main { class: "content", style: "width: {width}px",
            Panel {}
            // The panels stack over the list and show only over it, as every
            // app clips them.
            div { class: "stack",
                List {}
                for layer in 0..desktop_gauntlet().tier.layers {
                    StackedLayer { key: "{layer}", layer }
                }
            }
        }
    }
}

#[component]
fn Panel() -> Element {
    rsx! {
        section { class: "panel",
            for index in 0..desktop_gauntlet().tickers.len() {
                TickerTile { key: "{index}", index }
            }
        }
    }
}

/// One quote: symbol, price, change and a bar, all from the frame.
#[component]
fn TickerTile(index: usize) -> Element {
    let (data, colors) = (desktop_gauntlet(), colors());
    let ticker = &data.tickers[index];
    let cents = ticker_cents(ticker, FRAME());
    let share = (f64::from(cents - ticker.base_cents + ticker.swing_cents)
        / f64::from(2 * ticker.swing_cents))
    .clamp(0.0, 1.0)
        * 100.0;
    rsx! {
        div { class: "tile",
            span { class: "symbol", "{ticker.symbol}" }
            span { {cents_text(cents)} }
            span { class: if cents >= ticker.base_cents { "up" } else { "down" }, {change_text(ticker, cents)} }
            div { class: "track",
                div {
                    class: "fill",
                    style: "width: {share}%; background: {colors.palette[index % 8]}",
                }
            }
        }
    }
}

/// The rows from the anchor down to the bottom of the window, moved up by
/// the frame's offset.
#[component]
fn List() -> Element {
    let s = desktop_gauntlet().tier.scale;
    let gap = 8.0 * s;
    let offset = FRAME() as f32 * SCROLL_PER_FRAME;
    let anchor = ANCHOR();
    let (_, window_height) = DESKTOP_WINDOW;
    let rows = {
        let heights = HEIGHTS.peek();
        anchor.rows(offset, window_height as f32, gap, 10.0 * s, |row| {
            heights.get(&row).copied()
        })
    };
    let shift = anchor.top - gap - offset;
    rsx! {
        section { class: "list",
            div {
                class: "list-inner",
                style: "transform: translateY({shift}px)",
                for row in rows {
                    Row { key: "{row}", row }
                }
            }
        }
    }
}

/// A list row: cards side by side, or a cluster. It reports its height.
#[component]
fn Row(row: usize) -> Element {
    let columns = desktop_gauntlet().tier.columns;
    rsx! {
        div {
            class: "row",
            onresize: move |event| {
                if let Ok(size) = event.get_border_box_size() {
                    HEIGHTS.write().insert(row, size.height as f32);
                }
            },
            match gauntlet_row(row, columns) {
                GauntletRow::Cards(first) => rsx! {
                    for card in first..first + columns {
                        Card { key: "{card}", card }
                    }
                },
                GauntletRow::Cluster(cluster) => rsx! {
                    Level { cluster, remaining: desktop_gauntlet().tier.depth }
                },
            }
        }
    }
}

#[component]
fn Card(card: usize) -> Element {
    let (data, colors) = (desktop_gauntlet(), colors());
    let post = &data.posts[card % POST_COUNT];
    let (tag, tag_color) = &post.tags[0];
    rsx! {
        div { class: "card",
            div { class: "header",
                img {
                    class: "avatar",
                    src: "{ASSETS}://localhost/avatar/{card % AVATAR_COUNT}",
                }
                div { class: "head-text",
                    div { class: "paragraph title", "{post.title}" }
                    // `by Ada Lovelace · #rust`: the author bold, the tag in its color.
                    div { class: "subtitle",
                        "by "
                        b { "{post.author}" }
                        " · "
                        span { style: "color: {colors.gradient_end[*tag_color]}", "{tag}" }
                    }
                }
            }
            div { class: "paragraph body", "{post.body}" }
            Progress { card }
            Sparkline { card, color: post.color }
            div { class: "chips",
                for (text , color) in post.tags.iter() {
                    span {
                        class: "chip",
                        style: "background: {colors.chip_background[*color]}; color: {colors.gradient_end[*color]}",
                        "{text}"
                    }
                }
            }
            // Three counters split by dividers as tall as the row.
            div { class: "footer",
                for (index , label) in FOOTER_LABELS.iter().enumerate() {
                    if index > 0 {
                        div { class: "divider" }
                    }
                    div { class: "counter",
                        span { class: "stat", "{post.stats[index]}" }
                        span { class: "label", "{label}" }
                    }
                }
            }
            if card.is_multiple_of(5) {
                Badge { card }
            }
        }
    }
}

#[component]
fn Progress(card: usize) -> Element {
    let permille = progress_permille(card, FRAME());
    let width = f64::from(permille) / 10.0;
    rsx! {
        div { class: "progress",
            div { class: "track",
                div {
                    class: "fill",
                    style: "width: {width}%; background: {colors().palette[card % 8]}",
                }
            }
            span { class: "percent", "{permille / 10}%" }
        }
    }
}

/// A card's 48-point line over a fading fill, in its 0..47 by 0..1 box,
/// which CSS stretches to the card.
#[component]
fn Sparkline(card: usize, color: usize) -> Element {
    let frame = FRAME();
    let mut line = String::with_capacity(GAUNTLET_SPARK_POINTS * 12);
    for index in 0..GAUNTLET_SPARK_POINTS {
        let command = if index == 0 { 'M' } else { 'L' };
        let y = 1.0 - spark_value(card, index, frame);
        let _ = write!(line, "{command}{index} {y:.4}");
    }
    let area = format!("M0 1L{}L{} 1Z", &line[1..], GAUNTLET_SPARK_POINTS - 1);
    rsx! {
        svg {
            class: "sparkline",
            view_box: "0 0 {GAUNTLET_SPARK_POINTS - 1} 1",
            preserve_aspect_ratio: "none",
            path { fill: "url(#spark-{color})", d: "{area}" }
            path { class: "line", stroke: "{colors().palette[color]}", d: "{line}" }
        }
    }
}

/// A translucent panel stacked over the list. Its content never changes:
/// only the two components that carry its transforms read the frame.
#[component]
fn StackedLayer(layer: usize) -> Element {
    let (rows, colors) = (desktop_gauntlet().tier.layer_rows, colors());
    rsx! {
        LayerTilt { layer,
            div { class: "layer-title", "Layer {layer + 1}" }
            div { class: "layer-rows",
                for row in 0..rows {
                    div { key: "{row}", class: "layer-row",
                        for cell in row * LAYER_COLUMNS..(row + 1) * LAYER_COLUMNS {
                            div {
                                key: "{cell}",
                                class: "layer-cell",
                                style: "background: {colors.palette[layer_cell_color(layer, cell)]}",
                                "{cell + 1}"
                            }
                        }
                    }
                }
            }
            NestedTilt { layer,
                div { class: "nested-title", "Nested in layer {layer + 1}" }
                div { class: "nested-text", "Tilts against its panel" }
            }
        }
    }
}

/// The panel moved and tilted on this frame.
#[component]
fn LayerTilt(layer: usize, children: Element) -> Element {
    let frame = FRAME();
    let (x, y) = (layer_x(layer, frame), layer_y(layer, frame));
    let degrees = layer_degrees(layer, frame);
    rsx! {
        div { class: "layer", style: "transform: translate({x}px, {y}px) rotate({degrees}deg)", {children} }
    }
}

/// The nested card tilted against its panel on this frame.
#[component]
fn NestedTilt(layer: usize, children: Element) -> Element {
    let degrees = -layer_degrees(layer, FRAME());
    rsx! {
        div { class: "nested", style: "transform: rotate({degrees}deg)", {children} }
    }
}

/// A translucent tag tilting with the frame: transformed, never laid out
/// again.
#[component]
fn Badge(card: usize) -> Element {
    let degrees = badge_degrees(card, FRAME());
    rsx! {
        div { class: "badge", style: "transform: rotate({degrees}deg)", "HOT" }
    }
}

/// Levels nested inside one another, each a row of three labels above the
/// next level.
#[component]
fn Level(cluster: usize, remaining: usize) -> Element {
    let palette = &colors().palette;
    rsx! {
        div {
            class: "level",
            style: "background: {colors().level_background[remaining % 2]}",
            div { class: "level-row",
                for chip in 0..3 {
                    div {
                        class: "level-chip",
                        style: "background: {palette[(remaining + chip + cluster) % 8]}",
                        "C{cluster}.L{remaining}.{chip}"
                    }
                }
            }
            if remaining > 0 {
                Level { cluster, remaining: remaining - 1 }
            }
        }
    }
}

fn main() {
    perf_data::log_to_stdout();
    let (width, height) = DESKTOP_WINDOW;
    let window = WindowBuilder::new()
        .with_title("Gauntlet")
        .with_inner_size(LogicalSize::new(f64::from(width), f64::from(height)))
        .with_resizable(false);
    let config = Config::new()
        .with_window(window)
        .with_menu(None)
        .with_disable_context_menu(true)
        .with_custom_head(head())
        .with_custom_protocol(ASSETS, |_, request| serve(&request));
    dioxus::LaunchBuilder::desktop()
        .with_cfg(config)
        .launch(App);
}
