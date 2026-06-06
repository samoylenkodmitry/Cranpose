//! External X11 performance contract for the normal desktop demo shader paths.
//!
//! This runner launches the full demo shell with the FPS overlay enabled, walks
//! the user-visible tabs, then validates Shader Rect animation and Shaders glass
//! dragging through desktop frame telemetry.

mod external_x11_frame_telemetry;
mod output_paths;
mod text_showcase_external_helpers;

use cranpose::{AppLauncher, Robot};
use cranpose_testing::{find_button_exact_in_semantics, find_text_in_semantics};
use desktop_app::app::{self, DemoTab, ShaderSection, StartupSelection};
use external_x11_frame_telemetry::{
    clear_records, install_primary_frame_telemetry_logger, summarize_records, FrameTelemetryRecord,
};
use image::RgbaImage;
use std::io::Write;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use text_showcase_external_helpers::{
    capture_x11_window, find_window_id, focus_x11_window, mouse_down_x11_button,
    mouse_up_x11_button, move_x11_mouse_in_window,
};

const WINDOW_TITLE: &str = "Robot Shader Full Demo External Perf";
const WINDOW_WIDTH: u32 = 1200;
const WINDOW_HEIGHT: u32 = 1000;
const SAMPLE_DURATION_MS: u64 = 900;
const MIN_PRESENTED_FPS: f64 = 150.0;
const MIN_APP_REPORTED_FPS: f32 = 120.0;
const MAX_P95_TOTAL_MS: f64 = 6.67;
const MAX_APP_REPORTED_P95_MS: f32 = 8.33;
const MIN_SHADER_RECT_CHANGED_PIXELS: usize = 1_000;
const MIN_GLASS_DRAG_CHANGED_PIXELS: usize = 2_500;
const MIN_SHADER_RECT_FRAMES: usize = 96;
const MIN_GLASS_DRAG_FRAMES: usize = 36;
const GLASS_MOVE_STEPS: usize = 42;

fn main() {
    let records = install_primary_frame_telemetry_logger();
    let driver_records = Arc::clone(&records);

    AppLauncher::new()
        .with_title(WINDOW_TITLE)
        .with_size(WINDOW_WIDTH, WINDOW_HEIGHT)
        .with_headless(false)
        .with_fps_counter(true)
        .with_frame_pacing_controls(true)
        .with_test_driver(move |robot| {
            let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                run_external_driver(robot, driver_records);
            }));
            if let Err(payload) = result {
                eprintln!(
                    "FAIL: full-demo shader external driver panicked: {}",
                    panic_message(payload)
                );
                let _ = std::io::stderr().flush();
                std::process::exit(0);
            }
        })
        .run(|| {
            app::combined_app_with_startup(StartupSelection {
                initial_tab: Some(DemoTab::Counter),
                initial_shader_section: Some(ShaderSection::InteractiveEffects),
            });
        });
}

