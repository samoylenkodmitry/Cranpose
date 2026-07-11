//! Color fidelity contract: authored sRGB `Color` values must land on screen
//! byte-for-byte for solid fills, gradient stops, and text.
//!
//! Guards the surface-format policy — an sRGB swapchain view double-encodes
//! the framework's sRGB-space colors, washing out the entire UI (a solid
//! `(52, 199, 89)` fill used to display as `(125, 229, 160)`).
//!
//! Run with:
//! `cargo run --package desktop-app --example robot_color_fidelity --features robot-app`

use cranpose::widgets::{Box, BoxSpec, Column, ColumnSpec, Row, RowSpec, Text};
use cranpose::{AppLauncher, Brush, Color, CornerRadii, Modifier, Point, Size};
use cranpose_testing::sample_screenshot_pixel_logical;
use cranpose_ui::text::{SpanStyle, TextStyle, TextUnit};
use std::time::Duration;

const WINDOW_WIDTH: u32 = 600;
const WINDOW_HEIGHT: u32 = 260;

const SOLID: Color = Color(52.0 / 255.0, 199.0 / 255.0, 89.0 / 255.0, 1.0);
const GRADIENT_END: Color = Color(255.0 / 255.0, 45.0 / 255.0, 85.0 / 255.0, 1.0);
const TEXT_COLOR: Color = Color(17.0 / 255.0, 17.0 / 255.0, 20.0 / 255.0, 1.0);
const BACKGROUND: Color = Color(1.0, 1.0, 1.0, 1.0);

/// Anti-aliasing-free interior sample points.
const SOLID_PROBE: (f32, f32) = (75.0, 90.0);
const GRADIENT_SAME_PROBE: (f32, f32) = (225.0, 90.0);
const GRADIENT_MIX_START_PROBE: (f32, f32) = (308.0, 90.0);
const TEXT_REGION: (f32, f32, f32, f32) = (20.0, 190.0, 300.0, 250.0);

fn main() {
    let _ = env_logger::try_init();
    AppLauncher::new()
        .with_title("Robot Color Fidelity")
        .with_size(WINDOW_WIDTH, WINDOW_HEIGHT)
        .with_fonts(desktop_app::fonts::DEMO_FONTS)
        .with_headless(std::env::var("CRANPOSE_HEADLESS").as_deref() != Ok("0"))
        .with_test_driver(move |robot| {
            std::thread::sleep(Duration::from_millis(600));
            let _ = robot.wait_for_idle();
            let screenshot = robot.screenshot().expect("screenshot");

            let expect_exact = |label: &str, (x, y): (f32, f32), expected: Color| {
                let Some(actual) = sample_screenshot_pixel_logical(&screenshot, x, y) else {
                    fail(&robot, &format!("{label}: sample out of bounds"));
                };
                let expected = [
                    (expected.0 * 255.0).round() as u8,
                    (expected.1 * 255.0).round() as u8,
                    (expected.2 * 255.0).round() as u8,
                ];
                let actual = [actual[0], actual[1], actual[2]];
                // ±1 absorbs backend rounding; a double sRGB encode is tens
                // of steps off, far outside this window.
                let close = actual
                    .iter()
                    .zip(expected.iter())
                    .all(|(a, e)| (*a as i16 - *e as i16).abs() <= 1);
                if !close {
                    fail(
                        &robot,
                        &format!("{label}: expected {expected:?}, got {actual:?}"),
                    );
                }
                println!("{label}: {actual:?} == {expected:?}");
            };

            expect_exact("solid fill", SOLID_PROBE, SOLID);
            expect_exact("gradient same-endpoints", GRADIENT_SAME_PROBE, SOLID);
            expect_exact("gradient first stop", GRADIENT_MIX_START_PROBE, SOLID);

            // Text: glyph AA never reaches the authored color exactly, but the
            // darkest core pixel must be within a few steps of it. The washed
            // pipeline rendered (17,17,20) text at (73,73,79).
            let (left, top, right, bottom) = TEXT_REGION;
            let mut darkest = [255u8; 3];
            let mut darkest_sum = 765u32;
            let mut x = left;
            while x <= right {
                let mut y = top;
                while y <= bottom {
                    if let Some(pixel) = sample_screenshot_pixel_logical(&screenshot, x, y) {
                        let sum = pixel[0] as u32 + pixel[1] as u32 + pixel[2] as u32;
                        if sum < darkest_sum {
                            darkest_sum = sum;
                            darkest = [pixel[0], pixel[1], pixel[2]];
                        }
                    }
                    y += 1.0;
                }
                x += 1.0;
            }
            let text_expected = [17u8, 17, 20];
            let text_ok = darkest
                .iter()
                .zip(text_expected.iter())
                .all(|(a, e)| (*a as i16 - *e as i16).abs() <= 8);
            if !text_ok {
                fail(
                    &robot,
                    &format!("text core: expected ~{text_expected:?}, darkest {darkest:?}"),
                );
            }
            println!("text core: darkest {darkest:?} ~= {text_expected:?}");

            println!("PASS: color fidelity contract");
            robot.exit().expect("exit");
        })
        .run(content);
}

fn fail(robot: &cranpose::Robot, message: &str) -> ! {
    println!("FATAL: {message}");
    let _ = robot.exit();
    std::process::exit(1);
}

fn content() {
    Box(
        Modifier::empty()
            .fill_max_size()
            .draw_behind(|scope| scope.draw_rect(Brush::solid(BACKGROUND))),
        BoxSpec::default(),
        || {
            Column(Modifier::empty(), ColumnSpec::default(), || {
                Row(Modifier::empty(), RowSpec::default(), || {
                    Box(
                        Modifier::empty()
                            .size(Size::new(150.0, 180.0))
                            .draw_behind(|scope| scope.draw_rect(Brush::solid(SOLID))),
                        BoxSpec::default(),
                        || {},
                    );
                    Box(
                        Modifier::empty()
                            .size(Size::new(150.0, 180.0))
                            .draw_behind(|scope| {
                                let size = scope.size();
                                scope.draw_round_rect(
                                    Brush::linear_gradient_range(
                                        vec![SOLID, SOLID],
                                        Point::new(0.0, 0.0),
                                        Point::new(size.width, size.height),
                                    ),
                                    CornerRadii::uniform(0.0),
                                );
                            }),
                        BoxSpec::default(),
                        || {},
                    );
                    Box(
                        Modifier::empty()
                            .size(Size::new(300.0, 180.0))
                            .draw_behind(|scope| {
                                let size = scope.size();
                                scope.draw_round_rect(
                                    // 20px clamp zones on both ends give the
                                    // probes exact first-stop pixels.
                                    Brush::linear_gradient_range(
                                        vec![SOLID, GRADIENT_END],
                                        Point::new(20.0, 0.0),
                                        Point::new(size.width - 20.0, 0.0),
                                    ),
                                    CornerRadii::uniform(0.0),
                                );
                            }),
                        BoxSpec::default(),
                        || {},
                    );
                });
                Text(
                    "Color fidelity probe",
                    Modifier::empty().padding_each(20.0, 14.0, 0.0, 0.0),
                    TextStyle {
                        span_style: SpanStyle {
                            color: Some(TEXT_COLOR),
                            font_size: TextUnit::Sp(24.0),
                            font_weight: Some(cranpose_ui::text::FontWeight::BOLD),
                            ..Default::default()
                        },
                        ..Default::default()
                    },
                );
            });
        },
    );
}
