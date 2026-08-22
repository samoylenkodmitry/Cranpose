//! Robot contract for image atlas isolation and rested scroll sharpness.

mod output_paths;

use cranpose::AppLauncher;
use cranpose_testing::crop_screenshot_logical;
use cranpose_ui::{
    composable, text::SpanStyle, text::TextUnit, Alignment, Box, BoxSpec, Brush, Canvas, Color,
    ImageBitmap, Modifier, Rect, ScrollState, Size, Text, TextStyle,
};
use cranpose_ui_graphics::DrawScope;
use image::{ImageBuffer, RgbaImage};
use std::path::Path;
use std::time::Duration;

const WINDOW_WIDTH: u32 = 320;
const WINDOW_HEIGHT: u32 = 170;
const CAPTURE_SCALE: f32 = 1.25;

const ATLAS_X: f32 = 20.0;
const ATLAS_Y: f32 = 18.0;
const ATLAS_SIZE: f32 = 48.0;
const STATIC_X: f32 = 20.0;
const STATIC_Y: f32 = 86.4;
const SCROLLED_X: f32 = 170.4;
const SCROLLED_Y: f32 = 86.4;
const INITIAL_SCROLL: f32 = 1.0;
const BLOCK_W: f32 = 104.0;
const BLOCK_H: f32 = 52.0;
const SCROLL_COMPARE_TOP: f32 = 1.0;
const SCROLL_COMPARE_H: f32 = BLOCK_H - 2.0;
const GRID_COMPARE_TOP: f32 = 26.4;
const GRID_COMPARE_H: f32 = 20.0;

fn fail(robot: &cranpose::Robot, message: &str) -> ! {
    println!("FATAL: {message}");
    let _ = robot.exit();
    std::process::exit(1);
}

fn save_png(path: &Path, screenshot: &cranpose::RobotScreenshot) -> Result<(), String> {
    let image: RgbaImage = ImageBuffer::from_raw(
        screenshot.width,
        screenshot.height,
        screenshot.pixels.clone(),
    )
    .ok_or_else(|| "invalid screenshot dimensions".to_string())?;
    image
        .save(path)
        .map_err(|err| format!("failed to save {}: {}", path.display(), err))
}

fn atlas_bitmap() -> ImageBitmap {
    const WIDTH: u32 = 24;
    const HEIGHT: u32 = 16;
    let mut pixels = vec![0u8; (WIDTH * HEIGHT * 4) as usize];
    for y in 0..HEIGHT {
        for x in 0..WIDTH {
            let rgba = if x >= 16 {
                [0, 66, 255, 255]
            } else if ((x / 2) + (y / 2)) % 2 == 0 {
                [238, 244, 250, 255]
            } else {
                [18, 26, 38, 255]
            };
            let index = ((y * WIDTH + x) * 4) as usize;
            pixels[index..index + 4].copy_from_slice(&rgba);
        }
    }
    ImageBitmap::from_rgba8(WIDTH, HEIGHT, pixels).expect("valid atlas bitmap")
}

fn blue_leak_pixels(screenshot: &cranpose::RobotScreenshot) -> usize {
    screenshot
        .pixels
        .as_chunks::<4>()
        .0
        .iter()
        .filter(|rgba| rgba[2] > 118 && rgba[2] > rgba[0].saturating_add(42))
        .count()
}

fn luma(pixel: &[u8]) -> i32 {
    (77 * pixel[0] as i32 + 150 * pixel[1] as i32 + 29 * pixel[2] as i32) / 256
}

fn edge_energy(screenshot: &cranpose::RobotScreenshot) -> f32 {
    let width = screenshot.width as usize;
    let height = screenshot.height as usize;
    let mut total = 0u64;
    let mut samples = 0u64;
    for y in 0..height {
        for x in 0..width {
            let index = (y * width + x) * 4;
            let current = luma(&screenshot.pixels[index..index + 4]);
            if x + 1 < width {
                let right = (y * width + x + 1) * 4;
                total +=
                    (current - luma(&screenshot.pixels[right..right + 4])).unsigned_abs() as u64;
                samples += 1;
            }
            if y + 1 < height {
                let down = ((y + 1) * width + x) * 4;
                total += (current - luma(&screenshot.pixels[down..down + 4])).unsigned_abs() as u64;
                samples += 1;
            }
        }
    }
    if samples == 0 {
        0.0
    } else {
        total as f32 / samples as f32
    }
}