fn run_external_driver(robot: Robot, records: Arc<Mutex<Vec<FrameTelemetryRecord>>>) {
    let window_id = find_window_id(WINDOW_TITLE);
    focus_x11_window(&window_id);
    std::thread::sleep(Duration::from_millis(900));
    let _ = robot.wait_for_idle();
    let mut failures = Vec::new();

    for label in ["Images", "Text", "Winamp", "XKCD", "Shader Rect"] {
        click_tab(&robot, label);
    }

    let before_path = output_paths::diagnostic_path("shader_full_demo_rect_before.png");
    let before = capture_x11_window(&window_id, &before_path);
    clear_records(&records);
    robot
        .reset_fps_stats()
        .expect("reset Shader Rect app FPS stats");
    let shader_rect_start = Instant::now();
    std::thread::sleep(Duration::from_millis(SAMPLE_DURATION_MS));
    let shader_rect_end = Instant::now();
    let shader_rect_app_stats = robot.fps_stats().expect("read Shader Rect app FPS stats");
    let after_path = output_paths::diagnostic_path("shader_full_demo_rect_after.png");
    let after = capture_x11_window(&window_id, &after_path);
    let shader_rect_changed = changed_pixel_count(&before, &after, 10);
    let shader_rect_summary = summarize_records(&records, shader_rect_start, shader_rect_end);
    print_summary(
        "shader_full_demo_rect",
        shader_rect_changed,
        &shader_rect_summary,
        &before_path,
        &after_path,
    );
    print_app_fps("shader_full_demo_rect", shader_rect_app_stats);
    failures.extend(summary_failures(
        "Shader Rect full-demo animation",
        shader_rect_changed,
        MIN_SHADER_RECT_CHANGED_PIXELS,
        MIN_SHADER_RECT_FRAMES,
        &shader_rect_summary,
    ));
    failures.extend(app_fps_failures(
        "Shader Rect full-demo animation",
        shader_rect_app_stats,
    ));

    click_tab(&robot, "Shaders");
    let (glass_start_x, glass_start_y, glass_end_x, glass_end_y) = glass_drag_points(&robot);
    println!(
        "glass_drag_points: start=({glass_start_x:.1},{glass_start_y:.1}) end=({glass_end_x:.1},{glass_end_y:.1})"
    );
    let before_path = output_paths::diagnostic_path("shader_full_demo_glass_before.png");
    let before = capture_x11_window(&window_id, &before_path);
    move_x11_mouse_in_window(&window_id, glass_start_x, glass_start_y);
    std::thread::sleep(Duration::from_millis(120));
    clear_records(&records);
    robot
        .reset_fps_stats()
        .expect("reset Shaders glass app FPS stats");
    mouse_down_x11_button("1");
    let glass_start = Instant::now();
    for step in 1..=GLASS_MOVE_STEPS {
        let t = step as f32 / GLASS_MOVE_STEPS as f32;
        let x = glass_start_x + (glass_end_x - glass_start_x) * t;
        let y = glass_start_y + (glass_end_y - glass_start_y) * t;
        move_x11_mouse_in_window(&window_id, x, y);
        std::thread::sleep(Duration::from_millis(3));
    }
    std::thread::sleep(Duration::from_millis(420));
    let glass_end = Instant::now();
    let glass_app_stats = robot.fps_stats().expect("read Shaders glass app FPS stats");
    let glass_render_stats = robot.get_render_stats().ok().flatten();
    mouse_up_x11_button("1");
    std::thread::sleep(Duration::from_millis(180));
    let after_path = output_paths::diagnostic_path("shader_full_demo_glass_after.png");
    let after = capture_x11_window(&window_id, &after_path);
    let glass_changed = changed_pixel_count(&before, &after, 10);
    let glass_summary = summarize_records(&records, glass_start, glass_end);
    print_summary(
        "shader_full_demo_glass_drag",
        glass_changed,
        &glass_summary,
        &before_path,
        &after_path,
    );
    print_app_fps("shader_full_demo_glass_drag", glass_app_stats);
    print_render_stats("shader_full_demo_glass_drag", glass_render_stats);
    failures.extend(summary_failures(
        "Shaders full-demo glass drag",
        glass_changed,
        MIN_GLASS_DRAG_CHANGED_PIXELS,
        MIN_GLASS_DRAG_FRAMES,
        &glass_summary,
    ));
    failures.extend(app_fps_failures(
        "Shaders full-demo glass drag",
        glass_app_stats,
    ));

    click_tab(&robot, "Counter App");
    if !failures.is_empty() {
        eprintln!("FAIL: {}", failures.join("; "));
        let _ = std::io::stderr().flush();
        std::process::exit(0);
    }
    let _ = std::io::stdout().flush();
    std::process::exit(0);
}

fn print_app_fps(scenario: &str, stats: cranpose::FpsStats) {
    println!(
        "{scenario}_app_fps: fps={:.1} work_fps={:.1} p95_ms={:.2} work_p95_ms={:.2} max_ms={:.2} work_max_ms={:.2} missed_120hz={} work_missed_120hz={} stalls_50ms={} work_stalls_50ms={} frames={} recompositions={}",
        stats.fps,
        stats.work_fps,
        stats.p95_ms,
        stats.work_p95_ms,
        stats.max_ms,
        stats.work_max_ms,
        stats.missed_120hz_budget,
        stats.work_missed_120hz_budget,
        stats.stalled_50ms_frames,
        stats.work_stalled_50ms_frames,
        stats.frame_count,
        stats.recompositions,
    );
}

fn print_render_stats(
    scenario: &str,
    render_stats: Option<cranpose_render_wgpu::RenderStatsSnapshot>,
) {
    if let Some(stats) = render_stats {
        println!(
            "{scenario}_render_stats: submit={} encoder={} pass={} blur={} composite={} effect={} isolated={} layer_hit={} layer_miss={} transient_bytes={} retained_bytes={} upload_bytes={}",
            stats.submit_count,
            stats.encoder_count,
            stats.pass_count,
            stats.blur_passes,
            stats.composite_passes,
            stats.effect_applies,
            stats.isolated_layer_renders,
            stats.layer_cache_hits,
            stats.layer_cache_misses,
            stats.transient_texture_bytes,
            stats.retained_texture_bytes,
            stats.upload_bytes
        );
    } else {
        println!("{scenario}_render_stats: unavailable");
    }
}

fn glass_drag_points(robot: &Robot) -> (f32, f32, f32, f32) {
    let Some((x, y, w, h)) = find_text_in_semantics(robot, "Glass") else {
        panic!("Glass overlay text not found");
    };
    let start_x = x + w * 0.5;
    let start_y = y + h * 0.5;
    let end_x = (start_x - WINDOW_WIDTH as f32 * 0.10).max(24.0);
    let end_y = (start_y - WINDOW_HEIGHT as f32 * 0.07).max(96.0);
    (start_x, start_y, end_x, end_y)
}

