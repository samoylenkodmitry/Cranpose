//! Robot test: the glass "Library" top bar's blurred backdrop, on the
//! Receipts feed, must shift by the SAME per-step delta as the scrolling
//! list behind it — not merely "change".
//!
//! A "did the glass change at all" check is trivially satisfied even by a
//! backdrop that moves by the wrong amount, moves the wrong way, or updates
//! to garbage — the same weakness as a pass-count check on a blank nav bar.
//! The user's own word for the failure mode this test exists to catch is
//! "caterpillar": scrolling one logical pixel at a time, the blur lags,
//! then jumps to catch up, over and over. A test that only checks
//! cumulative movement across the whole run would pass on exactly that
//! pattern, so every step is asserted individually.
//!
//! This reuses the existing scroll-stability machinery
//! (`scroll_stability_external_helpers`, already trusted by
//! `robot_leetcodedaily_code_scroll_pixel_drift` and the `_exact_external_contract`
//! family) rather than inventing a second shift-search implementation:
//! `scroll_once_and_expect_target_delta` drives each one-pixel step and
//! confirms, via semantics on a receipt subtitle, that the CONTENT actually
//! moved by exactly that pixel — the ground truth this test's assertion is
//! measured against. `run_compare_script` (the same
//! `scripts/text_scroll_exact_external_compare.py` engine every
//! `_exact_external_contract` test already runs) then searches, for every
//! captured frame against the first, the vertical shift that best aligns it
//! — the identical "small search over candidate offsets minimising absolute
//! difference" `robot_scroll_decoration_invariance` and the horizontal
//! variant in `robot_leetcodedaily_full_layout_scroll_stability` already
//! use, just pointed at the glass bar's interior instead of a content
//! viewport. This file only adds what does not already exist anywhere:
//! comparing that engine's own per-step output against the confirmed
//! content delta and naming which of three ways it disagrees.
//!
//! NOT headless. NOT a renderer screenshot: real X11 window capture via
//! `text_showcase_external_helpers::capture_x11_window_screenshot`, because
//! a renderer screenshot renders into a fresh offscreen texture every call
//! and never touches `redraw_native_window`'s real, reused swapchain view.
//! Run under software present (`just robot-captures`) — the GPU-under-Xvfb
//! path cannot present a window at all (see `TIME_WASTERS.md`).

mod output_paths;
mod scroll_stability_external_helpers;
mod text_showcase_external_helpers;

use std::time::Duration;

use cranpose::AppLauncher;
use cranpose_testing::{find_in_semantics, find_text_exact};
use desktop_app::app;
use scroll_stability_external_helpers::{
    run_compare_script, scroll_once_and_expect_target_delta, CompareCrop, ExactScrollStepConfig,
    ScrollStabilityConfig, ScrollStepDriver,
};
use text_showcase_external_helpers::{capture_x11_window_screenshot, find_window_id};

/// The demo's normal window — do not expand it to reach the Receipts tab.
const WINDOW_WIDTH: u32 = 800;
const WINDOW_HEIGHT: u32 = 632;
const WINDOW_TITLE: &str = "Robot Glass Backdrop Scroll Stability";
/// Deep enough that the sampled chrome sits over real card content, not the
/// LazyColumn's still-empty top content padding (a false positive already
/// caught once in this investigation — see TIME_WASTERS.md).
const SETTLE_SCROLL: f32 = -400.0;
const SCROLL_DELTA_Y: f32 = -1.0;
const STEP_COUNT: usize = 20;
const STEP_EPSILON: f32 = 0.05;
/// Consecutive scroll events accumulate velocity under the current fling
/// physics, so back-to-back one-pixel input events do not produce
/// one-pixel steps: measured per-step diffs growing 18k to 77k until
/// settling 1.6s between events. `scroll_once_and_expect_target_delta`
/// already sleeps 150ms and re-checks the exact delta via semantics; this
/// adds the rest of the margin on top without touching that shared function.
const SETTLE_AFTER_SCROLL_EXTRA_MS: u64 = 1_500;
/// Search radius for the reused shift-search engine. Must comfortably
/// exceed the final step's expected cumulative shift (`STEP_COUNT` px).
const COMPARE_SEARCH_OFFSET_PX: u32 = 32;
/// Small: the sampled glass patch is only ~90px tall, and the reused
/// engine's fractional-alignment guard grows with the largest measured
/// shift (up to ~`STEP_COUNT` px here), which must still leave a positive
/// crop height.
const COMPARE_STABILIZED_GUARD_PX: u32 = 4;
/// Interior of the "Library" glass `TopBar`
/// (`apps/desktop-demo/src/app/glass_feed.rs`), clear of both the
/// "Library" text (left) and the round icon button (right). Verified by
/// eye against this run's own `glass_step00.png` before trusting any
/// number from it — sampling the wrong rectangle has cost this
/// investigation twice already (see TIME_WASTERS.md).
const GLASS_REGION: (f32, f32, f32, f32) = (150.0, 100.0, 500.0, 90.0);
/// A best-fit shift within this many pixels of the expected cumulative
/// shift counts as tracking correctly. Must be well under 1 step's worth
/// of motion or it cannot tell "frozen" from "on time" at step 1.
const SHIFT_TOLERANCE_PX: i32 = 1;

