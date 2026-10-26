//! Steady frames of a small animated screen through the framebuffer path a
//! microcontroller port takes: compose, lay out, then draw each changed area
//! into a pixel buffer. A bar's width animates, so every frame recomposes it.
//!
//! `FRAMES` frames (default 600) run in `steady_frames` after a warm-up; under
//! callgrind, `--toggle-collect=*steady_frames*` counts only those.
//! `STATIC_ROWS` text rows (default 0) that never change sit above the bar;
//! `NEST_DEPTH` plain boxes (default 0) wrap it. With `BAR=draw` the bar reads
//! its width in the draw phase instead, as a progress indicator that takes
//! its progress as a function does: frames then draw again without composing
//! or laying out.
//!
//! `TEXT=frame` adds a line of text whose number changes every frame, as a
//! clock or a sensor reading does: each frame composes, measures and draws
//! that text again. `TEXT=draw` draws that line in the draw phase instead,
//! reading the number there: frames then draw it again without composing or
//! laying out.
//!
//! `STRIP_ROWS` (default 0, an area at once) draws each area in strips of
//! that many rows, as a host with a small buffer does, and `REDRAW=all`
//! draws the whole picture each frame, as a host does after a screen change.
//!
//! `SCREEN=dashboard` shows the dashboard of the ESP32 benchmarks, 320 by
//! 480, in place of that screen: most of it changes every frame.
//!
//! `PIXELS=rgb565` draws 16-bit pixels, high byte first, as the ESP32
//! boards' displays take them, in place of RGBA bytes.

#![allow(
    clippy::print_stderr,
    clippy::print_stdout,
    reason = "a benchmark prints its numbers and why it could not run"
)]

use std::hint::black_box;

use cranpose::{
    SystemTheme,
    framebuffer::{Framebuffer, PixelRect, Rgb565Order},
    prelude::*,
};
use cranpose_animation::{
    AnimationSpec, RepeatMode, StartOffset, infiniteRepeatable, rememberInfiniteTransition,
};

#[path = "../tests/support/dashboard.rs"]
mod dashboard;
#[path = "../tests/support/reading.rs"]
mod reading;

use reading::{ComposedReading, DrawnReading};

const WIDTH: u32 = 320;
const HEIGHT: u32 = 240;
const DASHBOARD_HEIGHT: u32 = 480;
const FRAME_NANOS: u64 = 16_666_667;
const WARMUP_FRAMES: u64 = 60;

/// Text rows that never change, above the bar: `STATIC_ROWS` (default 0).
/// Each frame's cost should not grow with them.
fn static_rows() -> usize {
    env_count("STATIC_ROWS")
}

/// Plain boxes around the bar: `NEST_DEPTH` (default 0). Each frame's cost
/// should not grow with them either.
fn nest_depth() -> usize {
    env_count("NEST_DEPTH")
}

/// The line of text that changes every frame, if any: `TEXT=frame` composes
/// it, `TEXT=draw` draws it.
#[derive(Clone, Copy, PartialEq)]
enum TickerText {
    None,
    Composed,
    Drawn,
}

fn ticker_text() -> TickerText {
    match std::env::var("TEXT").as_deref() {
        Ok("frame") => TickerText::Composed,
        Ok("draw") => TickerText::Drawn,
        _ => TickerText::None,
    }
}

/// Whether the bar reads its width while it draws: `BAR=draw`.
fn bar_reads_in_draw() -> bool {
    std::env::var("BAR").is_ok_and(|bar| bar == "draw")
}

// Whether the frames show the dashboard: `SCREEN=dashboard`.
fn shows_dashboard() -> bool {
    std::env::var("SCREEN").is_ok_and(|screen| screen == "dashboard")
}

// How each frame's pixels are drawn: `STRIP_ROWS` and `REDRAW`, on a
// display `height` rows tall.
#[derive(Clone, Copy)]
struct Drawing {
    strip_rows: u32,
    whole_picture: bool,
    height: u32,
}

impl Drawing {
    fn from_env(height: u32) -> Self {
        Self {
            strip_rows: u32::try_from(env_count("STRIP_ROWS")).unwrap_or(u32::MAX),
            whole_picture: std::env::var("REDRAW").is_ok_and(|redraw| redraw == "all"),
            height,
        }
    }

    /// Draws the areas `framebuffer` changed, or its whole picture, into
    /// `pixels`.
    fn draw(self, framebuffer: &mut Framebuffer, areas: &mut Vec<PixelRect>, pixels: &mut Pixels) {
        framebuffer.changed_areas(areas);
        if self.whole_picture {
            areas.clear();
            areas.push(PixelRect {
                x: 0,
                y: 0,
                width: WIDTH,
                height: self.height,
            });
        }
        for area in areas.iter() {
            let rows = match self.strip_rows {
                0 => area.height,
                rows => rows,
            };
            let mut top = area.y;
            while top < area.y + area.height {
                let strip = PixelRect {
                    y: top,
                    height: rows.min(area.y + area.height - top),
                    ..*area
                };
                let len = strip.width as usize * strip.height as usize;
                match pixels {
                    Pixels::Rgba(bytes) => framebuffer.draw_area(strip, &mut bytes[..len * 4]),
                    Pixels::Rgb565(words) => framebuffer.draw_area_rgb565(
                        strip,
                        &mut words[..len],
                        Rgb565Order::BigEndian,
                    ),
                }
                top += strip.height;
            }
        }
    }
}

