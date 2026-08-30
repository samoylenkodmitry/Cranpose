mod robot_exit;

use std::{path::PathBuf, process::ExitCode, sync::atomic::AtomicBool, time::Duration};

use cranpose::{
    liquid::prelude::*,
    text::{FontWeight, SpanStyle, TextStyle, TextUnit},
    widgets::{Box as CBox, BoxSpec, Text},
    Alignment, AppLauncher, Color, Modifier, Size,
};
use image::RgbaImage;

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
    if std::env::var("CRANPOSE_ROBOT_SOFTWARE_RENDERER").as_deref() == Ok("1") {
        println!("PASS: adaptive frost contract (skipped on software renderer)");
        return ExitCode::SUCCESS;
    }
    let shot_dir = PathBuf::from(
        std::env::var("ROBOT_SHOT_DIR").unwrap_or_else(|_| "target/adaptive-frost".to_string()),
    );
    std::fs::create_dir_all(&shot_dir).expect("create adaptive frost shot dir");

    AppLauncher::new()
        .with_title("Adaptive Frost Contract")
        .with_size(WINDOW_WIDTH, WINDOW_HEIGHT)
        .with_fonts(desktop_app::fonts::DEMO_FONTS)
        .with_headless(std::env::var("CRANPOSE_HEADLESS").as_deref() != Ok("0"))
        .with_test_driver(move |robot| {
            std::thread::sleep(Duration::from_millis(900));
            let _ = robot.wait_for_idle();
            let mut shot = robot.screenshot().expect("shot");
            for _ in 0..60 {
                let bright_panel = mean_luma(&shot, PANEL_W * 0.5 - 30.0, 6.0, 60.0, 18.0);
                let dark_panel = mean_luma(&shot, PANEL_W * 1.5 - 30.0, 6.0, 60.0, 18.0);
                let plain_dark_face =
                    mean_luma(&shot, PANEL_W + (PANEL_W - GLASS_W) * 0.5 + 18.0, PLAIN_Y + 14.0, 22.0, GLASS_H - 28.0);
                if bright_panel > 225.0 && dark_panel < 30.0 && plain_dark_face < 90.0 {
                    break;
                }
                std::thread::sleep(Duration::from_millis(250));
                let _ = robot.wait_for_idle();
                shot = robot.screenshot().expect("shot");
            }
            let shot = shot;
            let image = RgbaImage::from_raw(shot.width, shot.height, shot.pixels.clone())
                .expect("adaptive frost screenshot buffer");
            image
                .save(shot_dir.join("adaptive-frost-foregrounds.png"))
                .expect("save adaptive frost proof");

            let sample = |x: f32, y: f32| -> f32 {
                mean_luma(&shot, x + 18.0, y + 14.0, 22.0, GLASS_H - 28.0)
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
            let white_contrast = contrast_ratio(255.0, adaptive_white);
            let black_contrast = contrast_ratio(0.0, adaptive_black);
            let white_text_extrema = region_luma_extrema(
                &shot,
                white_cx + 45.0,
                ADAPTIVE_Y + 10.0,
                GLASS_W - 90.0,
                GLASS_H - 20.0,
            );
            let black_text_extrema = region_luma_extrema(
                &shot,
                black_cx + 45.0,
                ADAPTIVE_Y + 10.0,
                GLASS_W - 90.0,
                GLASS_H - 20.0,
            );
            println!(
                "foreground contrast white={white_contrast:.2}:1 black={black_contrast:.2}:1 extrema white={white_text_extrema:?} black={black_text_extrema:?}"
            );

            if plain_white - adaptive_white < 8.0 {
                robot_exit::fail_and_await_shutdown(&robot, &FAILED,
                    &format!(
                        "adaptive glass must darken over a bright backdrop: \
                         adaptive {adaptive_white:.1} vs plain {plain_white:.1}"
                    ));
            }
            if adaptive_black - plain_black < 8.0 {
                robot_exit::fail_and_await_shutdown(&robot, &FAILED,
                    &format!(
                        "adaptive glass must lighten behind dark foreground on a dark backdrop: \
                         adaptive {adaptive_black:.1} vs plain {plain_black:.1}"
                    ));
            }
            if white_text_extrema.1 < 225.0 || black_text_extrema.0 > 30.0 {
                robot_exit::fail_and_await_shutdown(&robot, &FAILED,
                    &format!(
                        "adaptive proof labels did not render at their requested foreground polarity: white {white_text_extrema:?}, black {black_text_extrema:?}"
                    ));
            }
            if white_contrast < 4.5 || black_contrast < 4.5 {
                robot_exit::fail_and_await_shutdown(&robot, &FAILED,
                    &format!(
                        "adaptive frost failed foreground contrast: white {white_contrast:.2}:1, black {black_contrast:.2}:1"
                    ));
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
                        for (x, y, foreground, strength, label) in [
                            (
                                (PANEL_W - GLASS_W) * 0.5,
                                ADAPTIVE_Y,
                                Color::WHITE,
                                0.65,
                                "WHITE",
                            ),
                            (
                                (PANEL_W - GLASS_W) * 0.5,
                                PLAIN_Y,
                                Color::WHITE,
                                0.0,
                                "WHITE",
                            ),
                            (
                                PANEL_W + (PANEL_W - GLASS_W) * 0.5,
                                ADAPTIVE_Y,
                                Color::BLACK,
                                0.65,
                                "BLACK",
                            ),
                            (
                                PANEL_W + (PANEL_W - GLASS_W) * 0.5,
                                PLAIN_Y,
                                Color::BLACK,
                                0.0,
                                "BLACK",
                            ),
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
                                            .adaptive_frost(foreground, strength),
                                    ),
                                BoxSpec::default().content_alignment(Alignment::CENTER),
                                move || {
                                    Text(
                                        label,
                                        Modifier::empty(),
                                        TextStyle {
                                            span_style: SpanStyle {
                                                color: Some(foreground),
                                                font_size: TextUnit::Sp(18.0),
                                                font_weight: Some(FontWeight::BOLD),
                                                ..Default::default()
                                            },
                                            ..Default::default()
                                        },
                                    );
                                },
                            );
                        }
                    },
                );
            });
        })
        .expect("launch adaptive frost runner");

    robot_exit::exit_code(&FAILED)
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