fn fail(robot: &cranpose::Robot, message: &str) -> ! {
    println!("FATAL: {message}");
    let _ = robot.exit();
    std::process::exit(1);
}

fn main() {
    env_logger::init();
    println!("=== Robot Glass Backdrop Scroll Stability ===");
    let output_dir = output_paths::diagnostic_path(&format!(
        "cranpose_glass_backdrop_scroll_stability-{}",
        std::process::id()
    ));
    println!("Output dir: {}", output_dir.display());
    std::fs::create_dir_all(&output_dir).expect("create output dir");

    AppLauncher::new()
        .with_title(WINDOW_TITLE)
        .with_size(WINDOW_WIDTH, WINDOW_HEIGHT)
        .with_headless(false)
        .with_test_driver(move |robot| {
            std::thread::sleep(Duration::from_millis(1_000));
            let _ = robot.wait_for_idle();

            open_receipts_tab(&robot);
            std::thread::sleep(Duration::from_millis(400));
            let _ = robot.wait_for_idle();

            robot
                .mouse_move(WINDOW_WIDTH as f32 * 0.5, WINDOW_HEIGHT as f32 * 0.7)
                .expect("move cursor over list");
            std::thread::sleep(Duration::from_millis(30));
            robot
                .mouse_scroll_and_wait_for_frame(0.0, SETTLE_SCROLL)
                .expect("settle scroll past empty content padding");
            std::thread::sleep(Duration::from_millis(500));
            let _ = robot.wait_for_idle();

            // A receipt subtitle ("Receipt #NNNN — 12 items") is unique per
            // row, unlike the six recycled card titles, so once discovered
            // it is an unambiguous anchor for the rest of the run.
            let (_, _, _, _, anchor_text) = robot
                .find_text_by_prefix("Receipt #")
                .expect("query receipt subtitle")
                .unwrap_or_else(|| fail(&robot, "no receipt subtitle visible after settle scroll"));
            println!("tracking content anchor: {anchor_text:?}");
            let anchor_text: &'static str = Box::leak(anchor_text.into_boxed_str());

            let mut previous_bounds = find_in_semantics(&robot, |elem| {
                find_text_exact(elem, anchor_text)
            })
            .unwrap_or_else(|| fail(&robot, "content anchor should be visible after settle"));

            let window_id = find_window_id(WINDOW_TITLE);

            let step_config = ExactScrollStepConfig {
                target_text: anchor_text,
                window_width: WINDOW_WIDTH,
                window_height: WINDOW_HEIGHT,
                scroll_steps: STEP_COUNT,
                scroll_delta_y: SCROLL_DELTA_Y,
                step_epsilon: STEP_EPSILON,
                fallback_trim_top_px: GLASS_REGION.1.round() as u32,
                fallback_trim_bottom_px: (WINDOW_HEIGHT as f32
                    - (GLASS_REGION.1 + GLASS_REGION.3))
                    .round() as u32,
            };

            let mut capture_paths = Vec::with_capacity(STEP_COUNT + 1);
            let baseline_path = output_dir.join("glass_step00.png");
            capture_x11_window_screenshot(
                &window_id,
                &baseline_path,
                WINDOW_WIDTH as f32,
                WINDOW_HEIGHT as f32,
            );
            capture_paths.push(baseline_path);

            for step in 0..STEP_COUNT {
                previous_bounds = scroll_once_and_expect_target_delta(
                    &robot,
                    step_config,
                    previous_bounds,
                    step,
                    "glass-step",
                    ScrollStepDriver::PointerWheel,
                );
                std::thread::sleep(Duration::from_millis(SETTLE_AFTER_SCROLL_EXTRA_MS));
                let _ = robot.wait_for_idle();

                let step_path = output_dir.join(format!("glass_step{:02}.png", step + 1));
                capture_x11_window_screenshot(
                    &window_id,
                    &step_path,
                    WINDOW_WIDTH as f32,
                    WINDOW_HEIGHT as f32,
                );
                capture_paths.push(step_path);
            }

            let crop = CompareCrop {
                trim_top_px: GLASS_REGION.1.round() as u32,
                trim_bottom_px: (WINDOW_HEIGHT as f32 - (GLASS_REGION.1 + GLASS_REGION.3))
                    .round() as u32,
                trim_left_px: GLASS_REGION.0.round() as u32,
                trim_right_px: (WINDOW_WIDTH as f32 - (GLASS_REGION.0 + GLASS_REGION.2)).round()
                    as u32,
                logical_window_space: true,
            };
            let compare_config = ScrollStabilityConfig {
                window_title: WINDOW_TITLE,
                output_name: "glass_backdrop_scroll_stability",
                file_prefix: "glass",
                target_text: anchor_text,
                viewport_tag: None,
                window_width: WINDOW_WIDTH,
                window_height: WINDOW_HEIGHT,
                scroll_steps: capture_paths.len(),
                scroll_delta_y: SCROLL_DELTA_Y,
                step_epsilon: STEP_EPSILON,
                fallback_trim_top_px: crop.trim_top_px,
                fallback_trim_bottom_px: crop.trim_bottom_px,
                fallback_trim_left_px: crop.trim_left_px,
                fallback_trim_right_px: crop.trim_right_px,
                compare_search_offset_px: COMPARE_SEARCH_OFFSET_PX,
                compare_max_adjacent_score: u32::MAX,
                compare_max_channel_delta: 0,
                compare_stabilized_guard_px: COMPARE_STABILIZED_GUARD_PX,
                compare_viewport_inset_px: 0,
                render_stats_env: None,
                active_frame: None,
            };
            // Discarded on purpose: this call's own pass/fail is a
            // self-consistency check (do frames agree with each other once
            // each is aligned by its OWN best-fit shift), which cannot tell
            // a frozen backdrop (trivially self-consistent at shift=0) from
            // a correct one. The verdict this test actually needs comes
            // from comparing the shift itself against the confirmed
            // content delta below.
            let _ = run_compare_script(&capture_paths, crop, compare_config);

            let report_path = output_dir.join("fractional_alignment_report.txt");
            let report = std::fs::read_to_string(&report_path).unwrap_or_else(|err| {
                fail(
                    &robot,
                    &format!("compare script did not write {report_path:?}: {err}"),
                )
            });
            let anchor_dys = parse_anchor_dys(&report, capture_paths.len());

            let mut failures = Vec::new();
            for (step, &actual) in anchor_dys.iter().enumerate().skip(1) {
                let expected = (step as f32 * SCROLL_DELTA_Y.abs()).round() as i32;
                let mode = classify(actual, expected);
                println!(
                    "step {step:02}: expected_shift={expected} best_fit_shift={actual}{}",
                    mode.map(|m| format!(" MODE={m}")).unwrap_or_default()
                );
                if let Some(mode) = mode {
                    failures.push(format!(
                        "step {step:02}: {mode} (expected_shift={expected} best_fit_shift={actual})"
                    ));
                }
            }

            for path in &capture_paths {
                let _ = std::fs::remove_file(path);
            }

            if !failures.is_empty() {
                fail(
                    &robot,
                    &format!(
                        "glass backdrop did not track the content's confirmed scroll:\n{}\ncaptures retained in {}",
                        failures.join("\n"),
                        output_dir.display()
                    ),
                );
            }

            println!(
                "PASS: glass backdrop's best-fit shift matched the content's confirmed scroll on every one of {STEP_COUNT} one-pixel steps"
            );
            robot.exit().expect("exit");
        })
        .run(app::combined_app);
}

