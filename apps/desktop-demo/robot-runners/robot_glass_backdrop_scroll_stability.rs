//! Robot test: the glass "Library" top bar on the Receipts feed must keep
//! re-blurring the moving list behind it at every single one-pixel scroll
//! step — it must never hold a stale composite while the content behind it
//! moves.
//!
//! NOT headless. NOT renderer screenshots. Real X11 window capture via
//! ImageMagick `import`/`xwd`, the same mechanism `robot_underline_screenshot`
//! and `robot_text_strikeout_presented` use to see the actual windowed
//! present path: a `capture_frame`/`robot.screenshot()` call renders into a
//! fresh offscreen texture every time and never touches
//! `redraw_native_window`'s real, reused swapchain view — the only path a
//! human actually sees. Four earlier attempts to reproduce this bug through
//! that offscreen path (a hash-key unit test, a hand-built analog scene, the
//! literal production composable, and a broken robot-driven check) all
//! showed perfect continuity; none of them could have caught a bug that only
//! exists on the reused-view path in the first place.
//!
//! Structurally this is `robot_scroll_decoration_invariance`'s underline
//! test turned inside out: that test asserts a decoration row does NOT move
//! independently of the text it decorates. This test asserts a backdrop DOES
//! change on every step the content behind it moves — "held" is only ever
//! legitimate here if the content behind the chrome is legitimately static,
//! which a continuously scrolling list of colored cards never is.

mod output_paths;
mod text_showcase_external_helpers;

use std::time::Duration;

use cranpose::AppLauncher;
use cranpose_testing::{
    changed_pixel_count_in_region, find_button_in_semantics, find_text_in_semantics,
};
use desktop_app::app;
use text_showcase_external_helpers::{capture_x11_window_screenshot, find_window_id};

const WINDOW_WIDTH: u32 = 900;
const WINDOW_HEIGHT: u32 = 800;
const WINDOW_TITLE: &str = "Robot Glass Backdrop Scroll Stability";
/// Deep enough that the sampled chrome sits over real card content, not the
/// LazyColumn's still-empty top content padding — a false positive already
/// caught once in this investigation (see TIME_WASTERS.md: "A backdrop-
/// continuity probe that samples before content ever reaches the sampled
/// rect reads as a stale cache, and is not one").
const SETTLE_SCROLL: f32 = -400.0;
const SCROLL_DELTA_Y: f32 = -1.0;
const STEP_COUNT: usize = 10;
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
const COMPARE_SEARCH_OFFSET_PX: u32 = 16;
/// Small: the sampled glass patch is only ~90px tall, and the reused
/// engine's fractional-alignment guard grows with the largest measured
/// shift (up to ~`STEP_COUNT` px here), which must still leave a positive
/// crop height.
const COMPARE_STABILIZED_GUARD_PX: u32 = 4;
/// Interior of the "Library" glass `TopBar`
/// (`apps/desktop-demo/src/app/glass_feed.rs`). Confirmed against this
/// window size's own semantics dump: the bar's outer box is
/// (28, 103.6, 744, 56), its icon button occupies x=[680,756], so
/// x=[150,650] y=[105,157] sits inside the bar with margin, clear of
/// both the "Library" text and the button. Also verified by eye
/// against this run's own `glass_step00.png` before trusting any
/// number from it — sampling the wrong rectangle has cost this
/// investigation twice already (see TIME_WASTERS.md). Height is capped
/// by the reused compare script's fractional-shift guard, which grows
/// with the largest measured shift (~STEP_COUNT px) and must leave a
/// positive crop height.
const GLASS_REGION: (f32, f32, f32, f32) = (150.0, 105.0, 500.0, 52.0);
/// A best-fit shift within this many pixels of the expected cumulative
/// shift counts as tracking correctly. Must be well under 1 step's worth
/// of motion or it cannot tell "frozen" from "on time" at step 1.
const SHIFT_TOLERANCE_PX: i32 = 1;
/// Skip this many of the topmost visible receipt subtitles when picking a
/// tracking anchor, so a 20-step, 1px-per-step upward walk never pushes it
/// into whatever clips items out near the fixed chrome's lower edge.
const RECEIPT_ANCHOR_SKIP_FROM_TOP: usize = 2;

fn fail(robot: &cranpose::Robot, message: &str) -> ! {
    println!("FATAL: {message}");
    let _ = robot.exit();
    std::process::exit(1);
}

