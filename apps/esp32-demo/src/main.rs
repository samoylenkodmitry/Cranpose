#![deny(unsafe_code)]

mod board;
#[cfg(feature = "dashboard")]
#[path = "../../../crates/cranpose/tests/support/dashboard.rs"]
mod dashboard;
#[cfg(any(not(feature = "dashboard"), feature = "screens-in-turn"))]
mod demo;
#[cfg(feature = "screens-in-turn")]
mod in_turn;

use cranpose::prelude::*;
use cranpose_platform_esp_idf::{DisplayConfig, RunError, run};

static FONTS: &[&[u8]] = &[include_bytes!("../assets/NotoSans-Latin.ttf")];

#[cfg(all(
    feature = "dashboard",
    any(
        feature = "text-drawn",
        feature = "text-composed",
        feature = "workload"
    )
))]
compile_error!("dashboard replaces the demo's screen, which the other benchmark features change");

#[cfg(all(
    feature = "screens-in-turn",
    any(
        feature = "text-drawn",
        feature = "text-composed",
        feature = "workload"
    )
))]
compile_error!(
    "screens-in-turn shows every line in turn: drop text-drawn, text-composed and workload"
);

// The screen: the demo's own, with the `dashboard` feature the dashboard of
// the benchmarks, or with `screens-in-turn` each of them in turn.
#[composable]
fn Screen() {
    #[cfg(feature = "screens-in-turn")]
    in_turn::Screens();
    #[cfg(all(feature = "dashboard", not(feature = "screens-in-turn")))]
    dashboard::Dashboard(board::TITLE);
    #[cfg(not(any(feature = "dashboard", feature = "screens-in-turn")))]
    demo::Screen(demo::LINE);
}

fn main() {
    esp_idf_svc::sys::link_patches();
    esp_idf_svc::log::EspLogger::initialize_default();
    let config = DisplayConfig {
        fonts: FONTS,
        #[cfg(feature = "screens-in-turn")]
        stats_window: in_turn::STATS_WINDOW,
        ..board::config()
    };
    let Err(error) = board::open()
        .map_err(RunError::from)
        .and_then(|(panel, touch)| run(panel, touch, config, Screen));
    log::error!("esp32-demo stopped: {error}");
}
