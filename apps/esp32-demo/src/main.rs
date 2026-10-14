#![deny(unsafe_code)]

use std::time::Duration;

use cranpose::{prelude::*, text::TextUnit};
use cranpose_animation::{
    AnimationSpec, RepeatMode, StartOffset, infiniteRepeatable, rememberInfiniteTransition,
};
use cranpose_platform_esp_idf::{DisplayConfig, Panel, RunError, run};

const WIDTH: u32 = 320;
const HEIGHT: u32 = 240;

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
fn Screen() {
    Column(
        Modifier::empty()
            .fill_max_size()
            .background(Color::from_rgb_u8(18, 20, 30))
            .padding(20.0),
        ColumnSpec::default().vertical_arrangement(LinearArrangement::spaced_by(14.0)),
        || {
            Text(
                "Cranpose on ESP32-S3",
                Modifier::empty(),
                text_style(Color::WHITE, 22.0),
            );
            Text(
                "Composed, laid out and drawn on the chip.",
                Modifier::empty(),
                text_style(Color::from_rgb_u8(160, 166, 190), 14.0),
            );
            Pulse();
            Seconds();
        },
    );
}

fn main() {
    esp_idf_svc::sys::link_patches();
    esp_idf_svc::log::EspLogger::initialize_default();
    let Err(error) = Panel::qemu(WIDTH, HEIGHT)
        .map_err(RunError::from)
        .and_then(|panel| run(panel, DisplayConfig::default(), Screen));
    log::error!("esp32-demo stopped: {error}");
}
