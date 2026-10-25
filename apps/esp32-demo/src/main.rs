#![deny(unsafe_code)]

mod board;

#[cfg(not(any(
    feature = "text-drawn",
    feature = "text-composed",
    feature = "workload"
)))]
use std::time::Duration;

use cranpose::{prelude::*, text::TextUnit};
use cranpose_animation::{
    AnimationSpec, RepeatMode, StartOffset, infiniteRepeatable, rememberInfiniteTransition,
};
use cranpose_platform_esp_idf::{DisplayConfig, RunError, run};

static FONTS: &[&[u8]] = &[include_bytes!("../assets/NotoSans-Latin.ttf")];

fn text_style(color: Color, size: f32) -> TextStyle {
    TextStyle {
        span_style: SpanStyle {
            color: Some(color),
            font_size: TextUnit::Sp(size),
            ..SpanStyle::default()
        },
        ..TextStyle::default()
    }
}

#[composable]
fn Pulse() {
    let transition = rememberInfiniteTransition("pulse");
    let spec = infiniteRepeatable(
        AnimationSpec::linear(1500),
        RepeatMode::Reverse,
        StartOffset::default(),
    );
    let progress = transition.animateFloat(0.1, 1.0, spec, "pulse_width");
    // The bar reads the animated width while it draws: each frame draws it
    // again without composing or laying out the screen.
    Box(
        Modifier::empty()
            .fill_max_width()
            .height(16.0)
            .background(Color::from_rgb_u8(40, 44, 64))
            .rounded_corners(8.0)
            .draw_behind(move |scope| {
                let size = scope.size();
                scope.draw_round_rect_at(
                    Rect {
                        x: 0.0,
                        y: 0.0,
                        width: size.width * progress.value(),
                        height: size.height,
                    },
                    Brush::solid(Color::from_rgb_u8(98, 160, 255)),
                    CornerRadii::uniform(8.0),
                );
            }),
        BoxSpec::default(),
        || {},
    );
}

/// The line under the bar: the seconds since the start. The benchmarks'
/// `text-drawn` and `text-composed` features show a number that counts up
/// every frame there instead, as a clock or a sensor reading does, and the
/// `workload` feature the work of a busier screen.
#[cfg(not(any(
    feature = "text-drawn",
    feature = "text-composed",
    feature = "workload"
)))]
#[composable]
fn StatusLine() {
    let seconds = cranpose_core::rememberMutableStateOf(|| 0u32);
    cranpose_core::LaunchedEffectAsync((), move |_| {
        std::boxed::Box::pin(async move {
            loop {
                cranpose_core::delay(Duration::from_secs(1)).await;
                seconds.set(seconds.get() + 1);
            }
        })
    });
    Text(
        format!("Running for {} s", seconds.get()),
        Modifier::empty(),
        text_style(Color::from_rgb_u8(200, 204, 220), 14.0),
    );
}

#[cfg(any(
    all(feature = "text-drawn", feature = "text-composed"),
    all(feature = "text-drawn", feature = "workload"),
    all(feature = "text-composed", feature = "workload"),
))]
compile_error!("text-drawn, text-composed and workload each replace the seconds line: pick one");

#[cfg(any(feature = "text-drawn", feature = "text-composed"))]
#[path = "../../../crates/cranpose/tests/support/reading.rs"]
mod reading;

/// The number in a `Text`, composed again each frame.
#[cfg(feature = "text-composed")]
#[composable]
fn StatusLine() {
    reading::ComposedReading(text_style(Color::from_rgb_u8(200, 204, 220), 14.0));
}

/// The number drawn by a draw scope that reads it: frames draw it again
/// without composing or laying out.
#[cfg(feature = "text-drawn")]
#[composable]
fn StatusLine() {
    reading::DrawnReading(Color::from_rgb_u8(200, 204, 220), 14.0);
}

#[cfg(feature = "workload")]
#[path = "../../../crates/cranpose/tests/support/workload.rs"]
mod workload;

/// The work of a busier screen, for the profiles of the IRAM lists.
#[cfg(feature = "workload")]
#[composable]
fn StatusLine() {
    workload::Workload(text_style(Color::from_rgb_u8(200, 204, 220), 14.0));
}

#[composable]
fn Taps() {
    let taps = cranpose_core::rememberMutableStateOf(|| 0u32);
    Box(
        Modifier::empty()
            .fill_max_width()
            .background(Color::from_rgb_u8(98, 160, 255))
            .rounded_corners(12.0)
            .clickable(move |_| taps.set(taps.get() + 1))
            .padding(20.0),
        BoxSpec::default(),
        move || {
            Text(
                format!("Taps: {}", taps.get()),
                Modifier::empty(),
                text_style(Color::from_rgb_u8(18, 20, 30), 18.0),
            );
        },
    );
}

#[composable]
fn Screen() {
    Column(
        Modifier::empty()
            .fill_max_size()
            .background(Color::from_rgb_u8(18, 20, 30))
            .padding(20.0),
        ColumnSpec::default().vertical_arrangement(LinearArrangement::spaced_by(14.0)),
        || {
            Text(
                board::TITLE,
                Modifier::empty(),
                text_style(Color::WHITE, 22.0),
            );
            Text(
                "Composed, laid out and drawn on the chip.",
                Modifier::empty(),
                text_style(Color::from_rgb_u8(160, 166, 190), 14.0),
            );
            Pulse();
            StatusLine();
            Taps();
        },
    );
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
