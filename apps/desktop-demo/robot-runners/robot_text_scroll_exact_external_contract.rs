//! Robot test: capture REAL window screenshots of the Text showcase while scrolling by
//! exactly one logical pixel per step and require perfect overlap in the shared middle area.

mod text_showcase_external_helpers;

use cranpose::AppLauncher;
use cranpose_testing::find_text_in_semantics;
use desktop_app::app;
use image::{ImageBuffer, RgbaImage};
use std::path::PathBuf;
use std::process::Command;
use std::time::Duration;
use text_showcase_external_helpers::{
    find_window_id, open_text_tab, scroll_text_into_view, take_x11_screenshot,
};

const WINDOW_WIDTH: u32 = 1200;
const WINDOW_HEIGHT: u32 = 900;
const WINDOW_TITLE: &str = "Robot Text Scroll Exact External";
const TARGET_TEXT: &str =
    "This is bold green and this is normal text. This is red, italic, and underlined!";
const SCROLL_STEPS: usize = 10;
const SCROLL_DELTA_Y: f32 = -1.0;
const STEP_EPSILON: f32 = 0.05;
const COMPARE_TRIM_TOP_PX: u32 = 200;
const COMPARE_TRIM_BOTTOM_PX: u32 = 200;
const COMPARE_SEARCH_OFFSET_PX: u32 = 32;
const INTERNAL_DIAGNOSTIC_ENV: &str = "CRANPOSE_TEXT_SCROLL_INTERNAL_DIAGNOSTIC";
const INTERNAL_DIAGNOSTIC_SCALE_ENV: &str = "CRANPOSE_TEXT_SCROLL_INTERNAL_DIAGNOSTIC_SCALE";
const RENDER_STATS_ENV: &str = "CRANPOSE_TEXT_SCROLL_RENDER_STATS";

fn main() {
    env_logger::init();
    println!("=== Robot Text Scroll Exact External ===");
    let output_dir = prepare_output_dir();
    println!("Output dir: {}", output_dir.display());
    let internal_diagnostic = prepare_internal_diagnostic();
    if let Some(diagnostic) = &internal_diagnostic {
        println!(
            "Internal diagnostic dir: {} scale={:.4}",
            diagnostic.output_dir.display(),
            diagnostic.capture_scale
        );
    }

    AppLauncher::new()
        .with_title(WINDOW_TITLE)
        .with_size(WINDOW_WIDTH, WINDOW_HEIGHT)
        .with_headless(false)
        .with_test_driver(move |robot| {
            std::thread::sleep(Duration::from_millis(1000));
            let _ = robot.wait_for_idle();

            open_text_tab(&robot);
            scroll_text_into_view(&robot, TARGET_TEXT, WINDOW_HEIGHT, 20);
            std::thread::sleep(Duration::from_millis(500));
            let _ = robot.wait_for_idle();

            let window_id = find_window_id(WINDOW_TITLE);
            let primary_bounds =
                find_text_in_semantics(&robot, TARGET_TEXT).expect("target text must be visible");

            let mut previous_y = primary_bounds.1;
            let mut capture_paths = Vec::with_capacity(SCROLL_STEPS);
            let mut internal_capture_paths = internal_diagnostic
                .as_ref()
                .map(|_| Vec::with_capacity(SCROLL_STEPS));

            for step in 0..SCROLL_STEPS {
                let anchor_x = primary_bounds.0 + primary_bounds.2 * 0.5;
                let anchor_y = primary_bounds.1 + primary_bounds.3 * 0.5;
                robot.mouse_move(anchor_x, anchor_y).expect("move cursor");
                std::thread::sleep(Duration::from_millis(30));
                robot
                    .mouse_scroll(0.0, SCROLL_DELTA_Y)
                    .expect("1px scroll should succeed");
                std::thread::sleep(Duration::from_millis(150));
                let _ = robot.wait_for_idle();

                let current_bounds =
                    find_text_in_semantics(&robot, TARGET_TEXT).expect("target text still visible");
                let actual_delta = current_bounds.1 - previous_y;
                println!(
                    "step {step}: text_y={:.2} delta={:.2}",
                    current_bounds.1, actual_delta
                );
                assert!(
                    (actual_delta - SCROLL_DELTA_Y).abs() <= STEP_EPSILON,
                    "expected exact 1px upward scroll, got delta_y={actual_delta:.3} at step {step}",
                );
                previous_y = current_bounds.1;

                let screenshot_path = output_dir.join(format!("text_scroll_step{step:02}.png"));
                take_x11_screenshot(&window_id, screenshot_path.to_str().expect("utf8 path"));
                capture_paths.push(screenshot_path);
                if std::env::var_os(RENDER_STATS_ENV).is_some() {
                    log_render_stats(&robot, step);
                }
                if let (Some(diagnostic), Some(paths)) =
                    (internal_diagnostic.as_ref(), internal_capture_paths.as_mut())
                {
                    let screenshot = robot
                        .screenshot_with_scale(diagnostic.capture_scale)
                        .expect("internal screenshot");
                    let internal_path = diagnostic
                        .output_dir
                        .join(format!("text_scroll_step{step:02}.png"));
                    save_robot_screenshot(&internal_path, &screenshot);
                    paths.push(internal_path);
                }
            }

            let compare_ok = run_compare_script(&capture_paths);
            if let Some(paths) = internal_capture_paths.as_ref() {
                let internal_ok = run_compare_script(paths);
                println!("internal diagnostic compare_ok={internal_ok}");
            }
            cleanup_capture_paths(&capture_paths);
            if let Some(paths) = internal_capture_paths.as_ref() {
                cleanup_capture_paths(paths);
            }
            if !compare_ok {
                std::process::exit(1);
            }

            println!("\n=== Test Summary ===");
            println!("PASS: external scroll capture stayed pixel-identical in the overlapping region");
            robot.exit().expect("exit");
        })
        .run(app::combined_app);
}

