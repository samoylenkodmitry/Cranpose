mod output_paths;

use std::{
    fs,
    path::{Path, PathBuf},
    time::{Duration, Instant},
};

use cranpose::AppLauncher;
use cranpose_testing::{
    capture_screenshot, find_button_in_semantics, find_in_semantics, find_text_exact,
    find_text_in_semantics, print_semantics_with_bounds,
};
use cranpose_ui::Point;
use desktop_app::app;
use image::{ImageBuffer, RgbaImage};

const WINDOW_WIDTH: u32 = 1200;
const WINDOW_HEIGHT: u32 = 800;
const DEFAULT_VISUAL_SETTLE_MS: u64 = 900;
const DEFAULT_VISUAL_SCROLL_STEPS: u64 = 4;
const DEFAULT_VISUAL_SCROLL_DELAY_MS: u64 = 140;
const DEFAULT_PROFILE_DURATION_SECS: u64 = 20;
const DEFAULT_HEADLESS_PROFILE_DURATION_SECS: u64 = 5;
const DEFAULT_PROFILE_SCROLL_STEPS: usize = 10;
const DEFAULT_HEADLESS_PROFILE_SCROLL_STEPS: usize = 3;
const MAX_MIXED_DIRECT_LAYER_PIXELS: u64 = 400_000;
const MAX_MIXED_DIRECT_LAYER_LOGICAL_HEIGHT: f32 = 600.0;

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

fn env_usize(key: &str, default: usize) -> usize {
    std::env::var(key)
        .ok()
        .and_then(|v| v.parse::<usize>().ok())
        .unwrap_or(default)
}

fn default_profile_duration_secs(headless: bool) -> u64 {
    if headless {
        DEFAULT_HEADLESS_PROFILE_DURATION_SECS
    } else {
        DEFAULT_PROFILE_DURATION_SECS
    }
}

fn default_profile_scroll_steps(headless: bool) -> usize {
    if headless {
        DEFAULT_HEADLESS_PROFILE_SCROLL_STEPS
    } else {
        DEFAULT_PROFILE_SCROLL_STEPS
    }
}

fn fatal(robot: &cranpose::Robot, message: &str) -> ! {
    println!("FATAL: {message}");
    let _ = robot.exit();
    std::process::exit(1);
}

