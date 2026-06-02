use crate::output_paths;
use crate::text_showcase_external_helpers::{capture_x11_window, find_window_id};
use cranpose::AppLauncher;
use cranpose_ui::{
    composable, Alignment, Box, BoxSpec, Brush, Canvas, Color, Modifier, Rect, Size,
};
use cranpose_ui_graphics::DrawScope;
use image::RgbaImage;
use std::time::Duration;

const WINDOW_WIDTH: u32 = 360;
const WINDOW_HEIGHT: u32 = 240;
const ORIGIN_SIZE: u32 = 12;
const TOP_RIGHT_WIDTH: u32 = 17;
const TOP_RIGHT_HEIGHT: u32 = 19;
const BOTTOM_LEFT_WIDTH: u32 = 23;
const BOTTOM_LEFT_HEIGHT: u32 = 29;
const CENTER_WIDTH: u32 = 72;
const CENTER_HEIGHT: u32 = 48;
const CENTER_X: u32 = 144;
const CENTER_Y: u32 = 84;
const EDGE_TOLERANCE_PX: i32 = 3;
const SIZE_TOLERANCE_PX: i32 = 2;
const SCALE_AXIS_TOLERANCE: f32 = 0.08;

const ORIGIN_COLOR: [u8; 3] = [255, 0, 255];
const TOP_RIGHT_COLOR: [u8; 3] = [0, 255, 0];
const BOTTOM_LEFT_COLOR: [u8; 3] = [255, 0, 0];
const CENTER_COLOR: [u8; 3] = [255, 255, 255];

#[derive(Clone, Copy, Debug)]
struct PixelBounds {
    min_x: u32,
    min_y: u32,
    max_x: u32,
    max_y: u32,
}

#[derive(Clone, Copy, Debug)]
struct CaptureScale {
    x: f32,
    y: f32,
}

impl PixelBounds {
    fn width(self) -> u32 {
        self.max_x - self.min_x + 1
    }

    fn height(self) -> u32 {
        self.max_y - self.min_y + 1
    }
}

fn fail(robot: &cranpose::Robot, message: &str) -> ! {
    println!("FATAL: {message}");
    let _ = robot.exit();
    std::process::exit(1);
}

fn color_bounds(image: &RgbaImage, target: [u8; 3]) -> Option<PixelBounds> {
    let mut bounds: Option<PixelBounds> = None;
    for (x, y, pixel) in image.enumerate_pixels() {
        if !matches_color(pixel.0, target) {
            continue;
        }
        bounds = Some(match bounds {
            Some(existing) => PixelBounds {
                min_x: existing.min_x.min(x),
                min_y: existing.min_y.min(y),
                max_x: existing.max_x.max(x),
                max_y: existing.max_y.max(y),
            },
            None => PixelBounds {
                min_x: x,
                min_y: y,
                max_x: x,
                max_y: y,
            },
        });
    }
    bounds
}

fn matches_color(pixel: [u8; 4], target: [u8; 3]) -> bool {
    pixel[3] > 240
        && pixel[0].abs_diff(target[0]) <= 3
        && pixel[1].abs_diff(target[1]) <= 3
        && pixel[2].abs_diff(target[2]) <= 3
}

fn require_bounds(
    robot: &cranpose::Robot,
    image: &RgbaImage,
    name: &str,
    color: [u8; 3],
) -> PixelBounds {
    color_bounds(image, color).unwrap_or_else(|| {
        fail(
            robot,
            &format!("presented window marker '{name}' was not found"),
        )
    })
}

fn assert_close(robot: &cranpose::Robot, name: &str, actual: i32, expected: i32, tolerance: i32) {
    if (actual - expected).abs() > tolerance {
        fail(
            robot,
            &format!("{name}: actual={actual} expected={expected} tolerance={tolerance}"),
        );
    }
}

fn scaled_px(value: u32, scale: f32) -> i32 {
    (value as f32 * scale).round() as i32
}

