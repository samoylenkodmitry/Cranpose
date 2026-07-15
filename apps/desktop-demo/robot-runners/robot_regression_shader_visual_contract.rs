//! E2E visual contracts for Shaders tab regressions.

mod output_paths;
mod text_showcase_external_helpers;
mod visual_contract_metrics;

use cranpose::AppLauncher;
use cranpose_testing::{
    crop_screenshot_logical, find_button_in_semantics, find_text_in_semantics,
    scroll_text_into_view, set_slider_fraction, ScrollConfig,
};
use desktop_app::app::{self, DemoTab, ShaderSection, StartupSelection};
use image::RgbaImage;
use std::path::Path;
use std::time::Duration;
use text_showcase_external_helpers::{
    capture_x11_window, find_window_id, focus_x11_window, move_x11_mouse_in_window,
};
use visual_contract_metrics::{
    crop_rgba, edge_energy_rgba, edge_energy_screenshot, feature_stats_rgba,
    feature_stats_screenshot,
};

const INTERACTIVE_TITLE: &str = "Robot Shader Overlap Regression Contract";
const SCROLLBAR_TITLE: &str = "Robot Shader No Implicit Scrollbar Contract";
const SHADOW_TITLE: &str = "Robot Shader Shadow Regression Contract";
const DSTOUT_TITLE: &str = "Robot Shader DstOut Regression Contract";
const WINDOW_WIDTH: u32 = 1200;
const WINDOW_HEIGHT: u32 = 900;

fn fail(_robot: &cranpose::Robot, message: &str) -> ! {
    println!("FATAL: {message}");
    std::process::exit(1);
}

fn center((x, y, w, h): (f32, f32, f32, f32)) -> (f32, f32) {
    (x + w * 0.5, y + h * 0.5)
}

fn save_robot_screenshot(path: &Path, screenshot: &cranpose::RobotScreenshot) {
    let Some(image) = RgbaImage::from_raw(
        screenshot.width,
        screenshot.height,
        screenshot.pixels.clone(),
    ) else {
        println!(
            "failed to construct screenshot image for {}",
            path.display()
        );
        return;
    };
    if let Err(error) = image.save(path) {
        println!("failed to save {}: {error}", path.display());
    }
}

fn main() {
    env_logger::init();
    let mode = std::env::var("CRANPOSE_SHADER_VISUAL_MODE")
        .ok()
        .unwrap_or_else(|| "all".to_string());
    if mode == "interactive" || mode == "all" {
        run_interactive_overlap();
    }
    if mode == "scrollbar" || mode == "all" {
        run_shader_scrollbar();
    }
    if mode == "shadow" || mode == "all" {
        run_shadow_showcases();
    }
    if mode == "dstout" || mode == "all" {
        run_dstout_alpha();
    }
}

