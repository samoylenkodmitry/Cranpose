//! Steady frames of a small animated screen through the framebuffer path a
//! microcontroller port takes: compose, lay out, then draw each changed area
//! into a pixel buffer. A bar's width animates, so every frame recomposes it.
//!
//! `FRAMES` frames (default 600) run in `steady_frames` after a warm-up; under
//! callgrind, `--toggle-collect=*steady_frames*` counts only those.
//! `STATIC_ROWS` text rows (default 0) that never change sit above the bar;
//! `NEST_DEPTH` plain boxes (default 0) wrap it.

use std::hint::black_box;

use cranpose::{
    SystemTheme,
    framebuffer::{Framebuffer, PixelRect},
    prelude::*,
};
use cranpose_animation::{
    AnimationSpec, RepeatMode, StartOffset, infiniteRepeatable, rememberInfiniteTransition,
};

const WIDTH: u32 = 320;
const HEIGHT: u32 = 240;
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
fn Nested(depth: usize) {
    if depth == 0 {
        AnimatedBar();
    } else {
        Box(Modifier::empty(), BoxSpec::default(), move || {
            Nested(depth - 1);
        });
    }
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
            Nested(nest_depth());
        },
    );
}

/// Runs frames `from..to`, drawing what each changed into `pixels`.
fn run_frames(
    framebuffer: &mut Framebuffer,
    areas: &mut Vec<PixelRect>,
    pixels: &mut [u8],
    from: u64,
    to: u64,
) {
    for frame in from..to {
        if framebuffer.tick(frame * FRAME_NANOS) {
            framebuffer.changed_areas(areas);
            for area in areas.iter() {
                let len = area.width as usize * area.height as usize * 4;
                framebuffer.draw_area(*area, &mut pixels[..len]);
            }
        }
        framebuffer.run_pending_tasks();
    }
}

#[inline(never)]
fn steady_frames(
    framebuffer: &mut Framebuffer,
    areas: &mut Vec<PixelRect>,
    pixels: &mut [u8],
    frames: u64,
) {
    run_frames(
        framebuffer,
        areas,
        pixels,
        WARMUP_FRAMES,
        WARMUP_FRAMES + frames,
    );
}

fn main() {
    let frames = std::env::var("FRAMES")
        .ok()
        .and_then(|value| value.parse().ok())
        .unwrap_or(600);
    let Ok(mut framebuffer) = Framebuffer::new(WIDTH, HEIGHT, 1.0, SystemTheme::Dark, &[], Screen)
    else {
        eprintln!("the framebuffer could not be created");
        return;
    };
    framebuffer.set_active(true);
    let mut areas = Vec::new();
    let mut pixels = vec![0u8; WIDTH as usize * HEIGHT as usize * 4];
    run_frames(&mut framebuffer, &mut areas, &mut pixels, 0, WARMUP_FRAMES);
    steady_frames(&mut framebuffer, &mut areas, &mut pixels, frames);
    println!(
        "frames {frames} checksum {}",
        black_box(pixels.iter().map(|&byte| u64::from(byte)).sum::<u64>())
    );
}
