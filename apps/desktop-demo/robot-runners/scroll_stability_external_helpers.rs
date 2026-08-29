use std::{
    path::{Path, PathBuf},
    process::Command,
    time::Duration,
};

use cranpose_testing::{find_in_semantics, find_text_exact, print_semantics_with_bounds};
use image::{ImageBuffer, RgbaImage};

use crate::{
    output_paths,
    text_showcase_external_helpers::{find_window_id, take_x11_screenshot},
};

#[derive(Clone, Copy, Debug)]
pub(crate) struct ActiveFrameConfig {
    pub pump_frames_after_scroll: u32,
    pub settle_frames_after_capture: u32,
    pub max_capture_attempts: usize,
    pub sleep_between_attempts_ms: u64,
    pub min_improvement_ratio: f32,
}

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
    pub fallback_trim_left_px: u32,
    pub fallback_trim_right_px: u32,
    pub compare_search_offset_px: u32,
    pub compare_max_adjacent_score: u32,
    pub compare_max_channel_delta: u8,
    pub compare_stabilized_guard_px: u32,
    pub compare_viewport_inset_px: u32,
    pub render_stats_env: Option<&'static str>,
    pub active_frame: Option<ActiveFrameConfig>,
}

#[derive(Clone, Copy, Debug)]
#[allow(dead_code)]
pub(crate) struct ExactScrollStepConfig {
    pub target_text: &'static str,
    pub window_width: u32,
    pub window_height: u32,
    pub scroll_steps: usize,
    pub scroll_delta_y: f32,
    pub step_epsilon: f32,
    pub fallback_trim_top_px: u32,
    pub fallback_trim_bottom_px: u32,
}

