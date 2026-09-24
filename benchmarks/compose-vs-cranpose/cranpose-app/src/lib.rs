//! Cranpose half of the Cranpose versus Jetpack Compose comparison.
//!
//! Every screen mirrors `compose-app` element for element; see `../README.md`.
#![deny(unsafe_code)]

mod data;
mod screens;

use std::cell::Cell;

use cranpose::prelude::*;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Scenario {
    Feed,
    Ticker,
    Particles,
    Layers,
    Grid,
    GridLayer,
    Deep,
    DeepLayer,
}

impl Scenario {
    fn from_name(name: &str) -> Self {
        match name {
            "ticker" => Self::Ticker,
            "particles" => Self::Particles,
            "layers" => Self::Layers,
            "grid" => Self::Grid,
            "grid_layer" => Self::GridLayer,
            "deep" => Self::Deep,
            "deep_layer" => Self::DeepLayer,
            _ => Self::Feed,
        }
    }

    fn title(self) -> &'static str {
        match self {
            Self::Feed => "Cranpose · feed",
            Self::Ticker => "Cranpose · ticker",
            Self::Particles => "Cranpose · particles",
            Self::Layers => "Cranpose · layers",
            Self::Grid => "Cranpose · grid",
            Self::GridLayer => "Cranpose · grid + layers",
            Self::Deep => "Cranpose · deep",
            Self::DeepLayer => "Cranpose · deep + layers",
        }
    }
}

/// The device's Roboto faces, read from the same two files the Compose app
/// loads, so both frameworks shape and measure identical fonts. Read once and
/// kept for the life of the process; off Android the list is empty and the
/// embedded default face is used.
fn system_roboto() -> &'static [&'static [u8]] {
    let faces: Vec<&'static [u8]> = [
        "/system/fonts/Roboto-Regular.ttf",
        "/system/fonts/Roboto-Bold.ttf",
    ]
    .iter()
    .filter_map(|path| match std::fs::read(path) {
        Ok(bytes) => Some(&*Box::leak(bytes.into_boxed_slice())),
        Err(error) => {
            log::warn!("font {path} unavailable: {error}");
            None
        }
    })
    .collect();
    Box::leak(faces.into_boxed_slice())
}

pub fn create_app() -> AppLauncher {
    AppLauncher::new()
        .with_title("Perf Compare")
        .with_size(360, 748)
        .with_log_tag("PerfCompare")
        .with_fonts(system_roboto())
}

/// What `am start` asked for: the scenario and its knobs.
#[derive(Clone, Copy, PartialEq)]
struct Launch {
    scenario: Scenario,
    feed: screens::feed::FeedLoad,
    quotes: usize,
    particles: usize,
    opaque: bool,
    tile_columns: usize,
    tile_rows: usize,
    rows: usize,
    columns: usize,
    depth: usize,
    chips: usize,
}

impl Launch {
    fn from_args() -> Self {
        let args = launch_args();
        let count = |name: &str, default: usize| {
            args.int(name)
                .and_then(|value| usize::try_from(value).ok())
                .unwrap_or(default)
        };
        Self {
            scenario: Scenario::from_name(args.string("scenario").unwrap_or("feed")),
            feed: screens::feed::FeedLoad {
                speed: if args.boolean("still").unwrap_or(false) {
                    0.0
                } else {
                    args.float("speed").unwrap_or(2000.0)
                },
                columns: count("fcols", 1).max(1),
                comments: count("comments", 0),
                scale: args.float("ui").unwrap_or(1.0),
            },
            quotes: count("quotes", 24),
            particles: count("particles", 2000),
            opaque: args.boolean("opaque").unwrap_or(false),
            tile_columns: count("tcols", 6),
            tile_rows: count("trows", 11),
            rows: count("rows", 30),
            columns: count("cols", 12),
            depth: count("depth", 40),
            chips: count("chips", 6),
        }
    }
}

thread_local! {
    static FIRST_FRAME_LOGGED: Cell<bool> = const { Cell::new(false) };
}

#[composable]
pub fn PerfCompareApp() {
    let launch = remember(Launch::from_args).with(|launch| *launch);
    Column(
        Modifier::empty()
            .fill_max_size()
            .background(Color::from_rgb_u8(0xEE, 0xF0, 0xF5))
            .draw_behind(|_| {
                if !FIRST_FRAME_LOGGED.with(|logged| logged.replace(true)) {
                    log::info!("PERF first_frame");
                }
            }),
        ColumnSpec::default(),
        move || {
            screens::TopBar(launch.scenario.title());
            match launch.scenario {
                Scenario::Feed => screens::feed::FeedScreen(launch.feed),
                Scenario::Ticker => screens::ticker::TickerScreen(launch.quotes),
                Scenario::Particles => screens::particles::ParticlesScreen(launch.particles),
                Scenario::Layers => screens::layers::LayersScreen(
                    launch.opaque,
                    launch.tile_columns,
                    launch.tile_rows,
                ),
                Scenario::Grid | Scenario::GridLayer => screens::heavy::GridScreen(
                    launch.scenario == Scenario::GridLayer,
                    launch.rows,
                    launch.columns,
                ),
                Scenario::Deep | Scenario::DeepLayer => screens::heavy::DeepScreen(
                    launch.scenario == Scenario::DeepLayer,
                    launch.depth,
                    launch.chips,
                ),
            }
        },
    );
}

cranpose::android_main! {
    launcher: create_app(),
    content: PerfCompareApp,
}