fn capture_scale(robot: &cranpose::Robot, image: &RgbaImage) -> CaptureScale {
    let scale = CaptureScale {
        x: image.width() as f32 / WINDOW_WIDTH as f32,
        y: image.height() as f32 / WINDOW_HEIGHT as f32,
    };
    if !scale.x.is_finite()
        || !scale.y.is_finite()
        || scale.x < 0.5
        || scale.y < 0.5
        || (scale.x - scale.y).abs() > SCALE_AXIS_TOLERANCE
    {
        fail(
            robot,
            &format!(
                "presented capture has invalid scale: capture={}x{} logical={}x{} scale=({:.3}, {:.3})",
                image.width(),
                image.height(),
                WINDOW_WIDTH,
                WINDOW_HEIGHT,
                scale.x,
                scale.y
            ),
        );
    }
    scale
}

fn assert_marker_size(
    robot: &cranpose::Robot,
    scale: CaptureScale,
    name: &str,
    bounds: PixelBounds,
    expected_width: u32,
    expected_height: u32,
) {
    assert_close(
        robot,
        &format!("{name} width"),
        bounds.width() as i32,
        scaled_px(expected_width, scale.x),
        SIZE_TOLERANCE_PX,
    );
    assert_close(
        robot,
        &format!("{name} height"),
        bounds.height() as i32,
        scaled_px(expected_height, scale.y),
        SIZE_TOLERANCE_PX,
    );
}

fn draw_geometry_probe(scope: &mut dyn DrawScope) {
    scope.draw_rect(Brush::solid(Color::from_rgb_u8(12, 18, 26)));
    scope.draw_rect_at(
        Rect {
            x: 0.0,
            y: 0.0,
            width: ORIGIN_SIZE as f32,
            height: ORIGIN_SIZE as f32,
        },
        Brush::solid(Color::from_rgb_u8(
            ORIGIN_COLOR[0],
            ORIGIN_COLOR[1],
            ORIGIN_COLOR[2],
        )),
    );
    scope.draw_rect_at(
        Rect {
            x: (WINDOW_WIDTH - TOP_RIGHT_WIDTH) as f32,
            y: 0.0,
            width: TOP_RIGHT_WIDTH as f32,
            height: TOP_RIGHT_HEIGHT as f32,
        },
        Brush::solid(Color::from_rgb_u8(
            TOP_RIGHT_COLOR[0],
            TOP_RIGHT_COLOR[1],
            TOP_RIGHT_COLOR[2],
        )),
    );
    scope.draw_rect_at(
        Rect {
            x: 0.0,
            y: (WINDOW_HEIGHT - BOTTOM_LEFT_HEIGHT) as f32,
            width: BOTTOM_LEFT_WIDTH as f32,
            height: BOTTOM_LEFT_HEIGHT as f32,
        },
        Brush::solid(Color::from_rgb_u8(
            BOTTOM_LEFT_COLOR[0],
            BOTTOM_LEFT_COLOR[1],
            BOTTOM_LEFT_COLOR[2],
        )),
    );
    scope.draw_rect_at(
        Rect {
            x: CENTER_X as f32,
            y: CENTER_Y as f32,
            width: CENTER_WIDTH as f32,
            height: CENTER_HEIGHT as f32,
        },
        Brush::solid(Color::from_rgb_u8(
            CENTER_COLOR[0],
            CENTER_COLOR[1],
            CENTER_COLOR[2],
        )),
    );
}

#[allow(non_snake_case)]
#[composable]
fn PresentedWindowGeometryApp() {
    Box(
        Modifier::empty().size(Size::new(WINDOW_WIDTH as f32, WINDOW_HEIGHT as f32)),
        BoxSpec::default().content_alignment(Alignment::TOP_START),
        || {
            Canvas(
                Modifier::empty().size(Size::new(WINDOW_WIDTH as f32, WINDOW_HEIGHT as f32)),
                draw_geometry_probe,
            );
        },
    );
}