impl ScrollStabilityConfig {
    fn exact_step_config(self) -> ExactScrollStepConfig {
        ExactScrollStepConfig {
            target_text: self.target_text,
            window_width: self.window_width,
            window_height: self.window_height,
            scroll_steps: self.scroll_steps,
            scroll_delta_y: self.scroll_delta_y,
            step_epsilon: self.step_epsilon,
            fallback_trim_top_px: self.fallback_trim_top_px,
            fallback_trim_bottom_px: self.fallback_trim_bottom_px,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum ScrollStepDriver<'a> {
    PointerWheel,
    #[allow(dead_code)]
    AppHook(&'a str),
}

#[derive(Clone, Copy, Debug)]
pub(crate) struct CompareCrop {
    pub(crate) trim_top_px: u32,
    pub(crate) trim_bottom_px: u32,
    pub(crate) trim_left_px: u32,
    pub(crate) trim_right_px: u32,
    pub(crate) logical_window_space: bool,
}

#[derive(Debug)]
#[allow(dead_code)]
pub(crate) struct InternalDiagnostic {
    pub output_dir: PathBuf,
    pub capture_scale: f32,
}

#[allow(dead_code)]
pub(crate) fn run_scroll_stability_capture(
    robot: &cranpose::Robot,
    config: ScrollStabilityConfig,
    internal_diagnostic: Option<&InternalDiagnostic>,
) -> bool {
    run_scroll_stability_capture_with_driver(
        robot,
        config,
        internal_diagnostic,
        ScrollStepDriver::PointerWheel,
    )
}

#[allow(dead_code)]
pub(crate) fn run_scroll_stability_capture_with_app_hook(
    robot: &cranpose::Robot,
    config: ScrollStabilityConfig,
    internal_diagnostic: Option<&InternalDiagnostic>,
    hook_name: &str,
) -> bool {
    assert!(
        config.active_frame.is_none(),
        "app-hook scrolling does not support active-frame capture"
    );
    run_scroll_stability_capture_with_driver(
        robot,
        config,
        internal_diagnostic,
        ScrollStepDriver::AppHook(hook_name),
    )
}

#[allow(dead_code)]
pub(crate) fn run_presented_scroll_probe_with_app_hook(
    robot: &cranpose::Robot,
    config: ScrollStabilityConfig,
    hook_name: &str,
    mut inspect_frame: impl FnMut(usize, &Path) -> bool,
) -> bool {
    assert!(
        config.scroll_steps >= 2,
        "presented scroll probe needs at least two steps"
    );
    let output_dir = prepare_named_output_dir(config.output_name);
    println!("Output dir: {}", output_dir.display());
    let window_id = find_window_id(config.window_title);
    let mut previous_bounds = find_exact_target_in_semantics(robot, config.target_text)
        .unwrap_or_else(|| fail_with_semantics(robot, "target text must be visible"));
    let mut capture_paths = Vec::with_capacity(config.scroll_steps);
    let mut passed = true;

    for step in 0..config.scroll_steps {
        previous_bounds = scroll_once_and_expect_target_delta(
            robot,
            config.exact_step_config(),
            previous_bounds,
            step,
            "probe step",
            ScrollStepDriver::AppHook(hook_name),
        );
        let screenshot_path = output_dir.join(format!("{}_step{step:02}.png", config.file_prefix));
        take_x11_screenshot(&window_id, screenshot_path.to_str().expect("utf8 path"));
        passed &= inspect_frame(step, &screenshot_path);
        capture_paths.push(screenshot_path);
    }

    if passed {
        cleanup_capture_paths(&capture_paths);
    } else {
        eprintln!(
            "presented scroll probe failed; captures retained in {}",
            output_dir.display()
        );
    }
    passed
}

#[allow(dead_code)]
pub(crate) fn semantics_bounds_for_exact_text(
    robot: &cranpose::Robot,
    text: &str,
) -> (f32, f32, f32, f32) {
    wait_for_semantics_text_exact(robot, text, 2_000)
        .unwrap_or_else(|| fail_with_semantics(robot, "semantic bounds must be visible"))
}

#[allow(dead_code)]
pub(crate) fn advance_scroll_with_app_hook(
    robot: &cranpose::Robot,
    config: ExactScrollStepConfig,
    hook_name: &str,
) {
    assert!(
        config.scroll_steps > 0,
        "exact scroll advance needs at least one step"
    );
    let mut previous_bounds = find_exact_target_in_semantics(robot, config.target_text)
        .unwrap_or_else(|| fail_with_semantics(robot, "target text must be visible"));

    for step in 0..config.scroll_steps {
        previous_bounds = scroll_once_and_expect_target_delta(
            robot,
            config,
            previous_bounds,
            step,
            "phase step",
            ScrollStepDriver::AppHook(hook_name),
        );
    }
}

fn run_scroll_stability_capture_with_driver(
    robot: &cranpose::Robot,
    config: ScrollStabilityConfig,
    internal_diagnostic: Option<&InternalDiagnostic>,
    scroll_driver: ScrollStepDriver<'_>,
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

    let mut previous_bounds = find_exact_target_in_semantics(robot, config.target_text)
        .unwrap_or_else(|| fail_with_semantics(robot, "target text must be visible"));

    let mut capture_paths = Vec::with_capacity(config.scroll_steps);
    let mut active_capture_paths = Vec::new();
    let mut internal_capture_paths = internal_diagnostic
        .as_ref()
        .map(|_| Vec::with_capacity(config.scroll_steps));
    let mut previous_presented = if config.active_frame.is_some() {
        let path = output_dir.join(format!("{}_active_base.png", config.file_prefix));
        let screenshot = capture_x11_window_screenshot(
            &window_id,
            &path,
            config.window_width,
            config.window_height,
        );
        active_capture_paths.push(path);
        Some(screenshot)
    } else {
        None
    };

    for step in 0..config.scroll_steps {
        if let Some(active_config) = config.active_frame {
            let previous_screenshot = previous_presented
                .as_ref()
                .expect("active frame capture has a previous screenshot");
            let active = ActiveFrameRun {
                robot,
                config,
                active_config,
                window_id: &window_id,
                output_dir: &output_dir,
                crop,
            }
            .scroll_once(previous_screenshot, previous_bounds, step);
            previous_bounds = active.bounds;
            capture_paths.push(active.settled_path);
            active_capture_paths.extend(active.active_paths);
            previous_presented = Some(active.settled_screenshot);
        } else {
            previous_bounds = scroll_once_and_expect_target_delta(
                robot,
                config.exact_step_config(),
                previous_bounds,
                step,
                "step",
                scroll_driver,
            );

            let screenshot_path =
                output_dir.join(format!("{}_step{step:02}.png", config.file_prefix));
            take_x11_screenshot(&window_id, screenshot_path.to_str().expect("utf8 path"));
            capture_paths.push(screenshot_path);
        }
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
    cleanup_capture_paths(&active_capture_paths);
    if let Some(paths) = internal_capture_paths.as_ref() {
        cleanup_capture_paths(paths);
    }
    compare_ok
}

#[allow(dead_code)]
pub(crate) fn run_internal_scroll_stability_capture(
    robot: &cranpose::Robot,
    config: ScrollStabilityConfig,
    capture_scale: f32,
) -> bool {
    assert!(
        config.scroll_steps >= 2,
        "scroll stability capture needs at least two steps"
    );
    assert!(
        capture_scale.is_finite() && capture_scale > 0.0,
        "capture_scale must be positive and finite"
    );

    let output_dir = prepare_named_output_dir(config.output_name);
    println!("Window title: {}", config.window_title);
    println!("Output dir: {}", output_dir.display());
    let crop = compare_crop(robot, config);
    println!(
        "compare crop: left={} right={} top={} bottom={}",
        crop.trim_left_px, crop.trim_right_px, crop.trim_top_px, crop.trim_bottom_px
    );

    let mut previous_bounds = find_exact_target_in_semantics(robot, config.target_text)
        .unwrap_or_else(|| fail_with_semantics(robot, "target text must be visible"));

    let mut capture_paths = Vec::with_capacity(config.scroll_steps);
    for step in 0..config.scroll_steps {
        previous_bounds = scroll_once_and_expect_target_delta(
            robot,
            config.exact_step_config(),
            previous_bounds,
            step,
            "step",
            ScrollStepDriver::PointerWheel,
        );

        let screenshot = robot
            .screenshot_with_scale(capture_scale)
            .expect("internal screenshot");
        let screenshot_path = output_dir.join(format!("{}_step{step:02}.png", config.file_prefix));
        save_robot_screenshot(&screenshot_path, &screenshot);
        capture_paths.push(screenshot_path);

        if let Some(env_name) = config.render_stats_env {
            if std::env::var_os(env_name).is_some() {
                log_render_stats(robot, step);
            }
        }
    }

    let compare_ok = run_compare_script(&capture_paths, crop, config);
    cleanup_capture_paths(&capture_paths);
    compare_ok
}

pub(crate) fn scroll_once_and_expect_target_delta(
    robot: &cranpose::Robot,
    config: ExactScrollStepConfig,
    previous_bounds: (f32, f32, f32, f32),
    step: usize,
    label: &str,
    scroll_driver: ScrollStepDriver<'_>,
) -> (f32, f32, f32, f32) {
    let anchor_x =
        (previous_bounds.0 + previous_bounds.2 * 0.5).clamp(1.0, config.window_width as f32 - 1.0);
    let anchor_center_y = previous_bounds.1 + previous_bounds.3 * 0.5;
    let anchor_min_y = config.fallback_trim_top_px as f32 + 1.0;
    let anchor_max_y = config.window_height as f32 - config.fallback_trim_bottom_px as f32 - 1.0;
    let anchor_y = if anchor_min_y < anchor_max_y {
        anchor_center_y.clamp(anchor_min_y, anchor_max_y)
    } else {
        anchor_center_y.clamp(1.0, config.window_height as f32 - 1.0)
    };
    match scroll_driver {
        ScrollStepDriver::PointerWheel => {
            robot.mouse_move(anchor_x, anchor_y).expect("move cursor");
            std::thread::sleep(Duration::from_millis(30));
            robot
                .mouse_scroll(0.0, config.scroll_delta_y)
                .expect("1px scroll should succeed");
        }
        ScrollStepDriver::AppHook(hook_name) => {
            let delta = config.scroll_delta_y.to_string();
            let result = robot
                .invoke_app_hook(hook_name, &delta)
                .unwrap_or_else(|err| panic!("exact scroll hook '{hook_name}' failed: {err}"));
            println!("{label} {step}: exact scroll hook result={result:?}");
        }
    }
    std::thread::sleep(Duration::from_millis(150));
    let _ = robot.wait_for_idle();

    let current_bounds = find_exact_target_in_semantics(robot, config.target_text)
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

#[derive(Debug)]
struct ActiveFrameCapture {
    bounds: (f32, f32, f32, f32),
    settled_screenshot: cranpose::RobotScreenshot,
    settled_path: PathBuf,
    active_paths: Vec<PathBuf>,
}

struct ActiveFrameRun<'a> {
    robot: &'a cranpose::Robot,
    config: ScrollStabilityConfig,
    active_config: ActiveFrameConfig,
    window_id: &'a str,
    output_dir: &'a Path,
    crop: CompareCrop,
}

impl ActiveFrameRun<'_> {
    fn scroll_once(
        &self,
        previous_screenshot: &cranpose::RobotScreenshot,
        previous_bounds: (f32, f32, f32, f32),
        step: usize,
    ) -> ActiveFrameCapture {
        let config = self.config;
        let active_config = self.active_config;
        let anchor_x = (previous_bounds.0 + previous_bounds.2 * 0.5)
            .clamp(1.0, config.window_width as f32 - 1.0);
        let anchor_center_y = previous_bounds.1 + previous_bounds.3 * 0.5;
        let anchor_min_y = config.fallback_trim_top_px as f32 + 1.0;
        let anchor_max_y =
            config.window_height as f32 - config.fallback_trim_bottom_px as f32 - 1.0;
        let anchor_y = if anchor_min_y < anchor_max_y {
            anchor_center_y.clamp(anchor_min_y, anchor_max_y)
        } else {
            anchor_center_y.clamp(1.0, config.window_height as f32 - 1.0)
        };
        self.robot
            .mouse_move(anchor_x, anchor_y)
            .expect("move cursor");
        std::thread::sleep(Duration::from_millis(30));
        self.robot
            .mouse_scroll(0.0, config.scroll_delta_y)
            .expect("1px scroll should succeed");

        let mut active_paths = Vec::new();
        let mut moved = None;
        let attempts = active_config.max_capture_attempts.max(1);
        for attempt in 0..attempts {
            if attempt == 0 {
                self.robot
                    .pump_frames(active_config.pump_frames_after_scroll)
                    .expect("active scroll frame pump should succeed");
            } else {
                self.robot
                    .pump_frames(1)
                    .expect("follow-up active scroll frame pump should succeed");
                std::thread::sleep(Duration::from_millis(
                    active_config.sleep_between_attempts_ms,
                ));
            }

            let path = self.output_dir.join(format!(
                "{}_active_step{step:02}_attempt{attempt:02}.png",
                config.file_prefix
            ));
            let active_screenshot = capture_x11_window_screenshot(
                self.window_id,
                &path,
                config.window_width,
                config.window_height,
            );
            let report = active_frame_movement_report(
                previous_screenshot,
                &active_screenshot,
                self.crop,
                config,
                active_config,
            );
            println!(
                "active_frame step={step} attempt={attempt} expected_dy_px={} zero_score={} expected_score={} improvement_ratio={:.4}",
                report.expected_dy_px,
                report.zero_score,
                report.expected_score,
                report.improvement_ratio,
            );
            active_paths.push(path);
            if report.moved {
                moved = Some((active_screenshot, report));
                break;
            }
            moved = Some((active_screenshot, report));
        }

        let Some((_active_screenshot, report)) = moved else {
            unreachable!("active frame capture always runs at least one attempt");
        };
        if !report.moved {
            fail_with_semantics(
                self.robot,
                &format!(
                    "presented Markdown frame did not move after active scroll step {step}: expected_dy_px={} zero_score={} expected_score={} improvement_ratio={:.4}",
                    report.expected_dy_px,
                    report.zero_score,
                    report.expected_score,
                    report.improvement_ratio,
                ),
            );
        }

        if active_config.settle_frames_after_capture > 0 {
            self.robot
                .pump_frames(active_config.settle_frames_after_capture)
                .expect("settle frame pump should succeed");
        }
        std::thread::sleep(Duration::from_millis(150));
        let _ = self.robot.wait_for_idle();

        let current_bounds = find_exact_target_in_semantics(self.robot, config.target_text)
            .unwrap_or_else(|| fail_with_semantics(self.robot, "target text must stay visible"));
        let actual_delta = current_bounds.1 - previous_bounds.1;
        println!(
            "step {step}: target_y={:.2} delta={:.2}",
            current_bounds.1, actual_delta
        );
        assert!(
            (actual_delta - config.scroll_delta_y).abs() <= config.step_epsilon,
            "expected exact scroll delta {:.3}, got delta_y={actual_delta:.3} at step {step}",
            config.scroll_delta_y,
        );

        let settled_path = self
            .output_dir
            .join(format!("{}_step{step:02}.png", config.file_prefix));
        let settled_screenshot = capture_x11_window_screenshot(
            self.window_id,
            &settled_path,
            config.window_width,
            config.window_height,
        );

        ActiveFrameCapture {
            bounds: current_bounds,
            settled_screenshot,
            settled_path,
            active_paths,
        }
    }
}

#[derive(Clone, Copy, Debug)]
struct ActiveFrameMovementReport {
    expected_dy_px: i32,
    zero_score: u64,
    expected_score: u64,
    improvement_ratio: f32,
    moved: bool,
}

fn active_frame_movement_report(
    previous: &cranpose::RobotScreenshot,
    active: &cranpose::RobotScreenshot,
    crop: CompareCrop,
    config: ScrollStabilityConfig,
    active_config: ActiveFrameConfig,
) -> ActiveFrameMovementReport {
    assert_eq!(previous.width, active.width, "active capture width changed");
    assert_eq!(
        previous.height, active.height,
        "active capture height changed"
    );
    let scale_y = active.height as f32 / config.window_height.max(1) as f32;
    let expected_dy_px = (-config.scroll_delta_y * scale_y).round() as i32;
    assert_ne!(
        expected_dy_px, 0,
        "active frame movement needs a non-zero physical scroll delta"
    );
    let zero_score = vertical_alignment_score(previous, active, crop, 0)
        .expect("zero active-frame alignment crop should be valid");
    let expected_score = vertical_alignment_score(previous, active, crop, expected_dy_px)
        .expect("expected active-frame alignment crop should be valid");
    let improvement_ratio = if zero_score == 0 {
        0.0
    } else {
        zero_score.saturating_sub(expected_score) as f32 / zero_score as f32
    };
    let moved =
        expected_score < zero_score && improvement_ratio >= active_config.min_improvement_ratio;
    ActiveFrameMovementReport {
        expected_dy_px,
        zero_score,
        expected_score,
        improvement_ratio,
        moved,
    }
}

fn vertical_alignment_score(
    reference: &cranpose::RobotScreenshot,
    candidate: &cranpose::RobotScreenshot,
    crop: CompareCrop,
    dy: i32,
) -> Option<u64> {
    if reference.width != candidate.width || reference.height != candidate.height {
        return None;
    }
    let width = reference.width as i32;
    let height = reference.height as i32;
    let left = crop.trim_left_px as i32;
    let right = width - crop.trim_right_px as i32;
    let (ref_top, ref_bottom, cand_top, cand_bottom) = if dy >= 0 {
        (
            crop.trim_top_px as i32 + dy,
            height - crop.trim_bottom_px as i32,
            crop.trim_top_px as i32,
            height - crop.trim_bottom_px as i32 - dy,
        )
    } else {
        let shift = -dy;
        (
            crop.trim_top_px as i32,
            height - crop.trim_bottom_px as i32 - shift,
            crop.trim_top_px as i32 + shift,
            height - crop.trim_bottom_px as i32,
        )
    };
    if left < 0
        || right <= left
        || ref_top < 0
        || cand_top < 0
        || ref_bottom <= ref_top
        || cand_bottom <= cand_top
        || ref_bottom > height
        || cand_bottom > height
        || ref_bottom - ref_top != cand_bottom - cand_top
    {
        return None;
    }

    let width_usize = reference.width as usize;
    let mut score = 0u64;
    for y in 0..(ref_bottom - ref_top) as usize {
        let ref_y = ref_top as usize + y;
        let cand_y = cand_top as usize + y;
        for x in left as usize..right as usize {
            let ref_index = (ref_y * width_usize + x) * 4;
            let cand_index = (cand_y * width_usize + x) * 4;
            for channel in 0..4 {
                score = score.saturating_add(
                    reference.pixels[ref_index + channel]
                        .abs_diff(candidate.pixels[cand_index + channel])
                        as u64,
                );
            }
        }
    }
    Some(score)
}

fn capture_x11_window_screenshot(
    window_id: &str,
    path: &Path,
    logical_width: u32,
    logical_height: u32,
) -> cranpose::RobotScreenshot {
    take_x11_screenshot(window_id, path.to_str().expect("utf8 path"));
    let image = image::open(path)
        .unwrap_or_else(|err| panic!("failed to read X11 screenshot {}: {err}", path.display()))
        .to_rgba8();
    cranpose::RobotScreenshot {
        width: image.width(),
        height: image.height(),
        logical_width: logical_width as f32,
        logical_height: logical_height as f32,
        pixels: image.into_raw(),
    }
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
        trim_left_px: config.fallback_trim_left_px,
        trim_right_px: config.fallback_trim_right_px,
        logical_window_space: false,
    }
}

fn compare_crop_from_bounds(
    bounds: (f32, f32, f32, f32),
    config: ScrollStabilityConfig,
) -> CompareCrop {
    let inset = config.compare_viewport_inset_px as f32;
    let viewport_left = bounds.0.max(0.0);
    let viewport_top = bounds.1.max(0.0);
    let viewport_right = (bounds.0 + bounds.2).min(config.window_width as f32);
    let viewport_bottom = (bounds.1 + bounds.3).min(config.window_height as f32);
    let left = (viewport_left + inset).ceil();
    let top = (viewport_top + inset).ceil();
    let right = (config.window_width as f32 - (viewport_right - inset)).ceil();
    let bottom = (config.window_height as f32 - (viewport_bottom - inset)).ceil();
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

fn find_exact_target_in_semantics(
    robot: &cranpose::Robot,
    text: &str,
) -> Option<(f32, f32, f32, f32)> {
    find_in_semantics(robot, |elem| find_text_exact(elem, text))
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
    let path = unique_process_dir(
        output_paths::diagnostic_path("cranpose-scroll-stability").join(output_name),
    );
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

pub(crate) fn run_compare_script(
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
        .arg("--max-channel-delta")
        .arg(config.compare_max_channel_delta.to_string())
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

#[cfg(test)]
mod tests {
    use super::{
        active_frame_movement_report, ActiveFrameConfig, CompareCrop, ScrollStabilityConfig,
    };

    fn screenshot_from_row_values(row_values: &[u8], width: u32) -> cranpose::RobotScreenshot {
        let mut pixels = Vec::with_capacity(row_values.len() * width as usize * 4);
        for value in row_values {
            for _ in 0..width {
                pixels.extend_from_slice(&[*value, 0, 0, 255]);
            }
        }
        cranpose::RobotScreenshot {
            width,
            height: row_values.len() as u32,
            logical_width: width as f32,
            logical_height: row_values.len() as f32,
            pixels,
        }
    }

    fn test_config(scroll_delta_y: f32) -> ScrollStabilityConfig {
        ScrollStabilityConfig {
            window_title: "test",
            output_name: "test",
            file_prefix: "test",
            target_text: "target",
            viewport_tag: None,
            window_width: 3,
            window_height: 4,
            scroll_steps: 2,
            scroll_delta_y,
            step_epsilon: 0.05,
            fallback_trim_top_px: 0,
            fallback_trim_bottom_px: 0,
            fallback_trim_left_px: 0,
            fallback_trim_right_px: 0,
            compare_search_offset_px: 4,
            compare_max_adjacent_score: 0,
            compare_max_channel_delta: 0,
            compare_stabilized_guard_px: 0,
            compare_viewport_inset_px: 0,
            render_stats_env: None,
            active_frame: None,
        }
    }

    #[test]
    fn active_frame_report_uses_inverse_scroll_delta_to_align_moved_content() {
        let previous = screenshot_from_row_values(&[10, 20, 30, 40], 3);
        let candidate_moved_up = screenshot_from_row_values(&[20, 30, 40, 40], 3);
        let crop = CompareCrop {
            trim_top_px: 0,
            trim_bottom_px: 0,
            trim_left_px: 0,
            trim_right_px: 0,
            logical_window_space: false,
        };
        let report = active_frame_movement_report(
            &previous,
            &candidate_moved_up,
            crop,
            test_config(-1.0),
            ActiveFrameConfig {
                pump_frames_after_scroll: 1,
                settle_frames_after_capture: 0,
                max_capture_attempts: 1,
                sleep_between_attempts_ms: 0,
                min_improvement_ratio: 0.05,
            },
        );

        assert_eq!(report.expected_dy_px, 1);
        assert_eq!(report.expected_score, 0);
        assert!(report.zero_score > report.expected_score);
        assert!(report.moved);
    }
}