fn main() {
    env_logger::init();
    println!("=== Robot Glass Backdrop Scroll Stability ===");
    let output_dir = output_paths::diagnostic_path("cranpose_glass_backdrop_scroll_stability");
    println!("Output dir: {}", output_dir.display());
    std::fs::create_dir_all(&output_dir).expect("create output dir");

    AppLauncher::new()
        .with_title(WINDOW_TITLE)
        .with_size(WINDOW_WIDTH, WINDOW_HEIGHT)
        .with_headless(false)
        .with_test_driver(move |robot| {
            std::thread::sleep(Duration::from_millis(1000));
            let _ = robot.wait_for_idle();

            open_receipts_tab(&robot);
            std::thread::sleep(Duration::from_millis(400));
            let _ = robot.wait_for_idle();

            let window_id = find_window_id(WINDOW_TITLE);
            println!("Window ID: {window_id}");

            robot
                .mouse_move(WINDOW_WIDTH as f32 * 0.5, WINDOW_HEIGHT as f32 * 0.7)
                .expect("move cursor over list");
            std::thread::sleep(Duration::from_millis(30));
            robot
                .mouse_scroll_and_wait_for_frame(0.0, SETTLE_SCROLL)
                .expect("settle scroll past empty content padding");
            std::thread::sleep(Duration::from_millis(300));
            let _ = robot.wait_for_idle();

            // A receipt subtitle ("Receipt #NNNN — 12 items") is unique per
            // row, unlike the six recycled card titles, so once discovered
            // it is an unambiguous anchor for the rest of the run. The
            // topmost visible one is a bad choice: it starts close to
            // whatever clips items out once they scroll under the fixed
            // chrome, and a 20-step upward walk pushed it there mid-run
            // ("target text must stay visible" at step 13, first attempt).
            // Pick one with real headroom instead.
            let semantics = robot.get_semantics().expect("query semantics");
            let mut receipt_matches = Vec::new();
            for root in &semantics {
                collect_receipt_subtitles(root, &mut receipt_matches);
            }
            receipt_matches.sort_by(|a, b| a.1.partial_cmp(&b.1).expect("finite y"));
            let anchor_text = receipt_matches
                .get(RECEIPT_ANCHOR_SKIP_FROM_TOP)
                .map(|(text, _)| text.clone())
                .unwrap_or_else(|| {
                    fail(
                        &robot,
                        &format!(
                            "expected at least {} visible receipt subtitles after settle, found {}",
                            RECEIPT_ANCHOR_SKIP_FROM_TOP + 1,
                            receipt_matches.len()
                        ),
                    )
                });
            println!(
                "tracking content anchor: {anchor_text:?} (of {} visible: {:?})",
                receipt_matches.len(),
                receipt_matches
            );
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
            println!(
                "  baseline captured: {}x{} -> {}",
                prev_shot.width,
                prev_shot.height,
                baseline_path.display()
            );

            let mut min_changed = usize::MAX;
            let mut zero_steps = Vec::new();
            for step in 1..=STEP_COUNT {
                robot
                    .mouse_scroll_and_wait_for_frame(0.0, -1.0)
                    .expect("one-pixel scroll");
                std::thread::sleep(Duration::from_millis(120));
                let _ = robot.wait_for_idle();

                let shot_path = output_dir.join(format!("step_{step:02}_full.png"));
                let curr_shot = capture_x11_window_screenshot(
                    &window_id,
                    &shot_path,
                    WINDOW_WIDTH as f32,
                    WINDOW_HEIGHT as f32,
                );

                let changed = changed_pixel_count_in_region(
                    &prev_shot,
                    &curr_shot,
                    GLASS_REGION,
                    CHANGE_CHANNEL_THRESHOLD,
                );
                println!("  step {step}: glass_region_changed_pixels={changed}");
                min_changed = min_changed.min(changed);
                if changed == 0 {
                    zero_steps.push(step);
                }

                prev_shot = curr_shot;
            }

            if !zero_steps.is_empty() || min_changed < MIN_CHANGED_PIXELS_PER_STEP {
                fail(
                    &robot,
                    &format!(
                        "glass backdrop held a stale composite across a one-pixel scroll: \
                         zero_change_steps={zero_steps:?} min_changed_pixels={min_changed} \
                         (floor={MIN_CHANGED_PIXELS_PER_STEP}). Screenshots saved to {}/",
                        output_dir.display()
                    ),
                );
            }

            println!(
                "PASS: glass backdrop changed on every single one-pixel scroll step (min={min_changed})"
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

fn collect_receipt_subtitles(elem: &cranpose::SemanticElement, out: &mut Vec<(String, f32)>) {
    if let Some(text) = elem.text.as_deref() {
        if text.starts_with("Receipt #") {
            out.push((text.to_string(), elem.bounds.y));
        }
    }
    for child in &elem.children {
        collect_receipt_subtitles(child, out);
    }
}

fn open_receipts_tab(robot: &cranpose::Robot) {
    for _ in 0..30 {
        if let Some((x, y, w, h)) = find_button_in_semantics(robot, "Receipts") {
            robot
                .click(x + w * 0.5, y + h * 0.5)
                .expect("click Receipts tab");
            std::thread::sleep(Duration::from_millis(250));
            let _ = robot.wait_for_idle();
            if find_text_in_semantics(robot, "Library").is_some() {
                return;
            }
        }
        std::thread::sleep(Duration::from_millis(100));
    }
    panic!("Receipts tab / Library glass bar not found after 30 attempts");
}