fn run_interactive_overlap() {
    println!("=== Shader Interactive Overlap Contract ===");
    AppLauncher::new()
        .with_title(INTERACTIVE_TITLE)
        .with_size(WINDOW_WIDTH, WINDOW_HEIGHT)
        .with_headless(false)
        .with_test_driver(|robot| {
            std::thread::sleep(Duration::from_millis(900));
            let _ = robot.wait_for_idle();

            let window_id = find_window_id(INTERACTIVE_TITLE);
            focus_x11_window(&window_id);
            wait_for_text(&robot, "Interactive Effects (drag the rects!)");

            let blur = find_text_in_semantics(&robot, "Blur")
                .unwrap_or_else(|| fail(&robot, "Blur label missing"));
            let glass = find_text_in_semantics(&robot, "Glass")
                .unwrap_or_else(|| fail(&robot, "Glass label missing"));
            let (blur_cx, blur_cy) = center(blur);
            let (glass_cx, glass_cy) = center(glass);
            let blur_start = (blur_cx + 16.0, blur_cy + 16.0);
            let glass_start = (glass_cx + 244.0, glass_cy + 164.0);
            let glass_target = (glass_start.0 - 122.0, glass_start.1 - 104.0);
            let blur_target = (glass_target.0 + 60.0, glass_target.1 + 40.0);

            drag_window(&window_id, glass_start, glass_target);
            std::thread::sleep(Duration::from_millis(160));
            let _ = robot.wait_for_idle();
            drag_window(&window_id, blur_start, blur_target);
            std::thread::sleep(Duration::from_millis(220));
            let _ = robot.wait_for_idle();

            let path = output_paths::diagnostic_path("shader_overlap_glass_blur.png");
            let image = capture_x11_window(&window_id, &path);
            let glass_crop = (
                (glass_target.0 - 70.0).max(0.0) as u32,
                (glass_target.1 - 50.0).max(0.0) as u32,
                150,
                110,
            );
            let left_half = crop_rgba(&image, (glass_crop.0, glass_crop.1, 75, glass_crop.3));
            let right_half = crop_rgba(
                &image,
                (glass_crop.0 + 75, glass_crop.1, glass_crop.2 - 75, glass_crop.3),
            );
            let left_edge = edge_energy_rgba(&left_half);
            let right_edge = edge_energy_rgba(&right_half);
            let blur_label_pixels = feature_stats_rgba(&image, glass_crop, is_bright_label_pixel)
                .map(|stats| stats.count)
                .unwrap_or(0);
            let left_blue_blur_pixels = feature_stats_rgba(
                &image,
                (glass_crop.0, glass_crop.1, 75, glass_crop.3),
                is_blue_blur_pixel,
            )
            .map(|stats| stats.count)
            .unwrap_or(0);
            let right_blue_blur_pixels = feature_stats_rgba(
                &image,
                (glass_crop.0 + 75, glass_crop.1, glass_crop.2 - 75, glass_crop.3),
                is_blue_blur_pixel,
            )
            .map(|stats| stats.count)
            .unwrap_or(0);
            println!(
                "shader_overlap crop={glass_crop:?} left_edge={left_edge:.3} right_edge={right_edge:.3} bright_label_pixels={blur_label_pixels} left_blue_blur_pixels={left_blue_blur_pixels} right_blue_blur_pixels={right_blue_blur_pixels} screenshot={}",
                path.display()
            );
            if blur_label_pixels < 18
                || right_blue_blur_pixels < 500
                || right_blue_blur_pixels < left_blue_blur_pixels.saturating_mul(3)
                || right_edge < 0.65 * left_edge.max(1.0)
            {
                fail(
                    &robot,
                    "glass/blur overlap lost backdrop detail in the right half of the glass rect",
                );
            }

            println!("PASS: glass/blur overlap keeps visible backdrop detail");
            robot.exit().expect("exit");
        })
        .run(|| {
            app::combined_app_with_startup(StartupSelection {
                initial_tab: Some(DemoTab::Shaders),
                initial_shader_section: Some(ShaderSection::InteractiveEffects),
            });
        });
}

fn run_shader_scrollbar() {
    println!("=== Shader No Implicit Scrollbar Contract ===");
    AppLauncher::new()
        .with_title(SCROLLBAR_TITLE)
        .with_size(WINDOW_WIDTH, WINDOW_HEIGHT)
        .with_headless(false)
        .with_test_driver(|robot| {
            std::thread::sleep(Duration::from_millis(900));
            let _ = robot.wait_for_idle();
            select_shaders_tab(&robot);

            let before = robot.screenshot().expect("shader before screenshot");
            assert_no_implicit_scrollbar(&robot, "before scroll", &before);
            robot
                .mouse_move(WINDOW_WIDTH as f32 - 46.0, WINDOW_HEIGHT as f32 * 0.5)
                .expect("move to shader scrollbar area");
            robot.mouse_scroll(0.0, -520.0).expect("scroll shaders tab");
            std::thread::sleep(Duration::from_millis(220));
            let _ = robot.wait_for_idle();

            let after = robot.screenshot().expect("shader after screenshot");
            assert_no_implicit_scrollbar(&robot, "after scroll", &after);

            println!("PASS: plain shader scroll container draws no implicit scrollbar chrome");
            robot.exit().expect("exit");
        })
        .run(app::combined_app);
}