/// `frozen`: the backdrop did not move while the content (confirmed via
/// semantics) did. `lagging`: it moved, the right way, but not enough yet —
/// the user's "caterpillar" symptom, a per-step shortfall that later steps
/// may or may not catch up on. `wrong`: it moved further than tracking
/// explains, or the wrong way entirely.
fn classify(actual: i32, expected: i32) -> Option<&'static str> {
    if expected == 0 {
        return None;
    }
    if actual == 0 {
        return Some("frozen");
    }
    if actual.signum() == expected.signum() && actual.abs() < expected.abs() - SHIFT_TOLERANCE_PX {
        return Some("lagging");
    }
    if (actual - expected).abs() > SHIFT_TOLERANCE_PX {
        return Some("wrong");
    }
    None
}

/// Parse `step=00->NN anchor_dy=D ...` lines from the reused compare
/// script's `fractional_alignment_report.txt`, in step order.
fn parse_anchor_dys(report: &str, expected_len: usize) -> Vec<i32> {
    let mut anchor_dys = Vec::with_capacity(expected_len);
    for line in report.lines() {
        let Some(rest) = line.strip_prefix("step=00->") else {
            continue;
        };
        let Some(dy_field) = rest.split_whitespace().nth(1) else {
            continue;
        };
        let Some(value) = dy_field.strip_prefix("anchor_dy=") else {
            continue;
        };
        anchor_dys.push(
            value
                .parse()
                .unwrap_or_else(|err| panic!("anchor_dy {value:?} did not parse: {err}")),
        );
    }
    assert_eq!(
        anchor_dys.len(),
        expected_len,
        "expected one anchor_dy per captured frame; report:\n{report}"
    );
    anchor_dys
}

fn open_receipts_tab(robot: &cranpose::Robot) {
    for _ in 0..30 {
        if let Some((x, y, w, h)) = cranpose_testing::find_button_in_semantics(robot, "Receipts") {
            robot
                .click(x + w * 0.5, y + h * 0.5)
                .expect("click Receipts tab");
            std::thread::sleep(Duration::from_millis(250));
            let _ = robot.wait_for_idle();
            if cranpose_testing::find_text_in_semantics(robot, "Library").is_some() {
                return;
            }
        }
        std::thread::sleep(Duration::from_millis(100));
    }
    panic!("Receipts tab / Library glass bar not found after 30 attempts");
}
