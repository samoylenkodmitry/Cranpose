//! The framework comparison's screens in Floem, as `desktop.py` runs them:
//! `PERF_SCENARIO=wall PERF_TIER=20 PERF_FONTS=../fonts cargo run --release`.
//!
//! Floem builds the view tree once. Signals feed the views that change: a
//! label reads the frame, a style closure reads it for a view's place or turn,
//! and a keyed `dyn_stack` or `virtual_stack` makes and drops views. The
//! tiles, charts and pictures, which Floem has no view for, are views of
//! their own that paint themselves from text shaped once.

mod chat;
mod frame;
mod gauntlet;
mod layer;
mod patchwork;
mod picture;
mod raster;
mod surface;
mod text;
mod tiles;
mod wall;

use floem::{Application, IntoView, kurbo::Size, peniko::Color, prelude::*, window::WindowConfig};
use perf_data::{DESKTOP_WINDOW, Launch, Scenario};
use text::ROBOTO;

/// A color of the shared data.
pub const fn rgb(value: [u8; 3]) -> Color {
    Color::rgb8(value[0], value[1], value[2])
}

pub const TOP_BAR: Color = rgb([0x1E, 0x2A, 0x4A]);

/// The bar across the window's top, titled `title`.
fn top_bar(title: &'static str) -> impl IntoView {
    container(text(title).style(|s| s.font_size(20.0).font_bold().color(Color::WHITE))).style(|s| {
        s.width_full()
            .height(56.0)
            .flex_shrink(0.0_f32)
            .padding_horiz(16.0)
            .items_center()
            .background(TOP_BAR)
    })
}

/// The launch the environment asks for, with `--scenario=`, `--tier=` and
/// `--freeze=` over it.
fn launch() -> Launch {
    let mut launch = Launch::from_env();
    for argument in std::env::args().skip(1) {
        let Some((name, value)) = argument.trim_start_matches("--").split_once('=') else {
            continue;
        };
        match name {
            "scenario" => launch.scenario = Scenario::from_name(value),
            "tier" => launch.tier = value.parse().unwrap_or(launch.tier),
            "freeze" => launch.freeze = value.parse().unwrap_or(launch.freeze),
            _ => {}
        }
    }
    launch
}

fn screen(launch: Launch) -> impl IntoView {
    let (frame, clock) = frame::frame_clock(launch.freeze);
    let content = match launch.scenario {
        Scenario::Gauntlet => gauntlet::screen(frame).into_any(),
        Scenario::Wall => wall::screen(launch, frame).into_any(),
        Scenario::Patchwork => patchwork::screen(launch, frame).into_any(),
    };
    // The text of every label is Roboto, 1.3 lines tall.
    stack((content, clock)).style(|s| {
        s.size_full()
            .font_family(ROBOTO.to_owned())
            .line_height(1.3)
    })
}

fn main() {
    perf_data::log_to_stdout();
    let launch = launch();
    perf_data::set_launch(launch);
    text::load_fonts(&[
        "Roboto-Regular.ttf",
        "Roboto-Bold.ttf",
        "DejaVuSansMono.ttf",
    ]);
    let (width, height) = DESKTOP_WINDOW;
    let config = WindowConfig::default()
        .size(Size::new(f64::from(width), f64::from(height)))
        .resizable(false)
        .apply_default_theme(false)
        .title(launch.scenario.title());
    Application::new()
        .window(move |_| screen(launch), Some(config))
        .run();
}
