//! The gauntlet in Slint: `ui/gauntlet.slint` draws it; this side hands it
//! the deterministic data, the sparkline paths and the frame clock. The
//! benchmark's README describes it. On a desktop the patchwork, drawn by
//! `ui/patchwork.slint`, is a third screen.

use std::{fmt::Write, rc::Rc};

use perf_data::{
    AVATAR_COUNT, AVATAR_SIZE, CARD_ROWS_PER_CLUSTER, CHIP_BACKGROUND_RGB, FrameIndex,
    GAUNTLET_SPARK_POINTS, GRADIENT_END_RGB, GauntletRow, Launch, PALETTE_RGB, POST_COUNT, Post,
    Scenario, WALL_CLOCK_HEIGHT, avatar_rgba, gauntlet_row, gauntlet_tier, posts, spark_value,
    tickers, wall_clock, wall_dot_rgb, wall_dot_size, wall_fill_rgb, wall_grid, wall_label,
    wall_padding, wall_text_size, wall_tier, wall_value,
};
use slint::{
    Color, ComponentHandle, Image, Model, ModelRc, ModelTracker, PlatformError, RenderingState,
    Rgba8Pixel, SharedPixelBuffer, SharedString, VecModel,
};

slint::include_modules!();

#[cfg(not(any(target_os = "android", target_arch = "wasm32")))]
mod chat;
#[cfg(not(any(target_os = "android", target_arch = "wasm32")))]
mod patchwork;

/// Blocks of five card rows and a cluster: no measurement window reaches the end.
const ROWS: usize = 2000 * (CARD_ROWS_PER_CLUSTER + 1);
/// How far the list scrolls each frame, in logical pixels (dp).
const SCROLL_PER_FRAME: f32 = 3.0;

fn color(rgb: [u8; 3]) -> Color {
    Color::from_rgb_u8(rgb[0], rgb[1], rgb[2])
}

/// Tile `index` of the still wall.
fn wall_tile(index: usize) -> WallTile {
    WallTile {
        label: wall_label(index).into(),
        value: wall_value(index).into(),
        fill: color(wall_fill_rgb(index)),
        dot: color(wall_dot_rgb(index)),
    }
}

/// The list's rows, made as the list shows them.
struct Rows {
    columns: usize,
    posts: Rc<Vec<Post>>,
    avatars: Rc<Vec<Image>>,
}

impl Rows {
    fn card(&self, card: usize) -> CardData {
        let post = &self.posts[card % POST_COUNT];
        let (tag, tag_color) = &post.tags[0];
        let chips: Vec<ChipData> = post
            .tags
            .iter()
            .map(|(text, color_index)| ChipData {
                text: text.as_str().into(),
                background: color(CHIP_BACKGROUND_RGB[*color_index]),
                color: color(GRADIENT_END_RGB[*color_index]),
            })
            .collect();
        let stats: Vec<SharedString> = post.stats.iter().map(|stat| stat.as_str().into()).collect();
        CardData {
            card: card as i32,
            avatar: self.avatars[card % AVATAR_COUNT].clone(),
            title: post.title.as_str().into(),
            author: post.author.as_str().into(),
            tag: tag.as_str().into(),
            tag_color: color(GRADIENT_END_RGB[*tag_color]),
            body: post.body.as_str().into(),
            line: color(PALETTE_RGB[post.color]),
            bar: color(PALETTE_RGB[card % PALETTE_RGB.len()]),
            chips: ModelRc::new(VecModel::from(chips)),
            stats: ModelRc::new(VecModel::from(stats)),
        }
    }
}

impl Model for Rows {
    type Data = RowData;

    fn row_count(&self) -> usize {
        ROWS
    }

    fn row_data(&self, row: usize) -> Option<RowData> {
        if row >= ROWS {
            return None;
        }
        Some(match gauntlet_row(row, self.columns) {
            GauntletRow::Cluster(cluster) => RowData {
                cluster: cluster as i32,
                cards: ModelRc::default(),
            },
            GauntletRow::Cards(first) => {
                let cards: Vec<CardData> = (first..first + self.columns)
                    .map(|card| self.card(card))
                    .collect();
                RowData {
                    cluster: -1,
                    cards: ModelRc::new(VecModel::from(cards)),
                }
            }
        })
    }

    fn model_tracker(&self) -> &dyn ModelTracker {
        // The rows never change.
        &()
    }
}