fn output_dir() -> PathBuf {
    let mut path = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    path.pop();
    path.pop();
    path.push("docs");
    path.push("screenshots");
    path.push("text_scroll_exact_external");
    path
}

fn prepare_output_dir() -> PathBuf {
    let output_dir = output_dir();
    recreate_output_dir(&output_dir);
    output_dir
}

struct InternalDiagnostic {
    output_dir: PathBuf,
    capture_scale: f32,
}

fn prepare_internal_diagnostic() -> Option<InternalDiagnostic> {
    std::env::var_os(INTERNAL_DIAGNOSTIC_ENV)?;

    let output_dir = PathBuf::from("/tmp/cranpose_text_scroll_exact_internal");
    recreate_output_dir(&output_dir);
    let capture_scale = std::env::var(INTERNAL_DIAGNOSTIC_SCALE_ENV)
        .ok()
        .and_then(|value| value.parse::<f32>().ok())
        .filter(|value| value.is_finite() && *value > 0.0)
        .unwrap_or(1.0);
    Some(InternalDiagnostic {
        output_dir,
        capture_scale,
    })
}

fn recreate_output_dir(output_dir: &std::path::Path) {
    if output_dir.exists() {
        std::fs::remove_dir_all(output_dir)
            .unwrap_or_else(|err| panic!("failed to clear {}: {err}", output_dir.display()));
    }
    std::fs::create_dir_all(output_dir)
        .unwrap_or_else(|err| panic!("failed to create {}: {err}", output_dir.display()));
}

fn compare_script_path() -> PathBuf {
    let mut path = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    path.pop();
    path.pop();
    path.push("scripts");
    path.push("text_scroll_exact_external_compare.py");
    path
}

fn run_compare_script(capture_paths: &[PathBuf]) -> bool {
    assert_eq!(
        capture_paths.len(),
        SCROLL_STEPS,
        "expected exactly {SCROLL_STEPS} screenshots"
    );

    let script_path = compare_script_path();
    let output = Command::new("python3")
        .arg(&script_path)
        .arg("--trim-top")
        .arg(COMPARE_TRIM_TOP_PX.to_string())
        .arg("--trim-bottom")
        .arg(COMPARE_TRIM_BOTTOM_PX.to_string())
        .arg("--max-offset")
        .arg(COMPARE_SEARCH_OFFSET_PX.to_string())
        .args(capture_paths)
        .output()
        .unwrap_or_else(|err| panic!("failed to run {}: {err}", script_path.display()));

    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    if !stdout.trim().is_empty() {
        println!("{stdout}");
    }
    if !stderr.trim().is_empty() {
        eprintln!("{stderr}");
    }
    if !output.status.success() {
        eprintln!("compare script failed");
        return false;
    }
    true
}

fn cleanup_capture_paths(capture_paths: &[PathBuf]) {
    for path in capture_paths {
        if path.exists() {
            std::fs::remove_file(path)
                .unwrap_or_else(|err| panic!("failed to remove {}: {err}", path.display()));
        }
    }
}

fn save_robot_screenshot(path: &std::path::Path, screenshot: &cranpose::RobotScreenshot) {
    let image: RgbaImage = ImageBuffer::from_raw(
        screenshot.width,
        screenshot.height,
        screenshot.pixels.clone(),
    )
    .expect("valid screenshot dimensions");
    image
        .save(path)
        .unwrap_or_else(|err| panic!("failed to save {}: {err}", path.display()));
}

fn log_render_stats(robot: &cranpose::Robot, step: usize) {
    match robot.get_render_stats() {
        Ok(Some(stats)) => {
            println!(
                "render_stats step={step} isolated_layer_renders={} top_isolated_layer_count={} offscreen_acquires={} composite_passes={} blur_passes={} text_passes={} shape_passes={} image_passes={}",
                stats.isolated_layer_renders,
                stats.top_isolated_layer_count,
                stats.offscreen_acquires,
                stats.composite_passes,
                stats.blur_passes,
                stats.text_passes,
                stats.shape_passes,
                stats.image_passes
            );
            println!(
                "render_stats_top_slots step={step} slots={:?}",
                stats.top_isolated_layers
            );
            for (index, layer) in stats.top_isolated_layers().enumerate() {
                println!(
                    "isolated_layer step={step} rank={index} node={:?} rect=({:.1},{:.1},{:.1},{:.1}) target={}x{} reasons={}",
                    layer.node_id,
                    layer.logical_rect.x,
                    layer.logical_rect.y,
                    layer.logical_rect.width,
                    layer.logical_rect.height,
                    layer.width,
                    layer.height,
                    layer.reasons.display()
                );
            }
        }
        Ok(None) => println!("render_stats step={step} unavailable"),
        Err(err) => println!("render_stats step={step} error={err}"),
    }
}
