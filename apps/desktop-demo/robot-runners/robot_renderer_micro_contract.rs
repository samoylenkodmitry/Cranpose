//! Robot micro-contract for deterministic renderer screenshot validation.
//!
//! Run with:
//! `cargo run --package desktop-app --example robot_renderer_micro_contract --features robot-app`

use cranpose::AppLauncher;
use cranpose_testing::{crop_screenshot_logical, sample_screenshot_pixel_logical};
use cranpose_ui::{
    composable, text::SpanStyle, text::TextDecoration, text::TextUnit, Alignment, BasicText,
    BitmapPainter, Box, BoxSpec, Color, ContentScale, Image, ImageBitmap, Modifier, Rect, Row,
    RowSpec, Size, Text, TextOverflow, TextStyle,
};
use image::{ImageBuffer, RgbaImage};
use std::path::Path;
use std::time::Duration;

const WINDOW_WIDTH: u32 = 180;
const WINDOW_HEIGHT: u32 = 170;
const SCREENSHOT_PATH: &str = "/tmp/cranpose_renderer_micro_contract.png";

const BACKGROUND_COLOR: Color = Color(0.12, 0.16, 0.24, 1.0);
const LINE_VERTICAL_COLOR: Color = Color(0.98, 0.98, 0.98, 1.0);
const LINE_HORIZONTAL_COLOR: Color = Color(0.98, 0.72, 0.30, 1.0);
const RECT_FILL_COLOR: Color = Color(0.30, 0.78, 0.56, 1.0);
const PANEL_FILL_COLOR: Color = Color(0.04, 0.05, 0.09, 1.0);
const CHESS_LIGHT: [u8; 3] = [240, 240, 240];
const CHESS_DARK: [u8; 3] = [36, 54, 72];
const COLOR_TOLERANCE: u8 = 18;

fn within_tolerance(actual: [u8; 3], expected: [u8; 3], tolerance: u8) -> bool {
    let tolerance = tolerance as i16;
    (actual[0] as i16 - expected[0] as i16).abs() <= tolerance
        && (actual[1] as i16 - expected[1] as i16).abs() <= tolerance
        && (actual[2] as i16 - expected[2] as i16).abs() <= tolerance
}

fn sample_rgb(screenshot: &cranpose::RobotScreenshot, x: f32, y: f32) -> Result<[u8; 3], String> {
    let rgba = sample_screenshot_pixel_logical(screenshot, x, y)
        .ok_or_else(|| format!("sample out of bounds at ({x:.1}, {y:.1})"))?;
    Ok([rgba[0], rgba[1], rgba[2]])
}

fn linear_channel_to_srgb_u8(channel: f32) -> u8 {
    let channel = channel.clamp(0.0, 1.0);
    let srgb = if channel <= 0.003_130_8 {
        channel * 12.92
    } else {
        1.055 * channel.powf(1.0 / 2.4) - 0.055
    };
    (srgb * 255.0).round().clamp(0.0, 255.0) as u8
}

fn expected_screenshot_rgb(color: Color) -> [u8; 3] {
    [
        linear_channel_to_srgb_u8(color.0),
        linear_channel_to_srgb_u8(color.1),
        linear_channel_to_srgb_u8(color.2),
    ]
}

fn assert_color(
    screenshot: &cranpose::RobotScreenshot,
    label: &str,
    x: f32,
    y: f32,
    expected: [u8; 3],
) -> Result<(), String> {
    let actual = sample_rgb(screenshot, x, y)?;
    if within_tolerance(actual, expected, COLOR_TOLERANCE) {
        return Ok(());
    }
    Err(format!(
        "{label}: expected {expected:?} at ({x:.1}, {y:.1}), got {actual:?}"
    ))
}

fn count_bright_pixels(screenshot: &cranpose::RobotScreenshot) -> usize {
    screenshot
        .pixels
        .chunks_exact(4)
        .filter(|rgba| rgba[0] > 170 || rgba[1] > 170 || rgba[2] > 170)
        .count()
}

fn count_green_text_pixels(screenshot: &cranpose::RobotScreenshot) -> usize {
    screenshot
        .pixels
        .chunks_exact(4)
        .filter(|rgba| rgba[1] > 150 && rgba[1] > rgba[0].saturating_add(24) && rgba[1] > rgba[2])
        .count()
}