fn run_shadow_showcases() {
    println!("=== Shader Shadow Showcase Contract ===");
    AppLauncher::new()
        .with_title(SHADOW_TITLE)
        .with_size(WINDOW_WIDTH, WINDOW_HEIGHT)
        .with_headless(true)
        .with_test_driver(|robot| {
            std::thread::sleep(Duration::from_millis(700));
            let _ = robot.wait_for_idle();
            wait_for_text(&robot, "GraphicsLayer Fields");

            if scroll_text_into_view(
                &robot,
                "Shadow Fields",
                20,
                shader_scroll_config(),
            )
            .is_none()
            {
                fail(&robot, "Shadow Fields section not visible");
            }
            let _ = set_slider_fraction(&robot, "shadow_elevation", 1.0, 196.0, 9.0, shader_scroll_config());
            let _ = set_slider_fraction(&robot, "ambient_alpha", 1.0, 196.0, 9.0, shader_scroll_config());
            let _ = set_slider_fraction(&robot, "spot_alpha", 1.0, 196.0, 9.0, shader_scroll_config());
            std::thread::sleep(Duration::from_millis(160));
            let shadow_label = scroll_text_into_view(
                &robot,
                "shadow",
                12,
                shader_scroll_config(),
            )
            .unwrap_or_else(|| fail(&robot, "Shadow preview label not visible"));
            let screenshot = robot.screenshot().expect("shadow screenshot");
            let shadow_path = output_paths::diagnostic_path("shader_shadow_fields.png");
            save_robot_screenshot(&shadow_path, &screenshot);
            let probe = (
                (shadow_label.0 - 36.0).max(0.0),
                (shadow_label.1 - 92.0).max(0.0),
                230.0,
                118.0,
            );
            let shadow_pixels = feature_stats_screenshot(&screenshot, probe, is_shadow_pixel)
                .map(|stats| stats.count)
                .unwrap_or(0);
            println!(
                "shadow_fields probe={probe:?} shadow_pixels={shadow_pixels} screenshot={}",
                shadow_path.display()
            );
            if shadow_pixels < 120 {
                fail(
                    &robot,
                    &format!("Shadow Fields preview is missing shadow pixels: count={shadow_pixels}"),
                );
            }

            scroll_text_into_view(
                &robot,
                "Compose 1.9 Shadow API",
                24,
                shader_scroll_config(),
            )
            .unwrap_or_else(|| fail(&robot, "Compose 1.9 Shadow API section not visible"));
            let screenshot = robot.screenshot().expect("compose shadow screenshot");
            let compose_path = output_paths::diagnostic_path("shader_compose_shadow.png");
            save_robot_screenshot(&compose_path, &screenshot);
            let drop_label = find_text_in_semantics(&robot, "drop")
                .unwrap_or_else(|| fail(&robot, "drop shadow preview label not visible"));
            let drop_box_left = drop_label.0 - 10.0;
            let drop_box_top = drop_label.1 - 50.0;
            let compose_probe = (
                (drop_box_left + 34.0).max(0.0),
                (drop_box_top + 28.0).max(0.0),
                72.0,
                58.0,
            );
            let compose_shadow_pixels = feature_stats_screenshot(
                &screenshot,
                compose_probe,
                is_soft_shadow_pixel,
            )
                .map(|stats| stats.count)
                .unwrap_or(0);
            println!(
                "compose_shadow probe={compose_probe:?} shadow_pixels={compose_shadow_pixels} screenshot={}",
                compose_path.display()
            );
            if compose_shadow_pixels < 300 {
                fail(
                    &robot,
                    &format!("Compose shadow preview is missing shadow pixels: count={compose_shadow_pixels}"),
                );
            }

            println!("PASS: shadow showcases contain visible shadow pixels");
            robot.exit().expect("exit");
        })
        .run(|| {
            app::combined_app_with_startup(StartupSelection {
                initial_tab: Some(DemoTab::Shaders),
                initial_shader_section: Some(ShaderSection::GraphicsLayerFields),
            });
        });
}

