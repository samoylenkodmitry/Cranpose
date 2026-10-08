//! Cranpose half of the Cranpose versus Jetpack Compose comparison.
//!
//! Every screen mirrors `compose-app` element for element; see `../README.md`.
#![deny(unsafe_code)]

mod data;
mod screens;
#[cfg(all(feature = "web", target_arch = "wasm32"))]
mod web;

use std::cell::Cell;

use cranpose::prelude::*;
pub use screens::{
    gauntlet::{GauntletLoad, GauntletScreen},
    workspace::{FRAME_OBSERVER, WorkspaceFrame, WorkspaceMode},
};

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
    Workspace,
    Gauntlet,
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
            "workspace" => Self::Workspace,
            "gauntlet" => Self::Gauntlet,
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
            Self::Workspace => "Cranpose · trading workspace",
            // Both apps title it alike: the two pictures are compared.
            Self::Gauntlet => "Gauntlet",
        }
    }
}

/// The Roboto faces every app loads: on Android the device's own, the files
/// the Compose app loads, so both frameworks shape and measure identical
/// fonts; on a desktop the ones in the folder `PERF_FONTS` names, which
/// every desktop app loads. Read once and kept for the life of the process.
#[cfg(any(
    target_os = "android",
    feature = "desktop",
    all(feature = "web", target_arch = "wasm32")
))]
fn roboto_faces() -> &'static [&'static [u8]] {
    let faces: Vec<&'static [u8]> = ["Roboto-Regular.ttf", "Roboto-Medium.ttf", "Roboto-Bold.ttf"]
        .iter()
        .filter_map(|file| match perf_data::font_bytes(file) {
            Ok(bytes) => Some(&*Box::leak(bytes.into_boxed_slice())),
            Err(error) => {
                log::warn!("font {file} unavailable: {error}");
                None
            }
        })
        .collect();
    Box::leak(faces.into_boxed_slice())
}

/// On Android the app draws in the device's Roboto, so the APK carries no
/// font of its own, as the Compose APK does not.
#[cfg(target_os = "android")]
pub fn create_app() -> AppLauncher<AppFonts> {
    launcher().with_fonts(roboto_faces())
}

/// Runs the app in a desktop window, in the Roboto files every desktop app
/// loads, or without them in the embedded face.
#[cfg(all(not(target_os = "android"), feature = "desktop"))]
pub fn run_desktop() -> Result<(), cranpose::LaunchError> {
    let faces = roboto_faces();
    if faces.is_empty() {
        launcher().try_run(PerfCompareApp)
    } else {
        launcher().with_fonts(faces).try_run(PerfCompareApp)
    }
}

/// A phone-sized window, or for the trading workspace the 1280 × 820
/// window gpui-fast's showcase opens, the size every desktop app gives the
/// gauntlet too.
#[cfg(any(target_os = "android", feature = "desktop"))]
fn launcher() -> AppLauncher {
    let (width, height) = match launch_args().string("scenario") {
        Some("workspace" | "gauntlet") => perf_data::DESKTOP_WINDOW,
        _ => (360, 748),
    };
    AppLauncher::new()
        .with_title("Perf Compare")
        .with_size(width, height)
        .with_log_tag("PerfCompare")
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
    workspace: screens::workspace::WorkspaceMode,
    gauntlet: screens::gauntlet::GauntletLoad,
    still: bool,
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
            workspace: screens::workspace::WorkspaceMode::from_name(
                args.string("mode").unwrap_or("quotes"),
            ),
            gauntlet: screens::gauntlet::GauntletLoad {
                tier: count("tier", 5),
                freeze: u32::try_from(count("freeze", 0)).unwrap_or(u32::MAX),
            },
            still: args.boolean("still").unwrap_or(false),
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
            // The workspace brings the showcase's own toolbar.
            if launch.scenario != Scenario::Workspace {
                screens::TopBar(launch.scenario.title());
            }
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
                Scenario::Workspace => {
                    screens::workspace::WorkspaceFrame(launch.workspace, launch.still)
                }
                Scenario::Gauntlet => screens::gauntlet::GauntletScreen(launch.gauntlet),
            }
        },
    );
}

cranpose::android_main! {
    launcher: create_app(),
    content: PerfCompareApp,
}