fn count_yellow_text_pixels(screenshot: &cranpose::RobotScreenshot) -> usize {
    screenshot
        .pixels
        .chunks_exact(4)
        .filter(|rgba| rgba[0] > 170 && rgba[1] > 150 && rgba[2] < 140)
        .count()
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

fn fail(robot: &cranpose::Robot, message: &str) -> ! {
    println!("FATAL: {message}");
    let _ = robot.exit();
    std::process::exit(1);
}

fn generate_chessboard_bitmap(tile_size: u32, tiles_per_side: u32) -> ImageBitmap {
    let side = tile_size.max(1) * tiles_per_side.max(1);
    let mut pixels = vec![0u8; (side * side * 4) as usize];

    for y in 0..side {
        for x in 0..side {
            let tile_x = x / tile_size.max(1);
            let tile_y = y / tile_size.max(1);
            let color = if ((tile_x + tile_y) & 1) == 0 {
                [CHESS_LIGHT[0], CHESS_LIGHT[1], CHESS_LIGHT[2], 255]
            } else {
                [CHESS_DARK[0], CHESS_DARK[1], CHESS_DARK[2], 255]
            };
            let index = ((y * side + x) * 4) as usize;
            pixels[index..index + 4].copy_from_slice(&color);
        }
    }

    ImageBitmap::from_rgba8(side, side, pixels).expect("valid chessboard bitmap")
}

fn generate_icon_bitmap() -> ImageBitmap {
    const SIDE: u32 = 20;
    let mut pixels = vec![0u8; (SIDE * SIDE * 4) as usize];
    for y in 0..SIDE {
        for x in 0..SIDE {
            let rgba = if x < 3 || y < 3 || x >= SIDE - 3 || y >= SIDE - 3 {
                [255, 230, 48, 255]
            } else if (x + y) % 2 == 0 {
                [220, 45, 190, 255]
            } else {
                [36, 190, 255, 255]
            };
            let index = ((y * SIDE + x) * 4) as usize;
            pixels[index..index + 4].copy_from_slice(&rgba);
        }
    }
    ImageBitmap::from_rgba8(SIDE, SIDE, pixels).expect("valid icon bitmap")
}

fn row_text_style() -> TextStyle {
    TextStyle::from_span_style(SpanStyle {
        color: Some(Color::from_rgb_u8(180, 255, 190)),
        font_size: TextUnit::Sp(15.0),
        ..SpanStyle::default()
    })
}

fn panel_text_style() -> TextStyle {
    TextStyle::from_span_style(SpanStyle {
        color: Some(Color::from_rgb_u8(252, 228, 132)),
        font_size: TextUnit::Sp(13.0),
        ..SpanStyle::default()
    })
}

#[allow(non_snake_case)]
#[composable]
fn BitmapIconTextRow(icon: ImageBitmap, modifier: Modifier) {
    let style = row_text_style();
    Row(
        modifier.size(Size::new(148.0, 24.0)),
        RowSpec::default(),
        move || {
            Image(
                BitmapPainter(icon.clone()),
                Some("Bitmap icon".to_string()),
                Modifier::empty().size(Size::new(20.0, 20.0)),
                Alignment::CENTER,
                ContentScale::FillBounds,
                1.0,
                None,
            );
            BasicText(
                "Bitmap icon text",
                Modifier::empty().weight(1.0),
                style.clone(),
                TextOverflow::Clip,
                false,
                1,
                1,
            );
            BasicText(
                "OK",
                Modifier::empty(),
                style.clone(),
                TextOverflow::Clip,
                false,
                1,
                1,
            );
        },
    );
}

#[allow(non_snake_case)]
#[composable]
fn SourceIconTextRow(icon: ImageBitmap, modifier: Modifier) {
    let style = row_text_style();
    Row(
        modifier.size(Size::new(148.0, 24.0)),
        RowSpec::default(),
        move || {
            Box(
                Modifier::empty().size(Size::new(20.0, 20.0)).draw_behind({
                    let icon = icon.clone();
                    move |scope| {
                        scope.draw_image_src(
                            icon.clone(),
                            Rect {
                                x: 2.0,
                                y: 2.0,
                                width: 16.0,
                                height: 16.0,
                            },
                            Rect {
                                x: 0.0,
                                y: 0.0,
                                width: 20.0,
                                height: 20.0,
                            },
                            1.0,
                            None,
                        );
                    }
                }),
                BoxSpec::default(),
                || {},
            );
            BasicText(
                "Source icon",
                Modifier::empty(),
                style.clone(),
                TextOverflow::Clip,
                false,
                1,
                1,
            );
            BasicText(
                " weighted",
                Modifier::empty().weight(1.0),
                style.clone(),
                TextOverflow::Clip,
                false,
                1,
                1,
            );
        },
    );
}

#[allow(non_snake_case)]
#[composable]
fn RendererMicroContractApp() {
    let board = cranpose_core::remember(|| generate_chessboard_bitmap(8, 4)).with(|b| b.clone());
    let icon = cranpose_core::remember(generate_icon_bitmap).with(|bitmap| bitmap.clone());
    let underline_style = TextStyle::from_span_style(SpanStyle {
        color: Some(Color(0.97, 0.98, 1.0, 1.0)),
        font_size: TextUnit::Sp(18.0),
        text_decoration: Some(TextDecoration::UNDERLINE),
        ..SpanStyle::default()
    });

    Box(
        Modifier::empty()
            .size(Size::new(WINDOW_WIDTH as f32, WINDOW_HEIGHT as f32))
            .background(BACKGROUND_COLOR),
        BoxSpec::default().content_alignment(Alignment::TOP_START),
        move || {
            Box(
                Modifier::empty()
                    .offset(16.0, 14.0)
                    .size(Size::new(4.0, 24.0))
                    .background(LINE_VERTICAL_COLOR),
                BoxSpec::default(),
                || {},
            );
            Box(
                Modifier::empty()
                    .offset(24.0, 14.0)
                    .size(Size::new(24.0, 4.0))
                    .background(LINE_HORIZONTAL_COLOR),
                BoxSpec::default(),
                || {},
            );
            Image(
                BitmapPainter(board.clone()),
                Some("Micro chessboard".to_string()),
                Modifier::empty()
                    .offset(16.0, 42.0)
                    .size(Size::new(32.0, 32.0)),
                Alignment::CENTER,
                ContentScale::FillBounds,
                1.0,
                None,
            );
            Box(
                Modifier::empty()
                    .offset(60.0, 70.0)
                    .size(Size::new(26.0, 14.0))
                    .background(RECT_FILL_COLOR),
                BoxSpec::default(),
                || {},
            );
            Box(
                Modifier::empty()
                    .offset(96.0, 42.0)
                    .size(Size::new(68.0, 38.0))
                    .background(PANEL_FILL_COLOR)
                    .padding(6.0),
                BoxSpec::default().content_alignment(Alignment::TOP_START),
                || {
                    Text("PANEL", Modifier::empty(), panel_text_style());
                },
            );
            Text(
                "UNDER",
                Modifier::empty().offset(60.0, 24.0),
                underline_style.clone(),
            );
            BitmapIconTextRow(icon.clone(), Modifier::empty().offset(16.0, 100.0));
            SourceIconTextRow(icon.clone(), Modifier::empty().offset(16.0, 130.0));
        },
    );
}

fn main() {
    env_logger::init();
    println!("=== Robot Renderer Micro Contract ===");

    AppLauncher::new()
        .with_title("Robot Renderer Micro Contract")
        .with_size(WINDOW_WIDTH, WINDOW_HEIGHT)
        .with_fonts(desktop_app::fonts::DEMO_FONTS)
        .with_headless(true)
        .with_test_driver(|robot| {
            std::thread::sleep(Duration::from_millis(500));
            let _ = robot.wait_for_idle();

            let screenshot = robot
                .screenshot_with_scale(1.0)
                .unwrap_or_else(|err| fail(&robot, &format!("failed to capture screenshot: {err}")));
            if screenshot.width != WINDOW_WIDTH || screenshot.height != WINDOW_HEIGHT {
                fail(
                    &robot,
                    &format!(
                        "headless scale-1 screenshot dimensions drifted: expected={}x{} actual={}x{}",
                        WINDOW_WIDTH, WINDOW_HEIGHT, screenshot.width, screenshot.height
                    ),
                );
            }
            if (screenshot.logical_width - WINDOW_WIDTH as f32).abs() > f32::EPSILON
                || (screenshot.logical_height - WINDOW_HEIGHT as f32).abs() > f32::EPSILON
            {
                fail(
                    &robot,
                    &format!(
                        "headless screenshot logical size drifted: expected={}x{} actual={:.2}x{:.2}",
                        WINDOW_WIDTH,
                        WINDOW_HEIGHT,
                        screenshot.logical_width,
                        screenshot.logical_height
                    ),
                );
            }
            let output_path = Path::new(SCREENSHOT_PATH);
            if let Err(err) = save_png(output_path, &screenshot) {
                fail(&robot, &err);
            }
            println!("SCREENSHOT_PATH={}", output_path.display());

            let background = expected_screenshot_rgb(BACKGROUND_COLOR);
            let vertical_line = expected_screenshot_rgb(LINE_VERTICAL_COLOR);
            let horizontal_line = expected_screenshot_rgb(LINE_HORIZONTAL_COLOR);
            let fill_rect = expected_screenshot_rgb(RECT_FILL_COLOR);
            for (label, x, y, expected) in [
                ("background", 4.0, 4.0, background),
                ("vertical_line", 18.0, 20.0, vertical_line),
                ("vertical_left_bg", 13.0, 20.0, background),
                ("horizontal_line", 36.0, 16.0, horizontal_line),
                ("horizontal_above_bg", 36.0, 12.0, background),
                ("fill_rect", 72.0, 76.0, fill_rect),
                ("fill_rect_above_bg", 90.0, 66.0, background),
                ("nested_panel", 158.0, 74.0, expected_screenshot_rgb(PANEL_FILL_COLOR)),
                ("chess_0_0", 20.0, 46.0, CHESS_LIGHT),
                ("chess_1_0", 28.0, 46.0, CHESS_DARK),
                ("chess_0_1", 20.0, 54.0, CHESS_DARK),
                ("chess_1_1", 28.0, 54.0, CHESS_LIGHT),
            ] {
                if let Err(err) = assert_color(&screenshot, label, x, y, expected) {
                    fail(&robot, &err);
                }
            }

            let Some(underline_crop) =
                crop_screenshot_logical(&screenshot, 58.0, 22.0, 90.0, 24.0)
            else {
                fail(&robot, "failed to crop underlined text region");
            };
            let Some(underline_band) =
                crop_screenshot_logical(&screenshot, 58.0, 37.0, 90.0, 4.0)
            else {
                fail(&robot, "failed to crop underline band");
            };

            let underline_bright = count_bright_pixels(&underline_crop);
            let underline_band_bright = count_bright_pixels(&underline_band);
            if underline_bright < 150 {
                fail(
                    &robot,
                    &format!(
                        "underlined text region is too soft or missing: bright_pixels={underline_bright}"
                    ),
                );
            }
            if underline_band_bright < 22 {
                fail(
                    &robot,
                    &format!(
                        "underline band is too weak or misplaced: bright_pixels={underline_band_bright}"
                    ),
                );
            }
            let Some(bitmap_row_text) =
                crop_screenshot_logical(&screenshot, 38.0, 96.0, 134.0, 30.0)
            else {
                fail(&robot, "failed to crop bitmap icon text row");
            };
            let Some(source_row_text) =
                crop_screenshot_logical(&screenshot, 38.0, 126.0, 134.0, 30.0)
            else {
                fail(&robot, "failed to crop source icon text row");
            };
            let bitmap_green = count_green_text_pixels(&bitmap_row_text);
            let source_green = count_green_text_pixels(&source_row_text);
            let Some(panel_text) = crop_screenshot_logical(&screenshot, 100.0, 46.0, 60.0, 22.0)
            else {
                fail(&robot, "failed to crop nested panel text");
            };
            let panel_yellow = count_yellow_text_pixels(&panel_text);
            if bitmap_green < 35 {
                fail(
                    &robot,
                    &format!(
                        "text after Image(BitmapPainter) is missing or clipped: green_pixels={bitmap_green}"
                    ),
                );
            }
            if source_green < 35 {
                fail(
                    &robot,
                    &format!(
                        "text after draw_image_src is missing or clipped: green_pixels={source_green}"
                    ),
                );
            }
            if panel_yellow < 20 {
                fail(
                    &robot,
                    &format!(
                        "nested panel text after image layers is missing or clipped: yellow_pixels={panel_yellow}"
                    ),
                );
            }

            println!(
                "PASS: renderer micro-contract pixels match expected image/line/fill layout; underlined and post-image text crops are populated (underlined={}, underline_band={}, bitmap_text={}, source_text={}, panel_text={})",
                underline_bright, underline_band_bright, bitmap_green, source_green, panel_yellow
            );
            let _ = robot.exit();
        })
        .run(RendererMicroContractApp);
}
