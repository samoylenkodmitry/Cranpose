//! Adaptive-frost contract: a glass with positive `adaptive_contrast` must
//! darken over a BRIGHT backdrop (protecting light foreground content from
//! white-on-white) while staying inert over a dark one — measured against an
//! identical non-adaptive glass in the same scene.
//!
//! Run with:
//! `cargo run --package desktop-app --example robot_adaptive_frost --features desktop,robot-app`

use cranpose::liquid::prelude::*;
use cranpose::widgets::{Box as CBox, BoxSpec};
use cranpose::{AppLauncher, Color, Modifier, Size};
use std::process::ExitCode;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

const WINDOW_WIDTH: u32 = 460;
const WINDOW_HEIGHT: u32 = 340;
const PANEL_W: f32 = 230.0;
const GLASS_W: f32 = 150.0;
const GLASS_H: f32 = 52.0;
const ADAPTIVE_Y: f32 = 60.0;
const PLAIN_Y: f32 = 180.0;

static FAILED: AtomicBool = AtomicBool::new(false);

fn main() -> ExitCode {
    let _ = env_logger::try_init();

    AppLauncher::new()
        .with_title("Adaptive Frost Contract")
        .with_size(WINDOW_WIDTH, WINDOW_HEIGHT)
        .with_fonts(desktop_app::fonts::DEMO_FONTS)
        .with_headless(std::env::var("CRANPOSE_HEADLESS").as_deref() != Ok("0"))
        .with_test_driver(move |robot| {
            std::thread::sleep(Duration::from_millis(900));
            let _ = robot.wait_for_idle();
            let shot = robot.screenshot().expect("shot");

            let sample = |x: f32, y: f32| -> f32 {
                // Mean luma of the capsule's interior (clear of the rim).
                mean_luma(&shot, x + 25.0, y + 14.0, GLASS_W - 50.0, GLASS_H - 28.0)
            };
            let white_cx = (PANEL_W - GLASS_W) * 0.5;
            let black_cx = PANEL_W + (PANEL_W - GLASS_W) * 0.5;
            let adaptive_white = sample(white_cx, ADAPTIVE_Y);
            let plain_white = sample(white_cx, PLAIN_Y);
            let adaptive_black = sample(black_cx, ADAPTIVE_Y);
            let plain_black = sample(black_cx, PLAIN_Y);
            println!(
                "white: adaptive={adaptive_white:.1} plain={plain_white:.1}  \
                 black: adaptive={adaptive_black:.1} plain={plain_black:.1}"
            );

            if plain_white - adaptive_white < 8.0 {
                fail(
                    &robot,
                    &format!(
                        "adaptive glass must darken over a bright backdrop: \
                         adaptive {adaptive_white:.1} vs plain {plain_white:.1}"
                    ),
                );
            }
            if (adaptive_black - plain_black).abs() > 4.0 {
                fail(
                    &robot,
                    &format!(
                        "positive adaptive strength must be inert over a dark backdrop: \
                         adaptive {adaptive_black:.1} vs plain {plain_black:.1}"
                    ),
                );
            }

            println!("PASS: adaptive frost contract");
            robot.exit().expect("exit");
        })
        .try_run(move || {
            LiquidTheme(LiquidThemeSpec::default(), || {
                CBox(
                    Modifier::empty().size(Size {
                        width: WINDOW_WIDTH as f32,
                        height: WINDOW_HEIGHT as f32,
                    }),
                    BoxSpec::default(),
                    || {
                        // Bright and dark backdrops side by side.
                        CBox(
                            Modifier::empty()
                                .size(Size {
                                    width: PANEL_W,
                                    height: WINDOW_HEIGHT as f32,
                                })
                                .background(Color(0.97, 0.97, 0.97, 1.0)),
                            BoxSpec::default(),
                            || {},
                        );
                        CBox(
                            Modifier::empty()
                                .absolute_offset(PANEL_W, 0.0)
                                .size(Size {
                                    width: PANEL_W,
                                    height: WINDOW_HEIGHT as f32,
                                })
                                .background(Color(0.06, 0.06, 0.07, 1.0)),
                            BoxSpec::default(),
                            || {},
                        );
                        for (x, y, strength) in [
                            ((PANEL_W - GLASS_W) * 0.5, ADAPTIVE_Y, 0.35),
                            ((PANEL_W - GLASS_W) * 0.5, PLAIN_Y, 0.0),
                            (PANEL_W + (PANEL_W - GLASS_W) * 0.5, ADAPTIVE_Y, 0.35),
                            (PANEL_W + (PANEL_W - GLASS_W) * 0.5, PLAIN_Y, 0.0),
                        ] {
                            CBox(
                                Modifier::empty()
                                    .absolute_offset(x, y)
                                    .size(Size {
                                        width: GLASS_W,
                                        height: GLASS_H,
                                    })
                                    .glass_effect(
                                        Glass::regular()
                                            .blur_radius(10.0)
                                            .adaptive_contrast(strength),
                                    ),
                                BoxSpec::default(),
                                || {},
                            );
                        }
                    },
                );
            });
        })
        .expect("launch adaptive frost runner");

    if FAILED.load(Ordering::Relaxed) {
        ExitCode::FAILURE
    } else {
        ExitCode::SUCCESS
    }
}

fn fail(robot: &cranpose::Robot, message: &str) -> ! {
    println!("FATAL: {message}");
    FAILED.store(true, Ordering::Relaxed);
    std::thread::spawn(|| {
        std::thread::sleep(Duration::from_secs(15));
        std::process::exit(1);
    });
    let _ = robot.exit();
    loop {
        std::thread::sleep(Duration::from_secs(60));
    }
}

fn mean_luma(shot: &cranpose::RobotScreenshot, x: f32, y: f32, w: f32, h: f32) -> f32 {
    let sx = shot.width as f32 / shot.logical_width.max(1.0);
    let sy = shot.height as f32 / shot.logical_height.max(1.0);
    let (x0, y0) = ((x * sx) as usize, (y * sy) as usize);
    let (x1, y1) = (
        (((x + w) * sx) as usize).min(shot.width as usize),
        (((y + h) * sy) as usize).min(shot.height as usize),
    );
    let mut sum = 0.0f64;
    let mut count = 0usize;
    for py in y0..y1 {
        for px in x0..x1 {
            let i = (py * shot.width as usize + px) * 4;
            sum += 0.2126 * shot.pixels[i] as f64
                + 0.7152 * shot.pixels[i + 1] as f64
                + 0.0722 * shot.pixels[i + 2] as f64;
            count += 1;
        }
    }
    if count == 0 {
        0.0
    } else {
        (sum / count as f64) as f32
    }
}