fn click_tab(robot: &Robot, label: &str) {
    let Some((x, y, w, h)) = find_button_exact_in_semantics(robot, label) else {
        panic!("tab {label:?} not found");
    };
    robot
        .click(x + w * 0.5, y + h * 0.5)
        .unwrap_or_else(|err| panic!("click tab {label:?}: {err}"));
    std::thread::sleep(Duration::from_millis(260));
    let _ = robot.wait_for_idle();
    println!("clicked tab {label}");
}

fn print_summary(
    scenario: &str,
    changed_pixels: usize,
    summary: &external_x11_frame_telemetry::FrameTelemetrySummary,
    before: &std::path::Path,
    after: &std::path::Path,
) {
    println!(
        "{scenario}: changed_pixels={} frames={} fps={:.1} p95_total_ms={:.2} max_total_ms={:.2} p95_update_ms={:.2} p95_acquire_ms={:.2} p95_render_ms={:.2} p95_present_ms={:.2} stalls_50ms={} before={} after={}",
        changed_pixels,
        summary.frames,
        summary.fps,
        summary.p95_total_ms,
        summary.max_total_ms,
        summary.p95_update_ms,
        summary.p95_acquire_ms,
        summary.p95_render_ms,
        summary.p95_present_ms,
        summary.stalls_50ms,
        before.display(),
        after.display()
    );
}

fn summary_failures(
    label: &str,
    changed_pixels: usize,
    min_changed_pixels: usize,
    min_frames: usize,
    summary: &external_x11_frame_telemetry::FrameTelemetrySummary,
) -> Vec<String> {
    let mut failures = Vec::new();
    if changed_pixels < min_changed_pixels {
        failures.push(format!(
            "{label} did not visibly change: changed_pixels={changed_pixels}"
        ));
    }
    if summary.frames < min_frames {
        failures.push(format!(
            "{label} produced too few presented frames: frames={}",
            summary.frames
        ));
    }
    if summary.fps < MIN_PRESENTED_FPS {
        failures.push(format!(
            "{label} missed {MIN_PRESENTED_FPS:.0}Hz presented cadence: fps={:.1}",
            summary.fps,
        ));
    }
    if summary.p95_total_ms > MAX_P95_TOTAL_MS {
        failures.push(format!(
            "{label} p95 frame time exceeded {MIN_PRESENTED_FPS:.0}Hz budget: p95_total_ms={:.2}",
            summary.p95_total_ms,
        ));
    }
    if summary.stalls_50ms > 0 {
        failures.push(format!(
            "{label} produced {} frame stalls over 50ms",
            summary.stalls_50ms
        ));
    }
    failures
}

fn app_fps_failures(label: &str, stats: cranpose::FpsStats) -> Vec<String> {
    let mut failures = Vec::new();
    if stats.fps < MIN_APP_REPORTED_FPS {
        failures.push(format!(
            "{label} app FPS counter missed {MIN_APP_REPORTED_FPS:.0}Hz cadence: fps={:.1}, frames={}",
            stats.fps, stats.frame_count
        ));
    }
    if stats.p95_ms > MAX_APP_REPORTED_P95_MS {
        failures.push(format!(
            "{label} app FPS counter p95 exceeded {MAX_APP_REPORTED_P95_MS:.2}ms: p95_ms={:.2}",
            stats.p95_ms
        ));
    }
    if stats.work_fps < MIN_APP_REPORTED_FPS {
        failures.push(format!(
            "{label} app work FPS missed {MIN_APP_REPORTED_FPS:.0}Hz: work_fps={:.1}",
            stats.work_fps
        ));
    }
    if stats.work_p95_ms > MAX_APP_REPORTED_P95_MS {
        failures.push(format!(
            "{label} app work p95 exceeded {MAX_APP_REPORTED_P95_MS:.2}ms: work_p95_ms={:.2}",
            stats.work_p95_ms
        ));
    }
    if stats.stalled_50ms_frames > 0 || stats.work_stalled_50ms_frames > 0 {
        failures.push(format!(
            "{label} app FPS stats reported stalls: cadence_stalls={} work_stalls={}",
            stats.stalled_50ms_frames, stats.work_stalled_50ms_frames
        ));
    }
    failures
}

fn changed_pixel_count(before: &RgbaImage, after: &RgbaImage, threshold: u8) -> usize {
    let width = before.width().min(after.width());
    let height = before.height().min(after.height());
    let threshold = threshold as i16;
    (0..height)
        .flat_map(|y| (0..width).map(move |x| (x, y)))
        .filter(|(x, y)| {
            let a = before.get_pixel(*x, *y).0;
            let b = after.get_pixel(*x, *y).0;
            a.iter()
                .zip(b.iter())
                .any(|(lhs, rhs)| (*lhs as i16 - *rhs as i16).abs() > threshold)
        })
        .count()
}

fn panic_message(payload: Box<dyn std::any::Any + Send>) -> String {
    if let Some(message) = payload.downcast_ref::<&str>() {
        (*message).to_string()
    } else if let Some(message) = payload.downcast_ref::<String>() {
        message.clone()
    } else {
        "unknown panic payload".to_string()
    }
}