fn run_dstout_alpha() {
    println!("=== Shader DstOut Alpha Contract ===");
    AppLauncher::new()
        .with_title(DSTOUT_TITLE)
        .with_size(WINDOW_WIDTH, WINDOW_HEIGHT)
        .with_headless(true)
        .with_test_driver(|robot| {
            std::thread::sleep(Duration::from_millis(700));
            let _ = robot.wait_for_idle();
            wait_for_text(&robot, "Cut / Opacity Mask APIs");
            let _ = set_slider_fraction(&robot, "preview_alpha", 0.35, 156.0, 9.0, shader_scroll_config());
            std::thread::sleep(Duration::from_millis(180));
            let top_layer = settle_text_into_vertical_band(
                &robot,
                "TOP LAYER",
                560.0,
                830.0,
                shader_scroll_config(),
            )
            .unwrap_or_else(|| fail(&robot, "TOP LAYER text not visible in measurement band"));
            let screenshot = robot.screenshot().expect("DstOut screenshot");
            let dstout_path = output_paths::diagnostic_path("shader_dstout_alpha.png");
            save_robot_screenshot(&dstout_path, &screenshot);
            let top_crop = crop_screenshot_logical(
                &screenshot,
                (top_layer.0 - 4.0).max(0.0),
                (top_layer.1 - 4.0).max(0.0),
                top_layer.2 + 8.0,
                top_layer.3 + 8.0,
            )
            .unwrap_or_else(|| fail(&robot, "failed to crop TOP LAYER text"));
            let top_edge = edge_energy_screenshot(&top_crop);
            let underlay = find_text_in_semantics(&robot, "UNDERLAY CONTENT")
                .unwrap_or_else(|| fail(&robot, "UNDERLAY CONTENT text not visible after preview_alpha change"));
            let underlay_crop = crop_screenshot_logical(
                &screenshot,
                (underlay.0 - 4.0).max(0.0),
                (underlay.1 - 4.0).max(0.0),
                underlay.2 + 8.0,
                underlay.3 + 8.0,
            )
            .unwrap_or_else(|| fail(&robot, "failed to crop UNDERLAY CONTENT text"));
            let underlay_edge = edge_energy_screenshot(&underlay_crop);
            println!(
                "dstout_alpha top_edge={top_edge:.3} underlay_edge={underlay_edge:.3} top={top_layer:?} underlay={underlay:?} screenshot={}",
                dstout_path.display()
            );
            if top_edge < 3.5 || underlay_edge < 8.0 {
                fail(
                    &robot,
                    &format!("DstOut preview damaged text after preview_alpha changed: top_edge={top_edge:.3} underlay_edge={underlay_edge:.3}"),
                );
            }

            println!("PASS: DstOut alpha change keeps preview text visible");
            robot.exit().expect("exit");
        })
        .run(|| {
            app::combined_app_with_startup(StartupSelection {
                initial_tab: Some(DemoTab::Shaders),
                initial_shader_section: Some(ShaderSection::MaskApi),
            });
        });
}

fn settle_text_into_vertical_band(
    robot: &cranpose::Robot,
    text: &str,
    min_y: f32,
    max_y: f32,
    cfg: ScrollConfig,
) -> Option<(f32, f32, f32, f32)> {
    let mut bounds = scroll_text_into_view(robot, text, 30, cfg)?;
    for _ in 0..8 {
        if bounds.1 >= min_y && bounds.1 + bounds.3 <= max_y {
            return Some(bounds);
        }
        if bounds.1 + bounds.3 > max_y {
            let _ = robot.drag(cfg.center_x, cfg.down_from_y, cfg.center_x, cfg.down_to_y);
        } else {
            let _ = robot.drag(cfg.center_x, cfg.up_from_y, cfg.center_x, cfg.up_to_y);
        }
        std::thread::sleep(Duration::from_millis(160));
        let _ = robot.wait_for_idle();
        bounds = find_text_in_semantics(robot, text)?;
    }
    (bounds.1 >= min_y && bounds.1 + bounds.3 <= max_y).then_some(bounds)
}

