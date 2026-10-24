//! A line of text whose number changes every frame, as a clock or a sensor
//! reading does, for the framebuffer bench and tests and the ESP32 demo's
//! benchmark features.

use cranpose::prelude::*;
use cranpose_animation::{
    AnimationSpec, RepeatMode, StartOffset, infiniteRepeatable, rememberInfiniteTransition,
};

/// A number that counts up one every frame.
#[composable]
fn reading() -> State<f32> {
    let transition = rememberInfiniteTransition("reading");
    let spec = infiniteRepeatable(
        AnimationSpec::linear(100_000),
        RepeatMode::Restart,
        StartOffset::default(),
    );
    transition.animateFloat(0.0, 6000.0, spec, "count")
}

/// The number in a `Text` in `style`, composed again each frame.
#[composable]
pub fn ComposedReading(style: TextStyle) {
    let count = reading().value();
    Text(
        format!("Reading {}", count as u32),
        Modifier::empty(),
        style,
    );
}

/// The number in `color` at `font_size`, drawn by a draw scope that reads
/// it: frames draw it again without composing or laying out.
#[composable]
pub fn DrawnReading(color: Color, font_size: f32) {
    let count = reading();
    Box(
        Modifier::empty()
            .width(120.0)
            .height(20.0)
            .draw_behind(move |scope| {
                scope.draw_text_from(
                    Point::new(0.0, 0.0),
                    Brush::solid(color),
                    &format!("Reading {}", count.value() as u32),
                    &DrawTextStyle {
                        font_size,
                        ..DrawTextStyle::default()
                    },
                );
            }),
        BoxSpec::default(),
        || {},
    );
}
