use crate::text_showcase_external_helpers::{find_window_id, take_x11_screenshot};
use cranpose_testing::{find_in_semantics, find_text_in_semantics, print_semantics_with_bounds};
use image::{ImageBuffer, RgbaImage};
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::Duration;

#[derive(Clone, Copy, Debug)]
pub(crate) struct ScrollStabilityConfig {
    pub window_title: &'static str,
    pub output_name: &'static str,
    pub file_prefix: &'static str,
    pub target_text: &'static str,
    pub viewport_tag: Option<&'static str>,
    pub window_width: u32,
    pub window_height: u32,
    pub scroll_steps: usize,
    pub scroll_delta_y: f32,
    pub step_epsilon: f32,
    pub fallback_trim_top_px: u32,
    pub fallback_trim_bottom_px: u32,
    pub compare_search_offset_px: u32,
    pub compare_max_adjacent_score: u32,
    pub compare_stabilized_guard_px: u32,
    pub compare_viewport_inset_px: u32,
    pub render_stats_env: Option<&'static str>,
}

#[derive(Clone, Copy, Debug)]
struct CompareCrop {
    trim_top_px: u32,
    trim_bottom_px: u32,
    trim_left_px: u32,
    trim_right_px: u32,
    logical_window_space: bool,
}

#[derive(Debug)]
pub(crate) struct InternalDiagnostic {
    pub output_dir: PathBuf,
    pub capture_scale: f32,
}

pub(crate) fn run_scroll_stability_capture(
    robot: &cranpose::Robot,
    config: ScrollStabilityConfig,
    internal_diagnostic: Option<&InternalDiagnostic>,
) -> bool {
    assert!(
        config.scroll_steps >= 2,
        "scroll stability capture needs at least two steps"
    );
    let output_dir = prepare_named_output_dir(config.output_name);
    println!("Output dir: {}", output_dir.display());
    let window_id = find_window_id(config.window_title);
    let crop = compare_crop(robot, config);
    println!(
        "compare crop: left={} right={} top={} bottom={}",
        crop.trim_left_px, crop.trim_right_px, crop.trim_top_px, crop.trim_bottom_px
    );

    let mut previous_bounds = find_text_in_semantics(robot, config.target_text)
        .unwrap_or_else(|| fail_with_semantics(robot, "target text must be visible"));

    let mut capture_paths = Vec::with_capacity(config.scroll_steps);
    let mut internal_capture_paths = internal_diagnostic
        .as_ref()
        .map(|_| Vec::with_capacity(config.scroll_steps));

    for step in 0..config.scroll_steps {
        previous_bounds =
            scroll_once_and_expect_target_delta(robot, config, previous_bounds, step, "step");

        let screenshot_path = output_dir.join(format!("{}_step{step:02}.png", config.file_prefix));
        take_x11_screenshot(&window_id, screenshot_path.to_str().expect("utf8 path"));
        capture_paths.push(screenshot_path);
        if let Some(env_name) = config.render_stats_env {
            if std::env::var_os(env_name).is_some() {
                log_render_stats(robot, step);
            }
        }
        if let (Some(diagnostic), Some(paths)) =
            (internal_diagnostic, internal_capture_paths.as_mut())
        {
            let screenshot = robot
                .screenshot_with_scale(diagnostic.capture_scale)
                .expect("internal screenshot");
            let internal_path = diagnostic
                .output_dir
                .join(format!("{}_step{step:02}.png", config.file_prefix));
            save_robot_screenshot(&internal_path, &screenshot);
            paths.push(internal_path);
        }
    }

    let compare_ok = run_compare_script(&capture_paths, crop, config);
    if let Some(paths) = internal_capture_paths.as_ref() {
        let internal_ok = run_compare_script(paths, crop, config);
        println!("internal diagnostic compare_ok={internal_ok}");
    }
    cleanup_capture_paths(&capture_paths);
    if let Some(paths) = internal_capture_paths.as_ref() {
        cleanup_capture_paths(paths);
    }
    compare_ok
}

