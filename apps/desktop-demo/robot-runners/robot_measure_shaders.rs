//! Robot runner for Shaders performance profiling.
//!
//! Steps:
//! 1. Open app
//! 2. Go to Shaders tab
//! 3. Drag "Blur" and "Glass" rects around
//! 4. Scroll the page up and down
//! 5. Repeat for profiling
//!
//! Run with:
//! ```bash
//! cargo run --package desktop-app --example robot_measure_shaders --features robot-app
//! ```

use cranpose::fps_stats;
use cranpose::AppLauncher;
use cranpose_testing::{
    capture_screenshot, find_button_in_semantics, find_in_semantics, find_text_exact,
    find_text_in_semantics, print_semantics_with_bounds,
};
use cranpose_ui::Point;
use desktop_app::app;
use image::{ImageBuffer, RgbaImage};
use std::fs;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

const WINDOW_WIDTH: u32 = 1200;
const WINDOW_HEIGHT: u32 = 800;
const DEFAULT_VISUAL_SETTLE_MS: u64 = 900;
const DEFAULT_VISUAL_SCROLL_STEPS: u64 = 4;
const DEFAULT_VISUAL_SCROLL_DELAY_MS: u64 = 140;
const DEFAULT_VISUAL_OUTPUT_DIR: &str = "/tmp/cranpose_shaders_visual_compare";

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum MeasureMode {
    Profile,
    VisualCompare,
}

impl MeasureMode {
    fn from_env() -> Self {
        match std::env::var("CRANPOSE_MEASURE_SHADERS_MODE")
            .ok()
            .as_deref()
        {
            Some("visual_compare") => Self::VisualCompare,
            _ => Self::Profile,
        }
    }
}

fn env_u64(key: &str, default: u64) -> u64 {
    std::env::var(key)
        .ok()
        .and_then(|v| v.parse::<u64>().ok())
        .unwrap_or(default)
}

fn env_bool(key: &str, default: bool) -> bool {
    std::env::var(key)
        .ok()
        .map(|v| matches!(v.to_lowercase().as_str(), "1" | "true" | "yes" | "on"))
        .unwrap_or(default)
}

fn env_string(key: &str, default: &str) -> String {
    std::env::var(key).unwrap_or_else(|_| default.to_string())
}

fn fatal(robot: &cranpose::Robot, message: &str) -> ! {
    println!("FATAL: {message}");
    let _ = robot.exit();
    std::process::exit(1);
}

fn log_stage_fps(stage: &str) {
    let stats = fps_stats();
    println!(
        "VISUAL_FPS stage={} fps={:.1} avg_ms={:.2} frames={} recompositions={} recomps_per_second={}",
        stage,
        stats.fps,
        stats.avg_ms,
        stats.frame_count,
        stats.recompositions,
        stats.recomps_per_second
    );
}