pub fn run(window_title: &'static str) {
    env_logger::init();
    println!("=== {window_title} ===");

    AppLauncher::new()
        .with_title(window_title)
        .with_size(WINDOW_WIDTH, WINDOW_HEIGHT)
        .with_headless(false)
        .with_test_driver(move |robot| {
            std::thread::sleep(Duration::from_millis(500));
            robot.wait_for_idle().expect("initial idle");

            let window_id = find_window_id(window_title);
            let capture_path = output_paths::diagnostic_path(&format!(
                "cranpose-presented-window-geometry-{}.png",
                std::process::id()
            ));
            let image = capture_x11_window(&window_id, &capture_path);
            println!("SCREENSHOT_PATH={}", capture_path.display());
            let scale = capture_scale(&robot, &image);

            let origin = require_bounds(&robot, &image, "origin", ORIGIN_COLOR);
            let top_right = require_bounds(&robot, &image, "top-right", TOP_RIGHT_COLOR);
            let bottom_left = require_bounds(&robot, &image, "bottom-left", BOTTOM_LEFT_COLOR);
            let center = require_bounds(&robot, &image, "center", CENTER_COLOR);

            assert_marker_size(&robot, scale, "origin", origin, ORIGIN_SIZE, ORIGIN_SIZE);
            assert_marker_size(
                &robot,
                scale,
                "top-right",
                top_right,
                TOP_RIGHT_WIDTH,
                TOP_RIGHT_HEIGHT,
            );
            assert_marker_size(
                &robot,
                scale,
                "bottom-left",
                bottom_left,
                BOTTOM_LEFT_WIDTH,
                BOTTOM_LEFT_HEIGHT,
            );
            assert_marker_size(&robot, scale, "center", center, CENTER_WIDTH, CENTER_HEIGHT);

            assert_close(
                &robot,
                "origin marker left edge",
                origin.min_x as i32,
                0,
                EDGE_TOLERANCE_PX,
            );
            assert_close(
                &robot,
                "origin marker top edge",
                origin.min_y as i32,
                0,
                EDGE_TOLERANCE_PX,
            );
            assert_close(
                &robot,
                "top-right marker right edge",
                top_right.max_x as i32,
                image.width() as i32 - 1,
                EDGE_TOLERANCE_PX,
            );
            assert_close(
                &robot,
                "bottom-left marker bottom edge",
                bottom_left.max_y as i32,
                image.height() as i32 - 1,
                EDGE_TOLERANCE_PX,
            );
            assert_close(
                &robot,
                "top-right marker x offset",
                top_right.min_x as i32 - origin.min_x as i32,
                scaled_px(WINDOW_WIDTH - TOP_RIGHT_WIDTH, scale.x),
                EDGE_TOLERANCE_PX,
            );
            assert_close(
                &robot,
                "bottom-left marker y offset",
                bottom_left.min_y as i32 - origin.min_y as i32,
                scaled_px(WINDOW_HEIGHT - BOTTOM_LEFT_HEIGHT, scale.y),
                EDGE_TOLERANCE_PX,
            );
            assert_close(
                &robot,
                "center marker x offset",
                center.min_x as i32 - origin.min_x as i32,
                scaled_px(CENTER_X, scale.x),
                EDGE_TOLERANCE_PX,
            );
            assert_close(
                &robot,
                "center marker y offset",
                center.min_y as i32 - origin.min_y as i32,
                scaled_px(CENTER_Y, scale.y),
                EDGE_TOLERANCE_PX,
            );

            println!(
                "PASS: presented geometry capture={}x{} scale=({:.3},{:.3}) origin={origin:?} top_right={top_right:?} bottom_left={bottom_left:?} center={center:?}",
                image.width(),
                image.height(),
                scale.x,
                scale.y
            );
            robot.exit().expect("exit");
        })
        .run(PresentedWindowGeometryApp);
}