fn unique_rgb_count(screenshot: &cranpose::RobotScreenshot) -> usize {
    let mut colors = std::collections::HashSet::new();
    for rgba in screenshot.pixels.as_chunks::<4>().0 {
        colors.insert([rgba[0], rgba[1], rgba[2]]);
    }
    colors.len()
}

fn snapped_content_origin_y() -> f32 {
    ((SCROLLED_Y - INITIAL_SCROLL) * CAPTURE_SCALE).round() / CAPTURE_SCALE
}

fn draw_control_pattern(scope: &mut dyn DrawScope) {
    let size = scope.size();
    scope.draw_rect(Brush::solid(Color::from_rgb_u8(15, 21, 30)));
    let mut x = 4.8;
    while x < 100.0 {
        scope.draw_rect_at(
            Rect {
                x,
                y: 0.0,
                width: 0.8,
                height: size.height,
            },
            Brush::solid(Color::from_rgb_u8(245, 250, 255)),
        );
        x += 6.4;
    }
    let mut y = 4.8;
    while y < 48.0 {
        scope.draw_rect_at(
            Rect {
                x: 0.0,
                y,
                width: size.width,
                height: 0.8,
            },
            Brush::solid(Color::from_rgb_u8(78, 220, 170)),
        );
        y += 7.2;
    }
    scope.draw_rect_at(
        Rect {
            x: 0.0,
            y: 0.0,
            width: size.width,
            height: 0.8,
        },
        Brush::solid(Color::from_rgb_u8(245, 250, 255)),
    );
}

#[allow(non_snake_case)]
#[composable]
fn CrispControlBlock(modifier: Modifier) {
    Text(
        "MIX 128",
        modifier
            .size(Size::new(BLOCK_W, BLOCK_H))
            .draw_behind(draw_control_pattern),
        TextStyle::from_span_style(SpanStyle {
            color: Some(Color::from_rgb_u8(248, 252, 255)),
            font_size: TextUnit::Sp(18.0),
            ..SpanStyle::default()
        }),
    );
}

#[allow(non_snake_case)]
#[composable]
fn RenderCrispnessContractApp() {
    let atlas = cranpose_core::remember(atlas_bitmap).with(|bitmap| bitmap.clone());
    let scroll_state =
        cranpose_core::remember(|| ScrollState::new(INITIAL_SCROLL)).with(|state| *state);

    Box(
        Modifier::empty()
            .size(Size::new(WINDOW_WIDTH as f32, WINDOW_HEIGHT as f32))
            .background(Color::from_rgb_u8(35, 39, 50)),
        BoxSpec::default().content_alignment(Alignment::TOP_START),
        move || {
            Canvas(
                Modifier::empty()
                    .offset(ATLAS_X, ATLAS_Y)
                    .size(Size::new(ATLAS_SIZE, ATLAS_SIZE)),
                {
                    let atlas = atlas.clone();
                    move |scope| {
                        scope.draw_image_src(
                            atlas.clone(),
                            Rect {
                                x: 0.0,
                                y: 0.0,
                                width: 16.0,
                                height: 16.0,
                            },
                            Rect {
                                x: 0.0,
                                y: 0.0,
                                width: ATLAS_SIZE,
                                height: ATLAS_SIZE,
                            },
                            1.0,
                            None,
                        );
                    }
                },
            );

            CrispControlBlock(Modifier::empty().offset(STATIC_X, STATIC_Y));

            Box(
                Modifier::empty()
                    .offset(SCROLLED_X, SCROLLED_Y)
                    .size(Size::new(BLOCK_W + 6.0, BLOCK_H))
                    .vertical_scroll(scroll_state, false),
                BoxSpec::default().content_alignment(Alignment::TOP_START),
                || {
                    CrispControlBlock(Modifier::empty());
                    Box(
                        Modifier::empty()
                            .offset(0.0, BLOCK_H + 20.0)
                            .size(Size::new(1.0, 1.0)),
                        BoxSpec::default(),
                        || {},
                    );
                },
            );
        },
    );
}

