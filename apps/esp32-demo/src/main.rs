#![deny(unsafe_code)]

mod board;
#[cfg(feature = "dashboard")]
#[path = "../../../crates/cranpose/tests/support/dashboard.rs"]
mod dashboard;
#[cfg(not(feature = "dashboard"))]
mod demo;

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

// The screen: the demo's own, or with the `dashboard` feature the
// dashboard of the benchmarks.
#[composable]
fn Screen() {
    #[cfg(feature = "dashboard")]
    dashboard::Dashboard(board::TITLE);
    #[cfg(not(feature = "dashboard"))]
    demo::Screen();
}

fn main() {
    esp_idf_svc::sys::link_patches();
    esp_idf_svc::log::EspLogger::initialize_default();
    let config = DisplayConfig {
        fonts: FONTS,
        ..board::config()
    };
    let Err(error) = board::open()
        .map_err(RunError::from)
        .and_then(|(panel, touch)| run(panel, touch, config, Screen));
    log::error!("esp32-demo stopped: {error}");
}