fn scroll_once_and_expect_target_delta(
    robot: &cranpose::Robot,
    config: ScrollStabilityConfig,
    previous_bounds: (f32, f32, f32, f32),
    step: usize,
    label: &str,
) -> (f32, f32, f32, f32) {
    let anchor_x = previous_bounds.0 + previous_bounds.2 * 0.5;
    let anchor_y = previous_bounds.1 + previous_bounds.3 * 0.5;
    robot.mouse_move(anchor_x, anchor_y).expect("move cursor");
    std::thread::sleep(Duration::from_millis(30));
    robot
        .mouse_scroll(0.0, config.scroll_delta_y)
        .expect("1px scroll should succeed");
    std::thread::sleep(Duration::from_millis(150));
    let _ = robot.wait_for_idle();

    let current_bounds = find_text_in_semantics(robot, config.target_text)
        .unwrap_or_else(|| fail_with_semantics(robot, "target text must stay visible"));
    let actual_delta = current_bounds.1 - previous_bounds.1;
    println!(
        "{label} {step}: target_y={:.2} delta={:.2}",
        current_bounds.1, actual_delta
    );
    assert!(
        (actual_delta - config.scroll_delta_y).abs() <= config.step_epsilon,
        "expected exact scroll delta {:.3}, got delta_y={actual_delta:.3} at {label} {step}",
        config.scroll_delta_y,
    );
    current_bounds
}

#[allow(dead_code)]
pub(crate) fn prepare_internal_diagnostic(
    enable_env: &str,
    scale_env: &str,
    output_dir: PathBuf,
) -> Option<InternalDiagnostic> {
    std::env::var_os(enable_env)?;

    let output_dir = unique_process_dir(output_dir);
    std::fs::create_dir_all(&output_dir)
        .unwrap_or_else(|err| panic!("failed to create {}: {err}", output_dir.display()));
    let capture_scale = std::env::var(scale_env)
        .ok()
        .and_then(|value| value.parse::<f32>().ok())
        .filter(|value| value.is_finite() && *value > 0.0)
        .unwrap_or(1.0);
    Some(InternalDiagnostic {
        output_dir,
        capture_scale,
    })
}

fn compare_crop(robot: &cranpose::Robot, config: ScrollStabilityConfig) -> CompareCrop {
    if let Some(tag) = config.viewport_tag {
        if let Some(bounds) = wait_for_semantics_text_exact(robot, tag, 2000) {
            return compare_crop_from_bounds(bounds, config);
        }
        eprintln!("viewport tag '{tag}' not found; falling back to configured trims");
    }
    CompareCrop {
        trim_top_px: config.fallback_trim_top_px,
        trim_bottom_px: config.fallback_trim_bottom_px,
        trim_left_px: 0,
        trim_right_px: 0,
        logical_window_space: false,
    }
}

fn compare_crop_from_bounds(
    bounds: (f32, f32, f32, f32),
    config: ScrollStabilityConfig,
) -> CompareCrop {
    let inset = config.compare_viewport_inset_px as f32;
    let left = (bounds.0 + inset).ceil();
    let top = (bounds.1 + inset).ceil();
    let right = (config.window_width as f32 - (bounds.0 + bounds.2 - inset)).ceil();
    let bottom = (config.window_height as f32 - (bounds.1 + bounds.3 - inset)).ceil();
    CompareCrop {
        trim_top_px: clamp_crop_component(top, config.window_height),
        trim_bottom_px: clamp_crop_component(bottom, config.window_height),
        trim_left_px: clamp_crop_component(left, config.window_width),
        trim_right_px: clamp_crop_component(right, config.window_width),
        logical_window_space: true,
    }
}

fn clamp_crop_component(value: f32, max: u32) -> u32 {
    value.max(0.0).min(max as f32).round() as u32
}

fn wait_for_semantics_text_exact(
    robot: &cranpose::Robot,
    text: &str,
    timeout_ms: u64,
) -> Option<(f32, f32, f32, f32)> {
    let attempts = (timeout_ms / 100).max(1);
    for _ in 0..attempts {
        if let Some(bounds) = find_in_semantics(robot, |elem| find_text_exact_local(elem, text)) {
            return Some(bounds);
        }
        std::thread::sleep(Duration::from_millis(100));
        let _ = robot.wait_for_idle();
    }
    None
}