/// A card's 48-point sparkline on the frame as path commands in a `width` by
/// `height` box: the line, or with `area` the area under it.
fn spark_path(card: i32, frame: i32, width: f32, height: f32, area: bool) -> SharedString {
    let mut path = String::with_capacity(GAUNTLET_SPARK_POINTS * 16 + 32);
    let step = width / (GAUNTLET_SPARK_POINTS - 1) as f32;
    if area {
        let _ = write!(path, "M 0 {height}");
    }
    for index in 0..GAUNTLET_SPARK_POINTS {
        let x = index as f32 * step;
        let y = height * (1.0 - spark_value(card as usize, index, frame as u32));
        let command = if index == 0 && !area { 'M' } else { 'L' };
        let _ = write!(path, " {command} {x:.2} {y:.2}");
    }
    if area {
        let _ = write!(path, " L {width} {height} Z");
    }
    path.into()
}

/// Shows the gauntlet, in a window of `size` where the platform has
/// windows, and advances its frame after every frame drawn.
pub fn run(launch: Launch, size: Option<slint::LogicalSize>) -> Result<(), PlatformError> {
    if launch.scenario == Scenario::Wall {
        return run_wall(launch, size);
    }
    #[cfg(not(any(target_os = "android", target_arch = "wasm32")))]
    if let (Scenario::Patchwork, Some(size)) = (launch.scenario, size) {
        return patchwork::run(launch, size);
    }
    let tier = gauntlet_tier(launch.tier);
    let window = GauntletWindow::new()?;
    if let Some(size) = size {
        window.window().set_size(size);
    }
    let gauntlet = window.global::<Gauntlet>();
    gauntlet.set_s(tier.scale);
    gauntlet.set_depth(tier.depth as i32);
    gauntlet.set_layers(tier.layers as i32);
    gauntlet.set_layer_rows(tier.layer_rows as i32);
    gauntlet.set_palette(ModelRc::new(VecModel::from(
        PALETTE_RGB.map(color).to_vec(),
    )));
    gauntlet.on_spark_path(spark_path);
    let quotes: Vec<TickerData> = tickers(tier.tickers)
        .iter()
        .enumerate()
        .map(|(index, ticker)| TickerData {
            symbol: ticker.symbol.as_str().into(),
            base: ticker.base_cents,
            swing: ticker.swing_cents,
            step: ticker.step,
            phase: ticker.phase,
            color: color(PALETTE_RGB[index % PALETTE_RGB.len()]),
        })
        .collect();
    window.set_tickers(ModelRc::new(VecModel::from(quotes)));
    let avatars = (0..AVATAR_COUNT)
        .map(|index| {
            let size = AVATAR_SIZE;
            Image::from_rgba8(SharedPixelBuffer::<Rgba8Pixel>::clone_from_slice(
                &avatar_rgba(index),
                size,
                size,
            ))
        })
        .collect();
    window.set_rows(ModelRc::new(Rows {
        columns: tier.columns,
        posts: Rc::new(posts()),
        avatars: Rc::new(avatars),
    }));

    let weak = window.as_weak();
    let mut first_frame_logged = false;
    let mut frozen = false;
    window
        .window()
        .set_rendering_notifier(move |state, _| {
            if !matches!(state, RenderingState::AfterRendering) {
                return;
            }
            let Some(window) = weak.upgrade() else { return };
            if !first_frame_logged {
                first_frame_logged = true;
                log::info!("PERF first_frame");
            }
            let clock = window.global::<Clock>();
            let frame = clock.get_frame();
            if launch.freeze > 0 && frame >= launch.freeze as i32 {
                if !frozen {
                    frozen = true;
                    log::info!("PERF frozen frame={frame}");
                }
                return;
            }
            clock.set_frame(frame + 1);
            window.set_scroll(-((frame + 1) as f32) * SCROLL_PER_FRAME);
        })
        .map_err(|error| PlatformError::Other(format!("no rendering notifier: {error}")))?;
    window.run()
}