fn drag_window(window_id: &str, from: (f32, f32), to: (f32, f32)) {
    move_x11_mouse_in_window(window_id, from.0, from.1);
    std::thread::sleep(Duration::from_millis(40));
    let _ = std::process::Command::new("xdotool")
        .args(["mousedown", "1"])
        .status()
        .expect("xdotool mousedown");
    for step in 1..=24 {
        let t = step as f32 / 24.0;
        let x = from.0 + (to.0 - from.0) * t;
        let y = from.1 + (to.1 - from.1) * t;
        move_x11_mouse_in_window(window_id, x, y);
        std::thread::sleep(Duration::from_millis(8));
    }
    let _ = std::process::Command::new("xdotool")
        .args(["mouseup", "1"])
        .status()
        .expect("xdotool mouseup");
}

fn select_shaders_tab(robot: &cranpose::Robot) {
    let Some((x, y, w, h)) = find_button_in_semantics(robot, "Shaders") else {
        fail(robot, "Shaders tab not found");
    };
    robot
        .click(x + w * 0.5, y + h * 0.5)
        .expect("click Shaders");
    std::thread::sleep(Duration::from_millis(320));
    let _ = robot.wait_for_idle();
}

fn wait_for_text(robot: &cranpose::Robot, text: &str) {
    for _ in 0..40 {
        if find_text_in_semantics(robot, text).is_some() {
            return;
        }
        std::thread::sleep(Duration::from_millis(100));
        let _ = robot.wait_for_idle();
    }
    fail(robot, &format!("text {text:?} did not appear"));
}

fn shader_scroll_config() -> ScrollConfig {
    ScrollConfig {
        center_x: 620.0,
        down_from_y: 760.0,
        down_to_y: 220.0,
        up_from_y: 220.0,
        up_to_y: 760.0,
    }
}

fn scrollbar_probe_region(screenshot: &cranpose::RobotScreenshot) -> (f32, f32, f32, f32) {
    (
        screenshot.logical_width - 34.0,
        86.0,
        30.0,
        screenshot.logical_height - 120.0,
    )
}

fn assert_no_implicit_scrollbar(
    robot: &cranpose::Robot,
    phase: &str,
    screenshot: &cranpose::RobotScreenshot,
) {
    let stats = feature_stats_screenshot(
        screenshot,
        scrollbar_probe_region(screenshot),
        is_scrollbar_thumb_pixel,
    );
    println!("shader_no_implicit_scrollbar phase={phase} stats={stats:?}");
    if stats.is_some_and(|stats| stats.count > 80) {
        fail(
            robot,
            &format!("plain shader scroll container drew implicit scrollbar chrome {phase}"),
        );
    }
}

fn is_bright_label_pixel(pixel: [u8; 4]) -> bool {
    pixel[3] > 180 && pixel[0] > 210 && pixel[1] > 210 && pixel[2] > 210
}

fn is_scrollbar_thumb_pixel(pixel: [u8; 4]) -> bool {
    pixel[3] > 180 && pixel[0] > 145 && pixel[1] > 155 && pixel[2] > 180
}

fn is_blue_blur_pixel(pixel: [u8; 4]) -> bool {
    // The blurred overlap is cyan-blue, while the mint glass over the bare
    // checker remains green-dominant. Relative chroma keeps the classifier
    // valid when the glass material changes overall luminance.
    let [red, green, blue, alpha] = pixel;
    alpha > 170 && blue > 140 && green > 105 && blue > red.saturating_add(10) && blue > green
}

fn is_shadow_pixel(pixel: [u8; 4]) -> bool {
    let [red, green, blue, alpha] = pixel;
    alpha > 180 && red < 42 && green < 42 && blue < 56
}

fn is_soft_shadow_pixel(pixel: [u8; 4]) -> bool {
    let [red, green, blue, alpha] = pixel;
    alpha > 180 && red < 90 && green < 100 && blue < 120
}