fn log_stage_fps(robot: &cranpose::Robot, stage: &str) {
    let stats = robot
        .fps_stats()
        .unwrap_or_else(|err| fatal(robot, &format!("failed to read FPS stats: {err}")));
    println!(
        "VISUAL_FPS stage={} fps={:.1} avg_ms={:.2} p95_ms={:.2} p99_ms={:.2} max_ms={:.2} work_avg_ms={:.2} work_p95_ms={:.2} work_max_ms={:.2} missed_120hz={} missed_60hz={} stalls_50ms={} frames={} recompositions={} recomps_per_second={}",
        stage,
        stats.fps,
        stats.avg_ms,
        stats.p95_ms,
        stats.p99_ms,
        stats.max_ms,
        stats.work_avg_ms,
        stats.work_p95_ms,
        stats.work_max_ms,
        stats.missed_120hz_budget,
        stats.missed_60hz_budget,
        stats.stalled_50ms_frames,
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

fn assert_no_large_mixed_direct_layers(robot: &cranpose::Robot, stage: &str) {
    let stats = match robot.get_render_stats() {
        Ok(Some(stats)) => stats,
        Ok(None) => fatal(
            robot,
            &format!("render stats unavailable while checking stage '{stage}'"),
        ),
        Err(err) => fatal(
            robot,
            &format!("failed to read render stats for stage '{stage}': {err}"),
        ),
    };

    for layer in stats.top_isolated_layers() {
        if !layer.reasons.mixed_direct_content {
            continue;
        }
        let pixel_area = (layer.width as u64) * (layer.height as u64);
        if pixel_area > MAX_MIXED_DIRECT_LAYER_PIXELS
            || layer.logical_rect.height > MAX_MIXED_DIRECT_LAYER_LOGICAL_HEIGHT
        {
            fatal(
                robot,
                &format!(
                    "stage '{stage}' still has an oversized mixed-content isolated layer: node={:?} rect=({:.1},{:.1},{:.1},{:.1}) target={}x{} reasons={}",
                    layer.node_id,
                    layer.logical_rect.x,
                    layer.logical_rect.y,
                    layer.logical_rect.width,
                    layer.logical_rect.height,
                    layer.width,
                    layer.height,
                    layer.reasons.display()
                ),
            );
        }
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
    log_stage_fps(robot, stage);
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
    let output_dir = std::env::var("CRANPOSE_VISUAL_OUTPUT_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|_| output_paths::diagnostic_path("cranpose_shaders_visual_compare"));
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
    log_stage_fps(robot, "startup_idle");

    if !click_button(robot, "Shaders") {
        if let Ok(elements) = robot.get_semantics() {
            println!("Top-level semantics:");
            print_semantics_with_bounds(&elements, 0);
        }
        fatal(robot, "could not find 'Shaders' tab button");
    }
    wait_for_stage(robot, settle_ms, "shaders_open");
    assert_no_large_mixed_direct_layers(robot, "shaders_open");

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
        assert_no_large_mixed_direct_layers(robot, &stage);
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

fn run_profile(robot: &cranpose::Robot, duration: Duration, scroll_steps: usize, headless: bool) {
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
    assert_no_large_mixed_direct_layers(robot, "profile_open");
    log_stage_fps(robot, "profile_open");
    log_stage_render_stats(robot, "profile_open");

    if headless {
        println!("  ✓ Headless profile smoke completed");
        robot.exit().expect("Failed to exit");
        return;
    }

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

    let stats = robot
        .fps_stats()
        .unwrap_or_else(|err| fatal(robot, &format!("failed to read FPS stats: {err}")));
    println!("=== Profiling Run Complete ===");
    println!("  FPS: {:.1}", stats.fps);
    println!("  Avg frame time: {:.2}ms", stats.avg_ms);
    println!("  P95 frame time: {:.2}ms", stats.p95_ms);
    println!("  P99 frame time: {:.2}ms", stats.p99_ms);
    println!("  Max frame time: {:.2}ms", stats.max_ms);
    println!("  Avg frame work: {:.2}ms", stats.work_avg_ms);
    println!("  P95 frame work: {:.2}ms", stats.work_p95_ms);
    println!("  Max frame work: {:.2}ms", stats.work_max_ms);
    println!(
        "  Budget misses: 120Hz={} 60Hz={} stalls50ms={}",
        stats.missed_120hz_budget, stats.missed_60hz_budget, stats.stalled_50ms_frames
    );
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
    let headless_default = matches!(mode, MeasureMode::Profile);
    let headless = env_bool("CRANPOSE_HEADLESS", headless_default);
    let duration_secs = env_u64(
        "CRANPOSE_PERF_DURATION_SECS",
        default_profile_duration_secs(headless),
    );
    let duration = Duration::from_secs(duration_secs);
    let scroll_steps = env_usize(
        "CRANPOSE_PERF_SCROLL_STEPS",
        default_profile_scroll_steps(headless),
    );
    println!(
        "  mode={:?}, headless={}, duration={}s, scroll_steps={}",
        mode, headless, duration_secs, scroll_steps
    );

    AppLauncher::new()
        .with_title("Shaders Profiling")
        .with_size(WINDOW_WIDTH, WINDOW_HEIGHT)
        .with_headless(headless)
        .with_test_driver(move |robot| match mode {
            MeasureMode::Profile => run_profile(&robot, duration, scroll_steps, headless),
            MeasureMode::VisualCompare => run_visual_compare(&robot),
        })
        .run(|| {
            app::combined_app();
        });
}

#[cfg(test)]
mod tests {
    use super::{
        default_profile_duration_secs, default_profile_scroll_steps,
        DEFAULT_HEADLESS_PROFILE_DURATION_SECS, DEFAULT_HEADLESS_PROFILE_SCROLL_STEPS,
        DEFAULT_PROFILE_DURATION_SECS, DEFAULT_PROFILE_SCROLL_STEPS,
    };

    #[test]
    fn headless_profile_defaults_stay_bounded() {
        assert_eq!(
            default_profile_duration_secs(true),
            DEFAULT_HEADLESS_PROFILE_DURATION_SECS
        );
        assert_eq!(
            default_profile_scroll_steps(true),
            DEFAULT_HEADLESS_PROFILE_SCROLL_STEPS
        );
    }

    #[test]
    fn interactive_profile_defaults_keep_full_workload() {
        assert_eq!(
            default_profile_duration_secs(false),
            DEFAULT_PROFILE_DURATION_SECS
        );
        assert_eq!(
            default_profile_scroll_steps(false),
            DEFAULT_PROFILE_SCROLL_STEPS
        );
    }
}