fn main() {
    env_logger::init();
    println!("=== Robot Render Crispness Contract ===");

    AppLauncher::new()
        .with_title("Robot Render Crispness Contract")
        .with_size(WINDOW_WIDTH, WINDOW_HEIGHT)
        .with_fonts(desktop_app::fonts::DEMO_FONTS)
        .with_headless(true)
        .with_test_driver(|robot| {
            std::thread::sleep(Duration::from_millis(500));
            let _ = robot.wait_for_idle();

            let screenshot = robot
                .screenshot_with_scale(CAPTURE_SCALE)
                .unwrap_or_else(|err| fail(&robot, &format!("failed to capture screenshot: {err}")));
            let output_path =
                output_paths::diagnostic_path("cranpose_render_crispness_contract.png");
            if let Err(err) = save_png(&output_path, &screenshot) {
                fail(&robot, &err);
            }
            println!("SCREENSHOT_PATH={}", output_path.display());

            let mut failures = Vec::new();
            let atlas_crop = crop_screenshot_logical(
                &screenshot,
                ATLAS_X,
                ATLAS_Y,
                ATLAS_SIZE,
                ATLAS_SIZE,
            )
            .expect("atlas crop");
            let blue_leaks = blue_leak_pixels(&atlas_crop);
            if blue_leaks > 4 {
                failures.push(format!(
                    "atlas source rect sampled neighboring blue texels: blue_leaks={blue_leaks}"
                ));
            }

            let static_crop = crop_screenshot_logical(
                &screenshot,
                STATIC_X,
                STATIC_Y + SCROLL_COMPARE_TOP,
                BLOCK_W,
                SCROLL_COMPARE_H,
            )
                    .expect("static crop");
            let scrolled_crop = crop_screenshot_logical(
                &screenshot,
                SCROLLED_X,
                snapped_content_origin_y() + SCROLL_COMPARE_TOP,
                BLOCK_W,
                SCROLL_COMPARE_H,
            )
                    .expect("scrolled crop");
            let static_energy = edge_energy(&static_crop);
            let scrolled_energy = edge_energy(&scrolled_crop);
            let static_grid_crop = crop_screenshot_logical(
                &screenshot,
                STATIC_X,
                STATIC_Y + GRID_COMPARE_TOP,
                BLOCK_W,
                GRID_COMPARE_H,
            )
            .expect("static grid crop");
            let scrolled_grid_crop = crop_screenshot_logical(
                &screenshot,
                SCROLLED_X,
                snapped_content_origin_y() + GRID_COMPARE_TOP,
                BLOCK_W,
                GRID_COMPARE_H,
            )
            .expect("scrolled grid crop");
            let static_grid_colors = unique_rgb_count(&static_grid_crop);
            let scrolled_grid_colors = unique_rgb_count(&scrolled_grid_crop);
            println!(
                "metrics: atlas_blue_leaks={blue_leaks} static_edge_energy={static_energy:.2} scrolled_edge_energy={scrolled_energy:.2} static_grid_colors={static_grid_colors} scrolled_grid_colors={scrolled_grid_colors}"
            );
            if scrolled_energy < static_energy * 0.92 {
                failures.push(format!(
                    "1px rested scroll softened control/text edges: static_energy={static_energy:.2} scrolled_energy={scrolled_energy:.2}"
                ));
            }
            if scrolled_grid_colors > static_grid_colors + 8 {
                failures.push(format!(
                    "1px rested scroll blended crisp control pixels: static_grid_colors={static_grid_colors} scrolled_grid_colors={scrolled_grid_colors}"
                ));
            }

            if !failures.is_empty() {
                fail(&robot, &failures.join("; "));
            }

            println!(
                "PASS: atlas blue_leaks={blue_leaks}; scroll edge energy static={static_energy:.2} scrolled={scrolled_energy:.2}"
            );
            let _ = robot.exit();
        })
        .run(RenderCrispnessContractApp);
}
