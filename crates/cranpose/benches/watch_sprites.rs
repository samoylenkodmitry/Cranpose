//! Steady frames of a watch screen through the sprite path the watchOS host
//! takes: compose, lay out, then turn the picture into sprites for its GPU
//! compositor. `ROWS` text rows (default 12) that never change sit above a
//! bar whose width animates, so every frame draws again and looks up the
//! texture of every text it shows. `SHADOW=1` lifts each row on an elevation
//! shadow, whose sprite is a texture too. `SCREEN=paths` replaces the rows
//! and the bar with a list that scrolls and a card that fades while bars
//! inside them change every frame. `SCREEN=moves` shows a list of deep rows
//! that scrolls while each row holds still, and `SCREEN=wall` a wall of
//! small tiles that hold still under a clock that changes every frame. Run
//! it under `/usr/bin/time -l`
//! with `FRAMES=0` and `FRAMES=10000` and divide the difference in retired
//! instructions by 10000. `PICTURE=1` prints a hash of every frame's sprites,
//! texture uploads and releases, which two builds that show the same frames
//! print the same.

#![allow(
    clippy::print_stderr,
    clippy::print_stdout,
    reason = "a benchmark prints its numbers and why it could not run"
)]

use std::{
    hash::{DefaultHasher, Hasher},
    hint::black_box,
};

use cranpose::{prelude::*, watchos::Application};
use cranpose_animation::{
    AnimationSpec, RepeatMode, StartOffset, infiniteRepeatable, rememberInfiniteTransition,
};

#[path = "../tests/support/changing_paths.rs"]
mod changing_paths;
#[path = "../tests/support/deep_moves.rs"]
mod deep_moves;
#[path = "../tests/support/still_wall.rs"]
mod still_wall;

const WIDTH: u32 = 198;
const HEIGHT: u32 = 242;
const DENSITY: f32 = 2.0;
const FRAME_NANOS: u64 = 16_666_667;
const WARMUP_FRAMES: u64 = 60;

fn env_count(name: &str, default: usize) -> usize {
    std::env::var(name)
        .ok()
        .and_then(|value| value.parse().ok())
        .unwrap_or(default)
}

#[composable]
fn Screen(rows: usize, shadow: bool) {
    Column(
        Modifier::empty().fill_max_size().padding(4.0),
        ColumnSpec::default(),
        move || {
            for row in 0..rows {
                let card = Modifier::empty();
                let card = if shadow { card.shadow(4.0) } else { card };
                Text(
                    format!("Row {row}: 72 bpm"),
                    card.background(Color::from_rgb_u8(30, 34, 48))
                        .rounded_corners(6.0),
                    TextStyle::default(),
                );
            }
            AnimatedBar();
        },
    );
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
            .fill_max_width_fraction(fraction)
            .height(8.0)
            .background(Color::from_rgb_u8(98, 160, 255)),
        BoxSpec::default(),
        || {},
    );
}

fn main() {
    let rows = env_count("ROWS", 12);
    let frames = env_count("FRAMES", 1000) as u64;
    let shadow = env_count("SHADOW", 0) != 0;
    let shown = std::env::var("SCREEN").unwrap_or_default();
    let screen = move || match shown.as_str() {
        "paths" => changing_paths::ChangingPaths("Cranpose"),
        "moves" => deep_moves::DeepMoves("Cranpose"),
        "wall" => still_wall::StillWall("Cranpose"),
        _ => Screen(rows, shadow),
    };
    let mut app = match Application::new(WIDTH, HEIGHT, DENSITY, screen) {
        Ok(app) => app,
        Err(error) => {
            eprintln!("no surface: {error:?}");
            return;
        }
    };
    app.set_active(true);
    let picture = env_count("PICTURE", 0) != 0;
    let mut hasher = DefaultHasher::new();
    let mut sprites = 0;
    for frame in 1..=WARMUP_FRAMES + frames {
        app.tick(frame * FRAME_NANOS);
        let shown = app.sprites();
        sprites = black_box(shown.sprite_count());
        if picture {
            for value in shown.sprites() {
                hasher.write_u32(value.to_bits());
            }
            for upload in shown.uploads() {
                hasher.write_u32(upload.id);
                hasher.write_u32(upload.width);
                hasher.write_u32(upload.height);
                hasher.write(&upload.pixels);
            }
            for &release in shown.releases() {
                hasher.write_u32(release);
            }
        }
    }
    println!("rows {rows} frames {frames} sprites {sprites}");
    if picture {
        println!("picture {:016x}", hasher.finish());
    }
}
