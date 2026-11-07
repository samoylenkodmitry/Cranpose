#![deny(unsafe_code)]

mod board;
#[cfg(any(
    feature = "changing-paths",
    all(feature = "screens-in-turn", not(feature = "e32r35t"))
))]
#[path = "../../../crates/cranpose/tests/support/changing_paths.rs"]
mod changing_paths;
#[cfg(feature = "dashboard")]
#[path = "../../../crates/cranpose/tests/support/dashboard.rs"]
mod dashboard;
#[cfg(any(
    feature = "deep-moves",
    all(feature = "screens-in-turn", not(feature = "e32r35t"))
))]
#[path = "../../../crates/cranpose/tests/support/deep_moves.rs"]
mod deep_moves;
#[cfg(any(
    not(any(
        feature = "dashboard",
        feature = "changing-paths",
        feature = "deep-moves"
    )),
    feature = "screens-in-turn"
))]
mod demo;
#[cfg(any(
    feature = "dashboard",
    feature = "changing-paths",
    feature = "deep-moves",
    all(feature = "screens-in-turn", not(feature = "e32r35t"))
))]
#[path = "../../../crates/cranpose/tests/support/frame_time.rs"]
mod frame_time;
#[cfg(feature = "screens-in-turn")]
mod in_turn;
#[cfg(all(feature = "screens-in-turn", not(feature = "e32r35t")))]
#[path = "../../../crates/cranpose/tests/support/still_wall.rs"]
mod still_wall;
#[cfg(any(
    feature = "dashboard",
    feature = "changing-paths",
    feature = "deep-moves",
    all(feature = "screens-in-turn", not(feature = "e32r35t"))
))]
#[path = "../../../crates/cranpose/tests/support/text_style.rs"]
mod text_style;

use cranpose::prelude::*;
use cranpose_platform_esp_idf::{DisplayConfig, RunError, run};

static FONTS: &[&[u8]] = &[include_bytes!("../assets/NotoSans-Latin.ttf")];

#[cfg(all(
    any(
        feature = "dashboard",
        feature = "changing-paths",
        feature = "deep-moves"
    ),
    any(
        feature = "text-drawn",
        feature = "text-composed",
        feature = "workload"
    )
))]
compile_error!(
    "dashboard, changing-paths and deep-moves replace the demo's screen, which the other benchmark features change"
);

#[cfg(all(
    any(
        all(feature = "dashboard", feature = "changing-paths"),
        all(feature = "dashboard", feature = "deep-moves"),
        all(feature = "changing-paths", feature = "deep-moves")
    ),
    not(feature = "screens-in-turn")
))]
compile_error!("dashboard, changing-paths and deep-moves each replace the demo's screen: pick one");

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

// The screen: the demo's own, with the `dashboard`, `changing-paths` or
// `deep-moves` feature that screen of the benchmarks, or with
// `screens-in-turn` each of them in turn.
#[composable]
fn Screen() {
    #[cfg(feature = "screens-in-turn")]
    in_turn::Screens();
    #[cfg(all(feature = "dashboard", not(feature = "screens-in-turn")))]
    dashboard::Dashboard(board::TITLE);
    #[cfg(all(feature = "changing-paths", not(feature = "screens-in-turn")))]
    changing_paths::ChangingPaths(board::TITLE);
    #[cfg(all(feature = "deep-moves", not(feature = "screens-in-turn")))]
    deep_moves::DeepMoves(board::TITLE);
    #[cfg(not(any(
        feature = "dashboard",
        feature = "changing-paths",
        feature = "deep-moves",
        feature = "screens-in-turn"
    )))]
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
