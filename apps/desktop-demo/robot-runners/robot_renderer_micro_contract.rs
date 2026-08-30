mod output_paths;
mod robot_exit;
mod text_showcase_external_helpers;

use std::{path::Path, time::Duration};

use cranpose::AppLauncher;
use cranpose_testing::{crop_screenshot_logical, sample_screenshot_pixel_logical};
use cranpose_ui::{
    composable,
    text::{SpanStyle, TextDecoration, TextUnit},
    Alignment, BasicText, BitmapPainter, Box, BoxSpec, Color, ContentScale, Image, ImageBitmap,
    Modifier, Rect, Row, RowSpec, Size, Spacer, Text, TextOptions, TextOverflow, TextStyle,
    TextWithOptions,
};
use image::{ImageBuffer, RgbaImage};
use text_showcase_external_helpers::{capture_x11_window_screenshot, find_window_id};

const WINDOW_TITLE: &str = "Robot Renderer Micro Contract";
const WINDOW_WIDTH: u32 = 180;
const WINDOW_HEIGHT: u32 = 205;

const BACKGROUND_COLOR: Color = Color(0.12, 0.16, 0.24, 1.0);
const LINE_VERTICAL_COLOR: Color = Color(0.98, 0.98, 0.98, 1.0);
const LINE_HORIZONTAL_COLOR: Color = Color(0.98, 0.72, 0.30, 1.0);
const RECT_FILL_COLOR: Color = Color(0.30, 0.78, 0.56, 1.0);
const PANEL_FILL_COLOR: Color = Color(0.04, 0.05, 0.09, 1.0);
const BUTTON_FILL_COLOR: Color = Color(0.08, 0.10, 0.14, 1.0);
const BADGE_FILL_COLOR: Color = Color(0.18, 0.38, 0.92, 1.0);
const TOP_RIGHT_SENTINEL_COLOR: Color = Color(1.0, 0.0, 0.0, 1.0);
const BOTTOM_LEFT_SENTINEL_COLOR: Color = Color(0.0, 1.0, 0.0, 1.0);
const BOTTOM_RIGHT_SENTINEL_COLOR: Color = Color(0.0, 0.0, 1.0, 1.0);
const CHESS_LIGHT: [u8; 3] = [240, 240, 240];
const CHESS_DARK: [u8; 3] = [36, 54, 72];
const COLOR_TOLERANCE: u8 = 18;
const EDGE_SENTINEL_SIZE: f32 = 6.0;

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