fn find_text_exact_local(
    elem: &cranpose::SemanticElement,
    text: &str,
) -> Option<(f32, f32, f32, f32)> {
    if elem.text.as_deref() == Some(text) {
        return Some((
            elem.bounds.x,
            elem.bounds.y,
            elem.bounds.width,
            elem.bounds.height,
        ));
    }
    for child in &elem.children {
        if let Some(bounds) = find_text_exact_local(child, text) {
            return Some(bounds);
        }
    }
    None
}

fn fail_with_semantics(robot: &cranpose::Robot, message: &str) -> ! {
    eprintln!("FATAL: {message}");
    if let Ok(semantics) = robot.get_semantics() {
        print_semantics_with_bounds(&semantics, 0);
    }
    let _ = robot.exit();
    std::process::exit(1);
}

fn prepare_named_output_dir(output_name: &str) -> PathBuf {
    let mut path = std::env::temp_dir();
    path.push("cranpose-scroll-stability");
    path.push(output_name);
    let path = unique_process_dir(path);
    std::fs::create_dir_all(&path)
        .unwrap_or_else(|err| panic!("failed to create {}: {err}", path.display()));
    path
}

fn unique_process_dir(mut path: PathBuf) -> PathBuf {
    let file_name = path
        .file_name()
        .and_then(|name| name.to_str())
        .expect("output path has a final component")
        .to_owned();
    path.set_file_name(format!("{file_name}-{}", std::process::id()));
    path
}

fn compare_script_path() -> PathBuf {
    let mut path = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    path.pop();
    path.pop();
    path.push("scripts");
    path.push("text_scroll_exact_external_compare.py");
    path
}

fn run_compare_script(
    capture_paths: &[PathBuf],
    crop: CompareCrop,
    config: ScrollStabilityConfig,
) -> bool {
    assert_eq!(
        capture_paths.len(),
        config.scroll_steps,
        "expected exactly {} screenshots",
        config.scroll_steps
    );

    let crop = crop.to_screenshot_pixels(&capture_paths[0], config);
    let script_path = compare_script_path();
    let output = Command::new("python3")
        .arg(&script_path)
        .arg("--trim-top")
        .arg(crop.trim_top_px.to_string())
        .arg("--trim-bottom")
        .arg(crop.trim_bottom_px.to_string())
        .arg("--trim-left")
        .arg(crop.trim_left_px.to_string())
        .arg("--trim-right")
        .arg(crop.trim_right_px.to_string())
        .arg("--max-offset")
        .arg(config.compare_search_offset_px.to_string())
        .arg("--max-adjacent-score")
        .arg(config.compare_max_adjacent_score.to_string())
        .arg("--stabilized-guard")
        .arg(config.compare_stabilized_guard_px.to_string())
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

impl CompareCrop {
    fn to_screenshot_pixels(
        self,
        first_capture_path: &Path,
        config: ScrollStabilityConfig,
    ) -> Self {
        if !self.logical_window_space {
            return self;
        }
        let (image_width, image_height) = image::image_dimensions(first_capture_path)
            .unwrap_or_else(|err| {
                panic!(
                    "failed to read {} dimensions: {err}",
                    first_capture_path.display()
                )
            });
        let scale_x = image_width as f32 / config.window_width.max(1) as f32;
        let scale_y = image_height as f32 / config.window_height.max(1) as f32;
        Self {
            trim_top_px: scale_crop_component(self.trim_top_px, scale_y, image_height),
            trim_bottom_px: scale_crop_component(self.trim_bottom_px, scale_y, image_height),
            trim_left_px: scale_crop_component(self.trim_left_px, scale_x, image_width),
            trim_right_px: scale_crop_component(self.trim_right_px, scale_x, image_width),
            logical_window_space: false,
        }
    }
}

fn scale_crop_component(value: u32, scale: f32, max: u32) -> u32 {
    ((value as f32) * scale).round().max(0.0).min(max as f32) as u32
}

fn cleanup_capture_paths(capture_paths: &[PathBuf]) {
    for path in capture_paths {
        if path.exists() {
            std::fs::remove_file(path)
                .unwrap_or_else(|err| panic!("failed to remove {}: {err}", path.display()));
        }
    }
}

fn save_robot_screenshot(path: &Path, screenshot: &cranpose::RobotScreenshot) {
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