// The pixels the frames draw into, as `PIXELS` says.
enum Pixels {
    Rgba(Vec<u8>),
    Rgb565(Vec<u16>),
}

impl Pixels {
    fn from_env(len: usize) -> Self {
        if std::env::var("PIXELS").is_ok_and(|pixels| pixels == "rgb565") {
            Self::Rgb565(vec![0; len])
        } else {
            Self::Rgba(vec![0; len * 4])
        }
    }

    fn checksum(&self) -> u64 {
        match self {
            Self::Rgba(bytes) => bytes.iter().map(|&byte| u64::from(byte)).sum(),
            Self::Rgb565(words) => words.iter().map(|&word| u64::from(word)).sum(),
        }
    }
}

fn env_count(name: &str) -> usize {
    std::env::var(name)
        .ok()
        .and_then(|count| count.parse().ok())
        .unwrap_or(0)
}

#[composable]
fn AnimatedBar() {
    let transition = rememberInfiniteTransition("bar");
    let spec = infiniteRepeatable(
        AnimationSpec::linear(1500),
        RepeatMode::Reverse,
        StartOffset::default(),
    );
    let fraction = transition.animateFloat(0.1, 1.0, spec, "width").value();
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
                    .fill_max_width_fraction(fraction)
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
fn DrawnBar() {
    let transition = rememberInfiniteTransition("bar");
    let spec = infiniteRepeatable(
        AnimationSpec::linear(1500),
        RepeatMode::Reverse,
        StartOffset::default(),
    );
    let fraction = transition.animateFloat(0.1, 1.0, spec, "width");
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
                        width: size.width * fraction.value(),
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

#[composable]
fn Nested(depth: usize) {
    if depth == 0 {
        if bar_reads_in_draw() {
            DrawnBar();
        } else {
            AnimatedBar();
        }
    } else {
        Box(Modifier::empty(), BoxSpec::default(), move || {
            Nested(depth - 1);
        });
    }
}

#[composable]
fn Screen() {
    if shows_dashboard() {
        dashboard::Dashboard("Cranpose");
    } else {
        BarScreen();
    }
}

#[composable]
fn BarScreen() {
    Column(
        Modifier::empty()
            .fill_max_size()
            .background(Color::from_rgb_u8(18, 20, 30))
            .padding(20.0),
        ColumnSpec::default().vertical_arrangement(LinearArrangement::spaced_by(14.0)),
        || {
            Text("A title line", Modifier::empty(), TextStyle::default());
            Text(
                "A second, longer line of text",
                Modifier::empty(),
                TextStyle::default(),
            );
            for row in 0..static_rows() {
                cranpose_core::with_key(&row, || {
                    Text(
                        format!("Static row {row}"),
                        Modifier::empty(),
                        TextStyle::default(),
                    );
                });
            }
            match ticker_text() {
                TickerText::None => {}
                TickerText::Composed => ComposedReading(TextStyle::default()),
                TickerText::Drawn => DrawnReading(Color::WHITE, DrawTextStyle::default().font_size),
            }
            Nested(nest_depth());
        },
    );
}

/// Runs frames `from..to`, drawing each into `pixels` as `drawing` says.
fn run_frames(
    framebuffer: &mut Framebuffer,
    (areas, pixels): (&mut Vec<PixelRect>, &mut Pixels),
    drawing: Drawing,
    from: u64,
    to: u64,
) {
    for frame in from..to {
        if framebuffer.tick(frame * FRAME_NANOS) {
            drawing.draw(framebuffer, areas, pixels);
        }
        framebuffer.run_pending_tasks();
    }
}

#[inline(never)]
fn steady_frames(
    framebuffer: &mut Framebuffer,
    target: (&mut Vec<PixelRect>, &mut Pixels),
    drawing: Drawing,
    frames: u64,
) {
    run_frames(
        framebuffer,
        target,
        drawing,
        WARMUP_FRAMES,
        WARMUP_FRAMES + frames,
    );
}

fn main() {
    let frames = std::env::var("FRAMES")
        .ok()
        .and_then(|value| value.parse().ok())
        .unwrap_or(600);
    let height = if shows_dashboard() {
        DASHBOARD_HEIGHT
    } else {
        HEIGHT
    };
    let Ok(mut framebuffer) = Framebuffer::new(WIDTH, height, 1.0, SystemTheme::Dark, &[], Screen)
    else {
        eprintln!("the framebuffer could not be created");
        return;
    };
    framebuffer.set_active(true);
    let drawing = Drawing::from_env(height);
    let mut areas = Vec::new();
    let mut pixels = Pixels::from_env(WIDTH as usize * height as usize);
    run_frames(
        &mut framebuffer,
        (&mut areas, &mut pixels),
        drawing,
        0,
        WARMUP_FRAMES,
    );
    steady_frames(&mut framebuffer, (&mut areas, &mut pixels), drawing, frames);
    println!("frames {frames} checksum {}", black_box(pixels.checksum()));
}