fn region_luma_extrema(
    shot: &cranpose::RobotScreenshot,
    x: f32,
    y: f32,
    w: f32,
    h: f32,
) -> (f32, f32) {
    let sx = shot.width as f32 / shot.logical_width.max(1.0);
    let sy = shot.height as f32 / shot.logical_height.max(1.0);
    let (x0, y0) = ((x * sx) as usize, (y * sy) as usize);
    let (x1, y1) = (
        (((x + w) * sx) as usize).min(shot.width as usize),
        (((y + h) * sy) as usize).min(shot.height as usize),
    );
    let mut darkest = 255.0f32;
    let mut lightest = 0.0f32;
    for py in y0..y1 {
        for px in x0..x1 {
            let i = (py * shot.width as usize + px) * 4;
            let luma = 0.2126 * shot.pixels[i] as f32
                + 0.7152 * shot.pixels[i + 1] as f32
                + 0.0722 * shot.pixels[i + 2] as f32;
            darkest = darkest.min(luma);
            lightest = lightest.max(luma);
        }
    }
    (darkest, lightest)
}

fn contrast_ratio(foreground_luma: f32, backdrop_luma: f32) -> f32 {
    fn linear(byte_luma: f32) -> f32 {
        let value = (byte_luma / 255.0).clamp(0.0, 1.0);
        if value <= 0.04045 {
            value / 12.92
        } else {
            ((value + 0.055) / 1.055).powf(2.4)
        }
    }
    let foreground = linear(foreground_luma);
    let backdrop = linear(backdrop_luma);
    let lighter = foreground.max(backdrop);
    let darker = foreground.min(backdrop);
    (lighter + 0.05) / (darker + 0.05)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn contrast_ratio_matches_wcag_endpoints() {
        assert!((contrast_ratio(255.0, 0.0) - 21.0).abs() < 0.01);
        assert!((contrast_ratio(0.0, 255.0) - 21.0).abs() < 0.01);
        assert!((contrast_ratio(128.0, 128.0) - 1.0).abs() < 0.01);
    }
}