fn log_stage_render_stats(robot: &cranpose::Robot, stage: &str) {
    match robot.get_render_stats() {
        Ok(Some(stats)) => {
            println!(
                "VISUAL_RENDER stage={} submits={} offscreen_acquires={} offscreen_news={} offscreen_total_bytes={} upload_bytes={} isolated_layer_renders={} isolated_layer_pixels={} cache_hits={} cache_misses={} cache_evictions={} blur_passes={} composite_passes={} effect_applies={} shape_passes={} image_passes={} text_passes={}",
                stage,
                stats.submits,
                stats.offscreen_acquires,
                stats.offscreen_news,
                stats.offscreen_total_bytes,
                stats.upload_bytes,
                stats.isolated_layer_renders,
                stats.isolated_layer_pixels,
                stats.layer_cache_hits,
                stats.layer_cache_misses,
                stats.layer_cache_evictions,
                stats.blur_passes,
                stats.composite_passes,
                stats.effect_applies,
                stats.shape_passes,
                stats.image_passes,
                stats.text_passes
            );
            for (index, layer) in stats.top_isolated_layers().enumerate() {
                println!(
                    "VISUAL_ISOLATED stage={} rank={} node={:?} rect=({:.1},{:.1},{:.1},{:.1}) target={}x{} reasons={}",
                    stage,
                    index,
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
        Ok(None) => println!("VISUAL_RENDER stage={} unavailable", stage),
        Err(err) => println!("VISUAL_RENDER stage={} error={}", stage, err),
    }
}

fn save_png(path: &Path, screenshot: &cranpose::RobotScreenshot) -> Result<(), String> {
    let img: RgbaImage = ImageBuffer::from_raw(
        screenshot.width,
        screenshot.height,
        screenshot.pixels.clone(),
    )
    .ok_or_else(|| "invalid screenshot dimensions".to_string())?;
    img.save(path)
        .map_err(|err| format!("failed to save {}: {}", path.display(), err))
}

fn save_stage_screenshot(
    robot: &cranpose::Robot,
    output_dir: &Path,
    branch_label: &str,
    stage: &str,
) {
    let Some(screenshot) = capture_screenshot(robot) else {
        fatal(
            robot,
            &format!("failed to capture screenshot for stage '{stage}'"),
        );
    };
    let filename = format!("{}_{}.png", branch_label, stage);
    let path = output_dir.join(filename);
    if let Err(err) = save_png(&path, &screenshot) {
        fatal(
            robot,
            &format!("failed to save screenshot for stage '{stage}': {err}"),
        );
    }
    println!("VISUAL_SCREENSHOT stage={} path={}", stage, path.display());
}

fn wait_for_stage(robot: &cranpose::Robot, settle_ms: u64, stage: &str) {
    std::thread::sleep(Duration::from_millis(settle_ms));
    let _ = robot.wait_for_idle();
    log_stage_fps(stage);
    log_stage_render_stats(robot, stage);
}

fn click_button(robot: &cranpose::Robot, name: &str) -> bool {
    if let Some((x, y, w, h)) = find_button_in_semantics(robot, name) {
        let _ = robot.click(x + w / 2.0, y + h / 2.0);
        return true;
    }
    false
}

fn find_center(robot: &cranpose::Robot, text: &str) -> Option<Point> {
    find_in_semantics(robot, |elem| find_text_exact(elem, text)).map(|(x, y, w, h)| Point {
        x: x + w / 2.0,
        y: y + h / 2.0,
    })
}

fn run_visual_compare(robot: &cranpose::Robot) {
    let settle_ms = env_u64("CRANPOSE_VISUAL_SETTLE_MS", DEFAULT_VISUAL_SETTLE_MS);
    let scroll_steps = env_u64("CRANPOSE_VISUAL_SCROLL_STEPS", DEFAULT_VISUAL_SCROLL_STEPS);
    let scroll_delay_ms = env_u64(
        "CRANPOSE_VISUAL_SCROLL_DELAY_MS",
        DEFAULT_VISUAL_SCROLL_DELAY_MS,
    );
    let branch_label = env_string("CRANPOSE_BRANCH_LABEL", "unknown");
    let output_dir = PathBuf::from(env_string(
        "CRANPOSE_VISUAL_OUTPUT_DIR",
        DEFAULT_VISUAL_OUTPUT_DIR,
    ));
    if let Err(err) = fs::create_dir_all(&output_dir) {
        fatal(
            robot,
            &format!(
                "failed to create output dir {}: {}",
                output_dir.display(),
                err
            ),
        );
    }

    std::thread::sleep(Duration::from_millis(1000));
    let _ = robot.wait_for_idle();
    log_stage_fps("startup_idle");

    if !click_button(robot, "Shaders") {
        if let Ok(elements) = robot.get_semantics() {
            println!("Top-level semantics:");
            print_semantics_with_bounds(&elements, 0);
        }
        fatal(robot, "could not find 'Shaders' tab button");
    }
    wait_for_stage(robot, settle_ms, "shaders_open");

    if find_text_in_semantics(robot, "Shaders & Effects").is_none() {
        fatal(robot, "Shaders tab heading did not appear");
    }

    save_stage_screenshot(robot, &output_dir, &branch_label, "top");

    let scroll_x = WINDOW_WIDTH as f32 * 0.5;
    let scroll_start_y = WINDOW_HEIGHT as f32 * 0.82;
    let scroll_end_y = WINDOW_HEIGHT as f32 * 0.24;

    for step in 1..=scroll_steps {
        let _ = robot.drag(scroll_x, scroll_start_y, scroll_x, scroll_end_y);
        std::thread::sleep(Duration::from_millis(scroll_delay_ms));
        let stage = format!("scroll_down_{}", step);
        wait_for_stage(robot, settle_ms, &stage);
    }

    if find_text_in_semantics(robot, "Effect Semantics Checks").is_none() {
        println!("VISUAL_NOTE: 'Effect Semantics Checks' was not visible after scrolling");
    }

    save_stage_screenshot(robot, &output_dir, &branch_label, "scrolled");

    println!(
        "VISUAL_RUN_COMPLETE branch={} output_dir={}",
        branch_label,
        output_dir.display()
    );
    robot.exit().expect("Failed to exit");
}

fn run_profile(robot: &cranpose::Robot, duration: Duration, scroll_steps: usize) {
    std::thread::sleep(Duration::from_millis(1000));
    let _ = robot.wait_for_idle();

    if !click_button(robot, "Shaders") {
        println!("FATAL: Could not find 'Shaders' tab button");
        if let Ok(elements) = robot.get_semantics() {
            println!("Top-level semantics:");
            print_semantics_with_bounds(&elements, 0);
        }
        robot.exit().ok();
        std::process::exit(1);
    }
    std::thread::sleep(Duration::from_millis(500));
    println!("  ✓ Entered Shaders tab");

    let start_time = Instant::now();
    let mut loops = 0;

    while start_time.elapsed() < duration {
        loops += 1;
        println!(
            "  Loop #{} (elapsed: {:.1}s)",
            loops,
            start_time.elapsed().as_secs_f32()
        );

        if let Some(center) = find_center(robot, "Blur") {
            let p2 = Point {
                x: center.x + 100.0,
                y: center.y,
            };

            robot.drag(center.x, center.y, p2.x, p2.y).ok();
            let _ = robot.wait_for_idle();
        } else {
            println!("  ⚠ 'Blur' rect not found");
        }

        if let Some(center) = find_center(robot, "Glass") {
            let p2 = Point {
                x: center.x - 50.0,
                y: center.y + 50.0,
            };
            robot.drag(center.x, center.y, p2.x, p2.y).ok();
            let _ = robot.wait_for_idle();
        } else {
            println!("  ⚠ 'Glass' rect not found");
        }

        let scroll_x = WINDOW_WIDTH as f32 * 0.5;
        let window_h = WINDOW_HEIGHT as f32;

        for _ in 0..scroll_steps {
            robot
                .drag(scroll_x, window_h * 0.8, scroll_x, window_h * 0.2)
                .ok();
            std::thread::sleep(Duration::from_millis(100));
        }
        let _ = robot.wait_for_idle();

        for _ in 0..scroll_steps {
            robot
                .drag(scroll_x, window_h * 0.2, scroll_x, window_h * 0.8)
                .ok();
            std::thread::sleep(Duration::from_millis(100));
        }
        let _ = robot.wait_for_idle();
    }

    let stats = fps_stats();
    println!("=== Profiling Run Complete ===");
    println!("  FPS: {:.1}", stats.fps);
    println!("  Avg frame time: {:.2}ms", stats.avg_ms);
    println!("  Total frames: {}", stats.frame_count);
    println!(
        "  Recompositions: {} ({}/s)",
        stats.recompositions, stats.recomps_per_second
    );
    robot.exit().expect("Failed to exit");
}

fn main() {
    env_logger::init();
    println!("=== Shaders Performance Profiling Robot ===");

    let mode = MeasureMode::from_env();
    let headless = env_bool("CRANPOSE_HEADLESS", false);
    let duration_secs = env_u64("CRANPOSE_PERF_DURATION_SECS", 20);
    let duration = Duration::from_secs(duration_secs);
    println!(
        "  mode={:?}, headless={}, duration={}s",
        mode, headless, duration_secs
    );

    let scroll_steps = 10;

    AppLauncher::new()
        .with_title("Shaders Profiling")
        .with_size(WINDOW_WIDTH, WINDOW_HEIGHT)
        .with_headless(headless)
        .with_test_driver(move |robot| match mode {
            MeasureMode::Profile => run_profile(&robot, duration, scroll_steps),
            MeasureMode::VisualCompare => run_visual_compare(&robot),
        })
        .run(|| {
            app::combined_app();
        });
}