/// Shows the still wall: the tiles that fit the window as a model made once,
/// and after every frame drawn the next frame's clock.
fn run_wall(launch: Launch, size: Option<slint::LogicalSize>) -> Result<(), PlatformError> {
    let tier = wall_tier(launch.tier);
    let window = WallWindow::new()?;
    let size = size.unwrap_or_else(|| {
        window
            .window()
            .size()
            .to_logical(window.window().scale_factor())
    });
    window.window().set_size(size);
    let (columns, rows) = wall_grid(tier, size.width, size.height - 56.0 - WALL_CLOCK_HEIGHT);
    let tiles: Vec<WallTile> = (0..columns * rows).map(wall_tile).collect();
    let (label_size, value_size) = wall_text_size(tier.tile_height);
    window.set_tiles(ModelRc::new(VecModel::from(tiles)));
    window.set_columns(columns as i32);
    window.set_tile_width(tier.tile_width);
    window.set_tile_height(tier.tile_height);
    window.set_label_size(label_size);
    window.set_value_size(value_size);
    window.set_dot(wall_dot_size(tier.tile_height));
    window.set_pad(wall_padding(tier.tile_height));
    window.global::<WallClock>().set_text(wall_clock(0).into());
    let weak = window.as_weak();
    let mut frame = FrameIndex::new(launch.freeze);
    window
        .window()
        .set_rendering_notifier(move |state, _| {
            if !matches!(state, RenderingState::AfterRendering) {
                return;
            }
            let Some(window) = weak.upgrade() else { return };
            if frame.advance() {
                let clock = window.global::<WallClock>();
                clock.set_frame(frame.get() as i32);
                clock.set_text(wall_clock(frame.get()).into());
            }
        })
        .map_err(|error| PlatformError::Other(format!("no rendering notifier: {error}")))?;
    window.run()
}

/// Runs the gauntlet the launch activity asked for.
#[cfg(target_os = "android")]
#[unsafe(no_mangle)]
fn android_main(app: slint::android::AndroidApp) {
    android_logger::init_once(
        android_logger::Config::default()
            .with_max_level(log::LevelFilter::Info)
            .with_tag("PerfCompare"),
    );
    let launch = Launch::read(app.internal_data_path());
    if let Err(error) = slint::android::init(app) {
        log::error!("no Slint platform: {error}");
        return;
    }
    if let Err(error) = run(launch, None) {
        log::error!("Slint stopped: {error}");
    }
}

/// Runs the gauntlet in a desktop window, as `desktop.py` asks for it.
#[cfg(not(any(target_os = "android", target_arch = "wasm32")))]
pub fn run_desktop(launch: Launch) -> Result<(), PlatformError> {
    register_roboto();
    register_fonts(&["DejaVuSansMono.ttf"]);
    let (width, height) = perf_data::DESKTOP_WINDOW;
    run(
        launch,
        Some(slint::LogicalSize::new(width as f32, height as f32)),
    )
}

/// Draws the gauntlet in the page's `canvas`, the query string its launch.
#[cfg(target_arch = "wasm32")]
#[wasm_bindgen::prelude::wasm_bindgen(start)]
pub fn run_web() {
    wasm_logger::init(wasm_logger::Config::new(log::Level::Info));
    console_error_panic_hook::set_once();
    let page = web_sys::window();
    let query = page
        .as_ref()
        .and_then(|window| window.location().search().ok())
        .unwrap_or_default();
    // The canvas takes the root layout's preferred size, which is not the
    // page's: the window is the viewport.
    let viewport = page.as_ref().and_then(|window| {
        let side = |value: Result<wasm_bindgen::JsValue, _>| value.ok()?.as_f64();
        Some(slint::LogicalSize::new(
            side(window.inner_width())? as f32,
            side(window.inner_height())? as f32,
        ))
    });
    register_roboto();
    if let Err(error) = run(Launch::from_query(&query), viewport) {
        log::error!("Slint stopped: {error}");
    }
}

/// Roboto is no system font where the app runs but Android: the app registers
/// the files every desktop app loads, or the page's copy, which the markup's
/// `Roboto` family then finds.
#[cfg(not(target_os = "android"))]
fn register_roboto() {
    register_fonts(&["Roboto-Regular.ttf", "Roboto-Bold.ttf"]);
}

/// Registers the font files `files` name with Slint's font collection.
#[cfg(not(target_os = "android"))]
fn register_fonts(files: &[&str]) {
    use slint::fontique_011::fontique;
    let mut collection = slint::fontique_011::shared_collection();
    for file in files {
        match perf_data::font_bytes(file) {
            Ok(bytes) => {
                collection.register_fonts(fontique::Blob::new(std::sync::Arc::new(bytes)), None);
            }
            Err(error) => log::warn!("no {file}: {error}"),
        }
    }
}