fn expected_screenshot_rgb(color: Color) -> [u8; 3] {
    [
        (color.0.clamp(0.0, 1.0) * 255.0).round() as u8,
        (color.1.clamp(0.0, 1.0) * 255.0).round() as u8,
        (color.2.clamp(0.0, 1.0) * 255.0).round() as u8,
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
        .as_chunks::<4>()
        .0
        .iter()
        .filter(|rgba| rgba[0] > 170 || rgba[1] > 170 || rgba[2] > 170)
        .count()
}

fn count_green_text_pixels(screenshot: &cranpose::RobotScreenshot) -> usize {
    screenshot
        .pixels
        .as_chunks::<4>()
        .0
        .iter()
        .filter(|rgba| rgba[1] > 150 && rgba[1] > rgba[0].saturating_add(24) && rgba[1] > rgba[2])
        .count()
}

fn count_yellow_text_pixels(screenshot: &cranpose::RobotScreenshot) -> usize {
    screenshot
        .pixels
        .as_chunks::<4>()
        .0
        .iter()
        .filter(|rgba| {
            rgba[0] > 150
                && rgba[1] > 130
                && rgba[0] > rgba[2].saturating_add(20)
                && rgba[1] > rgba[2].saturating_add(12)
        })
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

fn normalize_micro_contract_logical_size(
    mut screenshot: cranpose::RobotScreenshot,
) -> cranpose::RobotScreenshot {
    screenshot.logical_width = WINDOW_WIDTH as f32;
    screenshot.logical_height = WINDOW_HEIGHT as f32;
    screenshot
}

struct MicroContractMetrics {
    underline_bright: usize,
    underline_band_bright: usize,
    bitmap_green: usize,
    source_green: usize,
    panel_yellow: usize,
    compact_green: usize,
    compact_gap_green: usize,
}

impl MicroContractMetrics {
    fn summary(&self) -> String {
        format!(
            "underlined={}, underline_band={}, bitmap_text={}, source_text={}, panel_text={}, compact_text={}, compact_gap={}",
            self.underline_bright,
            self.underline_band_bright,
            self.bitmap_green,
            self.source_green,
            self.panel_yellow,
            self.compact_green,
            self.compact_gap_green
        )
    }
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

fn compact_text_style() -> TextStyle {
    TextStyle::from_span_style(SpanStyle {
        color: Some(Color::from_rgb_u8(180, 255, 190)),
        font_size: TextUnit::Sp(18.0),
        ..SpanStyle::default()
    })
}

fn badge_text_style() -> TextStyle {
    TextStyle::from_span_style(SpanStyle {
        color: Some(Color::from_rgb_u8(255, 255, 255)),
        font_size: TextUnit::Sp(10.0),
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
fn CompactOverflowButton(icon: ImageBitmap, modifier: Modifier) {
    Row(
        modifier
            .size(Size::new(148.0, 28.0))
            .background(BUTTON_FILL_COLOR)
            .padding(2.0),
        RowSpec::default(),
        move || {
            Image(
                BitmapPainter(icon.clone()),
                Some("Compact icon".to_string()),
                Modifier::empty().size(Size::new(18.0, 18.0)),
                Alignment::CENTER,
                ContentScale::FillBounds,
                1.0,
                None,
            );
            TextWithOptions(
                "Save Cranpose WebP",
                Modifier::empty().weight(1.0),
                compact_text_style(),
                TextOptions {
                    overflow: TextOverflow::ScaleDown {
                        min_font_size_sp: 9.0,
                    },
                    soft_wrap: false,
                    max_lines: Some(1),
                    ..TextOptions::default()
                },
            );
            Spacer(Size::new(8.0, 1.0));
            Box(
                Modifier::empty()
                    .size(Size::new(24.0, 18.0))
                    .background(BADGE_FILL_COLOR),
                BoxSpec::default().content_alignment(Alignment::CENTER),
                || {
                    Text("42", Modifier::empty(), badge_text_style());
                },
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
            CompactOverflowButton(icon.clone(), Modifier::empty().offset(16.0, 160.0));
            Box(
                Modifier::empty()
                    .offset(WINDOW_WIDTH as f32 - EDGE_SENTINEL_SIZE, 0.0)
                    .size(Size::new(EDGE_SENTINEL_SIZE, EDGE_SENTINEL_SIZE))
                    .background(TOP_RIGHT_SENTINEL_COLOR),
                BoxSpec::default(),
                || {},
            );
            Box(
                Modifier::empty()
                    .offset(0.0, WINDOW_HEIGHT as f32 - EDGE_SENTINEL_SIZE)
                    .size(Size::new(EDGE_SENTINEL_SIZE, EDGE_SENTINEL_SIZE))
                    .background(BOTTOM_LEFT_SENTINEL_COLOR),
                BoxSpec::default(),
                || {},
            );
            Box(
                Modifier::empty()
                    .offset(
                        WINDOW_WIDTH as f32 - EDGE_SENTINEL_SIZE,
                        WINDOW_HEIGHT as f32 - EDGE_SENTINEL_SIZE,
                    )
                    .size(Size::new(EDGE_SENTINEL_SIZE, EDGE_SENTINEL_SIZE))
                    .background(BOTTOM_RIGHT_SENTINEL_COLOR),
                BoxSpec::default(),
                || {},
            );
        },
    );
}

fn assert_micro_contract_pixels(
    robot: &cranpose::Robot,
    screenshot: &cranpose::RobotScreenshot,
    capture_label: &str,
) -> MicroContractMetrics {
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
        (
            "nested_panel",
            158.0,
            74.0,
            expected_screenshot_rgb(PANEL_FILL_COLOR),
        ),
        ("chess_0_0", 20.0, 46.0, CHESS_LIGHT),
        ("chess_1_0", 28.0, 46.0, CHESS_DARK),
        ("chess_0_1", 20.0, 54.0, CHESS_DARK),
        ("chess_1_1", 28.0, 54.0, CHESS_LIGHT),
        (
            "compact_button",
            24.0,
            184.0,
            expected_screenshot_rgb(BUTTON_FILL_COLOR),
        ),
        (
            "compact_badge",
            143.0,
            174.0,
            expected_screenshot_rgb(BADGE_FILL_COLOR),
        ),
        (
            "top_right_scale_sentinel",
            WINDOW_WIDTH as f32 - EDGE_SENTINEL_SIZE * 0.5,
            EDGE_SENTINEL_SIZE * 0.5,
            expected_screenshot_rgb(TOP_RIGHT_SENTINEL_COLOR),
        ),
        (
            "bottom_left_scale_sentinel",
            EDGE_SENTINEL_SIZE * 0.5,
            WINDOW_HEIGHT as f32 - EDGE_SENTINEL_SIZE * 0.5,
            expected_screenshot_rgb(BOTTOM_LEFT_SENTINEL_COLOR),
        ),
        (
            "bottom_right_scale_sentinel",
            WINDOW_WIDTH as f32 - EDGE_SENTINEL_SIZE * 0.5,
            WINDOW_HEIGHT as f32 - EDGE_SENTINEL_SIZE * 0.5,
            expected_screenshot_rgb(BOTTOM_RIGHT_SENTINEL_COLOR),
        ),
    ] {
        if let Err(err) = assert_color(screenshot, label, x, y, expected) {
            robot_exit::fail(robot, &format!("{capture_label} {err}"));
        }
    }

    let Some(underline_crop) = crop_screenshot_logical(screenshot, 58.0, 22.0, 90.0, 24.0) else {
        robot_exit::fail(
            robot,
            &format!("{capture_label} failed to crop underlined text region"),
        );
    };
    let Some(underline_band) = crop_screenshot_logical(screenshot, 58.0, 37.0, 90.0, 4.0) else {
        robot_exit::fail(
            robot,
            &format!("{capture_label} failed to crop underline band"),
        );
    };

    let underline_bright = count_bright_pixels(&underline_crop);
    let underline_band_bright = count_bright_pixels(&underline_band);
    if underline_bright < 150 {
        robot_exit::fail(
            robot,
            &format!(
                "{capture_label} underlined text region is too soft or missing: bright_pixels={underline_bright}"
            ));
    }
    if underline_band_bright < 22 {
        robot_exit::fail(
            robot,
            &format!(
                "{capture_label} underline band is too weak or misplaced: bright_pixels={underline_band_bright}"
            ));
    }

    let Some(bitmap_row_text) = crop_screenshot_logical(screenshot, 38.0, 96.0, 134.0, 30.0) else {
        robot_exit::fail(
            robot,
            &format!("{capture_label} failed to crop bitmap icon text row"),
        );
    };
    let Some(source_row_text) = crop_screenshot_logical(screenshot, 38.0, 126.0, 134.0, 30.0)
    else {
        robot_exit::fail(
            robot,
            &format!("{capture_label} failed to crop source icon text row"),
        );
    };
    let bitmap_green = count_green_text_pixels(&bitmap_row_text);
    let source_green = count_green_text_pixels(&source_row_text);
    let Some(panel_text) = crop_screenshot_logical(screenshot, 100.0, 46.0, 60.0, 22.0) else {
        robot_exit::fail(
            robot,
            &format!("{capture_label} failed to crop nested panel text"),
        );
    };
    let panel_yellow = count_yellow_text_pixels(&panel_text);
    if bitmap_green < 35 {
        robot_exit::fail(
            robot,
            &format!(
                "{capture_label} text after Image(BitmapPainter) is missing or clipped: green_pixels={bitmap_green}"
            ));
    }
    if source_green < 35 {
        robot_exit::fail(
            robot,
            &format!(
                "{capture_label} text after draw_image_src is missing or clipped: green_pixels={source_green}"
            ));
    }
    if panel_yellow < 20 {
        robot_exit::fail(
            robot,
            &format!(
                "{capture_label} nested panel text after image layers is missing or clipped: yellow_pixels={panel_yellow}"
            ));
    }

    let Some(compact_text) = crop_screenshot_logical(screenshot, 36.0, 156.0, 94.0, 34.0) else {
        robot_exit::fail(
            robot,
            &format!("{capture_label} failed to crop compact text row"),
        );
    };
    let Some(compact_gap) = crop_screenshot_logical(screenshot, 135.0, 158.0, 4.0, 28.0) else {
        robot_exit::fail(
            robot,
            &format!("{capture_label} failed to crop compact text gap"),
        );
    };
    let compact_green = count_green_text_pixels(&compact_text);
    let compact_gap_green = count_green_text_pixels(&compact_gap);
    if compact_green < 18 {
        robot_exit::fail(
            robot,
            &format!(
                "{capture_label} scaled compact text is missing or over-clipped: green_pixels={compact_green}"
            ));
    }
    if compact_gap_green != 0 {
        robot_exit::fail(
            robot,
            &format!(
                "{capture_label} scaled compact text drew outside its bounds into badge gap: green_pixels={compact_gap_green}"
            ));
    }

    MicroContractMetrics {
        underline_bright,
        underline_band_bright,
        bitmap_green,
        source_green,
        panel_yellow,
        compact_green,
        compact_gap_green,
    }
}

fn main() {
    env_logger::init();
    println!("=== Robot Renderer Micro Contract ===");

    AppLauncher::new()
        .with_title(WINDOW_TITLE)
        .with_size(WINDOW_WIDTH, WINDOW_HEIGHT)
        .with_fonts(desktop_app::fonts::DEMO_FONTS)
        .with_headless(false)
        .with_test_driver(|robot| {
            std::thread::sleep(Duration::from_millis(500));
            let _ = robot.wait_for_idle();

            let screenshot = robot
                .screenshot_with_scale(1.0)
                .unwrap_or_else(|err| robot_exit::fail(&robot, &format!("failed to capture screenshot: {err}")));
            if screenshot.width < WINDOW_WIDTH || screenshot.height < WINDOW_HEIGHT {
                robot_exit::fail(
                    &robot,
                    &format!(
                        "renderer readback scale-1 screenshot is smaller than the requested app surface: expected_at_least={}x{} actual={}x{}",
                        WINDOW_WIDTH, WINDOW_HEIGHT, screenshot.width, screenshot.height
                    ));
            }
            let screenshot = normalize_micro_contract_logical_size(screenshot);
            let output_path = output_paths::diagnostic_path("cranpose_renderer_micro_contract.png");
            if let Err(err) = save_png(&output_path, &screenshot) {
                robot_exit::fail(&robot, &err);
            }
            println!("SCREENSHOT_PATH={}", output_path.display());
            println!(
                "readback_capture_pixels={}x{} logical={:.1}x{:.1}",
                screenshot.width, screenshot.height, screenshot.logical_width, screenshot.logical_height
            );

            let readback_metrics = assert_micro_contract_pixels(&robot, &screenshot, "readback");

            std::thread::sleep(Duration::from_millis(200));
            let _ = robot.wait_for_idle();
            let window_id = find_window_id(WINDOW_TITLE);
            let presented_path =
                output_paths::diagnostic_path("cranpose_renderer_micro_contract_presented.png");
            let presented = capture_x11_window_screenshot(
                &window_id,
                &presented_path,
                WINDOW_WIDTH as f32,
                WINDOW_HEIGHT as f32,
            );
            println!("PRESENTED_SCREENSHOT_PATH={}", presented_path.display());
            println!(
                "presented_capture_pixels={}x{} logical={:.1}x{:.1}",
                presented.width,
                presented.height,
                presented.logical_width,
                presented.logical_height
            );
            let presented_metrics = assert_micro_contract_pixels(&robot, &presented, "presented");

            println!(
                "PASS: renderer micro-contract pixels match in readback and presented X11 capture; readback=({}); presented=({})",
                readback_metrics.summary(),
                presented_metrics.summary()
            );
            let _ = robot.exit();
        })
        .run(RendererMicroContractApp);
}
