#![deny(unsafe_code)]

use std::time::Duration;

use cranpose::{prelude::*, text::TextUnit};
use cranpose_animation::{
    AnimationSpec, RepeatMode, StartOffset, infiniteRepeatable, rememberInfiniteTransition,
};
use cranpose_platform_esp_idf::run_board;

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
    let progress = transition
        .animateFloat(0.1, 1.0, spec, "pulse_width")
        .value();
    Box(
        Modifier::empty()
            .fill_max_width()
            .height(16.0)
            .background(Color::from_rgb_u8(40, 44, 64))
            .rounded_corners(8.0),
        BoxSpec::default(),
        move || {
            Box(
                Modifier::empty()
                    .fill_max_width_fraction(progress)
                    .height(16.0)
                    .background(Color::from_rgb_u8(98, 160, 255))
                    .rounded_corners(8.0),
                BoxSpec::default(),
                || {},
            );
        },
    );
}

#[composable]
fn Seconds() {
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
            Text(TITLE, Modifier::empty(), text_style(Color::WHITE, 22.0));
            Text(
                "Composed, laid out and drawn on the chip.",
                Modifier::empty(),
                text_style(Color::from_rgb_u8(160, 166, 190), 14.0),
            );
            Pulse();
            Seconds();
            Taps();
        },
    );
}

#[cfg(all(feature = "es3c35p", feature = "e32r35t"))]
compile_error!("choose one board feature: es3c35p or e32r35t");

#[cfg(feature = "e32r35t")]
const TITLE: &str = "Cranpose on ESP32";
#[cfg(not(feature = "e32r35t"))]
const TITLE: &str = "Cranpose on ESP32-S3";

/// The board the demo runs on.
#[cfg(feature = "es3c35p")]
type Board = cranpose_platform_esp_idf::Es3c35p;
#[cfg(feature = "e32r35t")]
type Board = cranpose_platform_esp_idf::E32r35t;
#[cfg(not(any(feature = "es3c35p", feature = "e32r35t")))]
type Board = cranpose_platform_esp_idf::Qemu;

fn main() {
    esp_idf_svc::sys::link_patches();
    esp_idf_svc::log::EspLogger::initialize_default();
    let Err(error) = run_board::<Board>(FONTS, Screen);
    log::error!("esp32-demo stopped: {error}");
}
