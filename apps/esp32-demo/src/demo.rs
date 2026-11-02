// The demo's own screen: a title, a line of text, a bar that sweeps its
// width, a line that changes and a box that counts taps.

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

// What the line under the bar shows: the seconds since the start, or for
// the benchmarks a number that counts up every frame there instead, as a
// clock or a sensor reading does, drawn by a draw scope that reads it
// (`text-drawn`: frames draw it again without composing or laying out) or
// composed in a `Text` (`text-composed`), or the work of a busier screen
// (`workload`). A line no feature shows is left out: the line passes
// through `StatusLine`'s parameter, so the build cannot drop the others.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Line {
    #[cfg(not(any(
        feature = "text-drawn",
        feature = "text-composed",
        feature = "workload"
    )))]
    Seconds,
    #[cfg(any(feature = "text-drawn", feature = "screens-in-turn"))]
    Drawn,
    #[cfg(any(feature = "text-composed", feature = "screens-in-turn"))]
    Composed,
    #[cfg(any(feature = "workload", feature = "screens-in-turn"))]
    Workload,
}

#[cfg(any(
    all(feature = "text-drawn", feature = "text-composed"),
    all(feature = "text-drawn", feature = "workload"),
    all(feature = "text-composed", feature = "workload"),
))]
compile_error!("text-drawn, text-composed and workload each replace the seconds line: pick one");

// The line the build's features choose.
#[cfg(feature = "text-drawn")]
pub const LINE: Line = Line::Drawn;
#[cfg(feature = "text-composed")]
pub const LINE: Line = Line::Composed;
#[cfg(feature = "workload")]
pub const LINE: Line = Line::Workload;
#[cfg(not(any(
    feature = "text-drawn",
    feature = "text-composed",
    feature = "workload",
    feature = "screens-in-turn"
)))]
pub const LINE: Line = Line::Seconds;

#[cfg(any(
    feature = "text-drawn",
    feature = "text-composed",
    feature = "screens-in-turn"
))]
#[path = "../../../crates/cranpose/tests/support/reading.rs"]
mod reading;
#[cfg(any(feature = "workload", feature = "screens-in-turn"))]
#[path = "../../../crates/cranpose/tests/support/workload.rs"]
mod workload;

// The color and size of the line under the bar.
const LINE_COLOR: Color = Color::from_rgb_u8(200, 204, 220);
const LINE_SIZE: f32 = 14.0;

#[composable]
fn StatusLine(line: Line) {
    match line {
        #[cfg(not(any(
            feature = "text-drawn",
            feature = "text-composed",
            feature = "workload"
        )))]
        Line::Seconds => Seconds(text_style(LINE_COLOR, LINE_SIZE)),
        #[cfg(any(feature = "text-drawn", feature = "screens-in-turn"))]
        Line::Drawn => reading::DrawnReading(LINE_COLOR, LINE_SIZE),
        #[cfg(any(feature = "text-composed", feature = "screens-in-turn"))]
        Line::Composed => reading::ComposedReading(text_style(LINE_COLOR, LINE_SIZE)),
        #[cfg(any(feature = "workload", feature = "screens-in-turn"))]
        Line::Workload => workload::Workload(text_style(LINE_COLOR, LINE_SIZE)),
    }
}

#[cfg(not(any(
    feature = "text-drawn",
    feature = "text-composed",
    feature = "workload"
)))]
#[composable]
fn Seconds(style: TextStyle) {
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
        style,
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
pub fn Screen(line: Line) {
    Column(
        Modifier::empty()
            .fill_max_size()
            .background(Color::from_rgb_u8(18, 20, 30))
            .padding(20.0),
        ColumnSpec::default().vertical_arrangement(LinearArrangement::spaced_by(14.0)),
        move || {
            Text(
                crate::board::TITLE,
                Modifier::empty(),
                text_style(Color::WHITE, 22.0),
            );
            Text(
                "Composed, laid out and drawn on the chip.",
                Modifier::empty(),
                text_style(Color::from_rgb_u8(160, 166, 190), 14.0),
            );
            Pulse();
            StatusLine(line);
            Taps();
        },
    );
}
