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
//! `SCREEN=paths` shows, 320 by 480, rows that scroll and a card that fades
//! while bars inside them change every frame: each frame changes nodes that
//! move or fade and hold a change.
//! `SCREEN=moves` shows, 320 by 480, a list of deep rows with shadows, text
//! and paths that scrolls by itself while each row holds still: each frame
//! moves nodes that do not change. `SCREEN=wall` shows, 320 by 480, a wall
//! of 96 small tiles that hold still under a clock whose text changes every
//! frame. `SCREEN=scroll` shows, 320 by 480, a short viewport over a long
//! column of sixty rows that scrolls by itself: each frame moves every row
//! while the viewport shows a few.
//!
//! `PIXELS=rgb565` draws 16-bit pixels, high byte first, as the ESP32
//! boards' displays take them, in place of RGBA bytes.
//!
//! `MEMORY=tight` reports a heap almost full before the first frame, as a
//! board with little heap left does: lazy lists keep only the items they
//! show, and the renderer keeps less between frames.
//!
//! `FRAME_MS=300` steps the frame clock 300 ms a frame in place of a 60 Hz
//! frame, as a board that draws a screen at about three frames a second
//! does: a screen that moves by time moves farther each frame.
//!
//! `PICTURE=file.ppm` with `REDRAW=all` saves the last frame's RGBA picture,
//! to look at what a screen shows: without it each area lands at the start of
//! the pixels, as a board's strip buffer takes it.

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

#[path = "../tests/support/changing_paths.rs"]
mod changing_paths;
#[path = "../tests/support/dashboard.rs"]
mod dashboard;
#[path = "../tests/support/deep_moves.rs"]
mod deep_moves;
#[path = "../tests/support/long_scroll.rs"]
mod long_scroll;
#[path = "../tests/support/reading.rs"]
mod reading;
#[path = "../tests/support/still_wall.rs"]
mod still_wall;
#[path = "../tests/support/text_style.rs"]
mod text_style;

use reading::{ComposedReading, DrawnReading};

const WIDTH: u32 = 320;
const HEIGHT: u32 = 240;
// The height of the screens the ESP32 boards show.
const TALL_HEIGHT: u32 = 480;
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

// What the frames show: `SCREEN=dashboard`, `paths`, `moves`, `wall`,
// `scroll` or the bar.
#[derive(Clone, Copy)]
enum Shown {
    Bar,
    Dashboard,
    Paths,
    Moves,
    Wall,
    Scroll,
}

impl Shown {
    fn from_env() -> Self {
        match std::env::var("SCREEN").as_deref() {
            Ok("dashboard") => Self::Dashboard,
            Ok("paths") => Self::Paths,
            Ok("moves") => Self::Moves,
            Ok("wall") => Self::Wall,
            Ok("scroll") => Self::Scroll,
            _ => Self::Bar,
        }
    }

    fn height(self) -> u32 {
        match self {
            Self::Bar => HEIGHT,
            Self::Dashboard | Self::Paths | Self::Moves | Self::Wall | Self::Scroll => TALL_HEIGHT,
        }
    }
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

    // Saves an RGBA picture `width` pixels wide to `path` as a binary PPM.
    fn save(&self, width: usize, path: &str) -> std::io::Result<()> {
        let Self::Rgba(bytes) = self else {
            return Err(std::io::Error::other("PICTURE saves RGBA pixels only"));
        };
        let height = bytes.len() / 4 / width;
        let mut file = format!("P6\n{width} {height}\n255\n").into_bytes();
        file.extend(
            bytes
                .as_chunks::<4>()
                .0
                .iter()
                .flat_map(|pixel| &pixel[..3]),
        );
        std::fs::write(path, file)
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
    match Shown::from_env() {
        Shown::Bar => BarScreen(),
        Shown::Dashboard => dashboard::Dashboard("Cranpose"),
        Shown::Paths => changing_paths::ChangingPaths("Cranpose"),
        Shown::Moves => deep_moves::DeepMoves("Cranpose"),
        Shown::Wall => still_wall::StillWall("Cranpose"),
        Shown::Scroll => long_scroll::LongScroll("Cranpose"),
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
    let step = std::env::var("FRAME_MS")
        .ok()
        .and_then(|ms| ms.parse::<u64>().ok())
        .map_or(FRAME_NANOS, |ms| ms * 1_000_000);
    for frame in from..to {
        if framebuffer.tick(frame * step) {
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
    let height = Shown::from_env().height();
    let Ok(mut framebuffer) = Framebuffer::new(WIDTH, height, 1.0, SystemTheme::Dark, &[], Screen)
    else {
        eprintln!("the framebuffer could not be created");
        return;
    };
    framebuffer.set_active(true);
    if std::env::var("MEMORY").is_ok_and(|memory| memory == "tight") {
        framebuffer.report_memory(0, 1);
    }
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
    if let Ok(path) = std::env::var("PICTURE")
        && let Err(error) = pixels.save(WIDTH as usize, &path)
    {
        eprintln!("the picture could not be saved: {error}");
    }
}
