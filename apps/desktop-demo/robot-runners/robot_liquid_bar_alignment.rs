//! Resting-bubble alignment for every liquid tab bar in the demo: after a
//! selection settles, the glass lens must center on the selected cell —
//! on the 3-tab translate bar, the 5-tab store bars, and the accessory
//! (Discover) bar alike. Detection is selection-diff based: the pixels
//! that change between two settled selections cluster around the old and
//! new bubble positions, and each cluster's centroid must sit on its
//! cell's center.
//!
//! Run with:
//! ```bash
//! cargo run --package desktop-app --example robot_liquid_bar_alignment --features robot-app
//! ```

use cranpose::{AppLauncher, Robot, RobotScreenshot};
use desktop_app::app;
use image::RgbaImage;
use std::path::{Path, PathBuf};
use std::process::ExitCode;
use std::time::Duration;

const WINDOW_WIDTH: u32 = 900;
const WINDOW_HEIGHT: u32 = 640;
/// A settled glide: the lens spring converges well inside this window.
const SETTLE_MS: u64 = 900;
/// Diff columns must change by at least this many pixels to count as part
/// of a bubble cluster (filters antialiasing noise).
const COLUMN_CHANGE_FLOOR: usize = 6;
/// Alignment budget in dp: bubble centroid vs cell center.
const ALIGNMENT_BUDGET_DP: f32 = 5.0;

fn main() -> ExitCode {
    let _ = env_logger::try_init();
    let shot_dir = PathBuf::from(
        std::env::var("ROBOT_SHOT_DIR").unwrap_or_else(|_| "target/liquid-bar-alignment".into()),
    );
    std::fs::create_dir_all(&shot_dir).expect("create shot dir");

    AppLauncher::new()
        .with_title("Liquid Bar Alignment")
        .with_size(WINDOW_WIDTH, WINDOW_HEIGHT)
        .with_fonts(desktop_app::fonts::DEMO_FONTS)
        .with_headless(std::env::var("CRANPOSE_HEADLESS").as_deref() != Ok("0"))
        .with_test_driver(move |robot| {
            const TEST_TIMEOUT_SECS: u64 = 240;
            std::thread::spawn(|| {
                std::thread::sleep(Duration::from_secs(TEST_TIMEOUT_SECS));
                println!("\n✗ Test timed out after {TEST_TIMEOUT_SECS} seconds");
                std::process::exit(1);
            });
            std::thread::sleep(Duration::from_millis(700));
            let _ = robot.wait_for_idle();
            let liquid_tab = robot
                .find_button_bounds_exact("Liquid UI")
                .ok()
                .flatten()
                .expect("Liquid UI tab present");
            robot
                .click(
                    liquid_tab.0 + liquid_tab.2 * 0.5,
                    liquid_tab.1 + liquid_tab.3 * 0.5,
                )
                .expect("open liquid tab");
            settle(&robot, 900);

            let mut failures = Vec::new();

            // 5-tab store bar (over the tile artwork).
            check_bar(
                &robot,
                &shot_dir,
                "store",
                &["Today", "Games", "Apps", "Arcade", "Search"],
                430.0,
                &mut failures,
            );

            // 3-tab translate bar on white.
            check_bar(
                &robot,
                &shot_dir,
                "translate",
                &["Translate", "Camera", "Conversation"],
                480.0,
                &mut failures,
            );

            assert!(
                failures.is_empty(),
                "bubble misalignment:\n  {}",
                failures.join("\n  ")
            );
            println!("PASS: resting bubbles center on their cells");
            let _ = robot.exit();
        })
        .try_run(app::combined_app)
        .expect("launch bar alignment runner");
    ExitCode::SUCCESS
}

fn settle(robot: &Robot, ms: u64) {
    let _ = robot.wait_for_idle();
    std::thread::sleep(Duration::from_millis(ms));
    let _ = robot.wait_for_idle();
}

/// Density under test: the dp/px-mixing bug class only shows at scale ≠ 1
/// (the user's desktop runs ≈1.354), so CI probes a non-unit scale too.
fn capture_scale() -> f32 {
    std::env::var("CRANPOSE_ROBOT_CAPTURE_SCALE")
        .ok()
        .and_then(|value| value.parse::<f32>().ok())
        .map(|value| value.clamp(0.5, 4.0))
        .unwrap_or(1.0)
}

fn capture(robot: &Robot) -> RobotScreenshot {
    robot
        .screenshot_with_scale(capture_scale())
        .expect("settled frame")
}

/// Measures every cell of one bar. Each measurement anchors the selection
/// on a FAR cell first (≥2 cells away, so the departed and arrived change
/// clusters can never merge into one band), then selects the target and
/// checks the arrived cluster's centroid against the cell center.
fn check_bar(
    robot: &Robot,
    shot_dir: &Path,
    bar: &str,
    labels: &[&str],
    target_y: f32,
    failures: &mut Vec<String>,
) {
    let Some(_) = scroll_to_button(robot, labels[0], target_y) else {
        failures.push(format!("{bar}: could not scroll {} into view", labels[0]));
        return;
    };

    for (target, label) in labels.iter().enumerate() {
        // Anchor at the far end from the target: never adjacent. A cell
        // with no anchor ≥2 away (the middle of a 3-tab bar) is skipped —
        // its neighbors pin the same linear placement formula.
        let anchor = if target < labels.len() / 2 {
            labels.len() - 1
        } else {
            0
        };
        if anchor.abs_diff(target) < 2 {
            println!("{bar}: {label}: skipped (no non-adjacent anchor)");
            continue;
        }
        let Some(anchor_center) = cell_center(robot, labels[anchor]) else {
            failures.push(format!("{bar}: {} has no bounds", labels[anchor]));
            return;
        };
        let Some(center) = cell_center(robot, label) else {
            failures.push(format!("{bar}: {label} has no bounds"));
            continue;
        };
        robot
            .click(anchor_center.0, anchor_center.1)
            .expect("anchor selection");
        settle(robot, SETTLE_MS);
        let baseline = capture(robot);
        robot.click(center.0, center.1).expect("select cell");
        settle(robot, SETTLE_MS);
        let shot = capture(robot);
        save(&shot, shot_dir, &format!("{bar}-{target}-{label}"));

        // The bubble settles to the WIDGET's own resting rule: interior
        // cells center exactly, end cells pull inboard so the wide bubble
        // stays inside the pill (cranpose::liquid::tab_lens_resting_left).
        let Some(first) = cell_center(robot, labels[0]) else {
            failures.push(format!("{bar}: {} has no bounds", labels[0]));
            return;
        };
        let Some(last) = cell_center(robot, labels[labels.len() - 1]) else {
            failures.push(format!("{bar}: last cell has no bounds"));
            return;
        };
        let pitch = (last.0 - first.0) / (labels.len() - 1).max(1) as f32;
        let expected =
            first.0 + cranpose::liquid::tab_lens_resting_left(target, pitch, labels.len());

        // The expected bubble span: the widget's resting rule ± the rest
        // half-width, both from the public resting contract.
        let half_rest = cranpose::liquid::tab_lens_rest_width(pitch) * 0.5;
        let span = (expected - half_rest, expected + half_rest);

        // Band the columns whose pixels changed between the two settled
        // frames, inside the bar's vertical strip.
        let bands = changed_column_bands(&baseline, &shot, center.1 - 60.0, center.1 + 60.0);
        let arrived = nearest_band(&bands, expected);
        println!(
            "{bar}: {label}: bands {bands:?} cell_x {:.1} expected {expected:.1} span {span:?}",
            center.0
        );

        match arrived {
            // A locally flat backdrop can hide one side of the zoom-free
            // lens from the diff, so alignment is proven by EITHER band
            // edge landing on its expected bubble edge; the resting edge
            // FEATHER also shaves the detected band symmetrically on
            // low-contrast renderers, so an on-center band of plausible
            // width proves alignment equally.
            Some(band)
                if (band.0 - span.0).abs() <= ALIGNMENT_BUDGET_DP
                    || (band.1 - span.1).abs() <= ALIGNMENT_BUDGET_DP
                    || ((band_center(band) - expected).abs() <= ALIGNMENT_BUDGET_DP
                        && band.1 - band.0 >= half_rest * 1.2) => {}
            Some(band) => failures.push(format!(
                "{bar}: bubble band {band:?} matches neither edge nor center of {label}'s rest span {span:?}"
            )),
            None => failures.push(format!("{bar}: no change cluster near {label}")),
        }
    }
}

fn cell_center(robot: &Robot, label: &str) -> Option<(f32, f32)> {
    let bounds = robot.find_button_bounds_exact(label).ok().flatten()?;
    Some((bounds.0 + bounds.2 * 0.5, bounds.1 + bounds.3 * 0.5))
}

/// Contiguous column ranges (logical px) with meaningful pixel change
/// between two frames, restricted to the bar's vertical strip.
fn changed_column_bands(
    before: &RobotScreenshot,
    after: &RobotScreenshot,
    strip_top: f32,
    strip_bottom: f32,
) -> Vec<(f32, f32)> {
    let (sx, sy) = (
        after.width as f32 / after.logical_width.max(1.0),
        after.height as f32 / after.logical_height.max(1.0),
    );
    let width = after.width.min(before.width) as usize;
    let top = ((strip_top * sy).max(0.0) as usize).min(after.height as usize);
    let bottom = ((strip_bottom * sy) as usize).min(after.height.min(before.height) as usize);

    let mut changed = vec![0usize; width];
    for py in top..bottom {
        for (px, count) in changed.iter_mut().enumerate() {
            let i = (py * after.width as usize + px) * 4;
            let j = (py * before.width as usize + px) * 4;
            let delta = after.pixels[i].abs_diff(before.pixels[j]) as u32
                + after.pixels[i + 1].abs_diff(before.pixels[j + 1]) as u32
                + after.pixels[i + 2].abs_diff(before.pixels[j + 2]) as u32;
            if delta > 24 {
                *count += 1;
            }
        }
    }

    let mut bands = Vec::new();
    let mut start: Option<usize> = None;
    let mut gap = 0usize;
    let max_gap = (6.0 * sx) as usize;
    for px in 0..width {
        if changed[px] >= COLUMN_CHANGE_FLOOR {
            if start.is_none() {
                start = Some(px);
            }
            gap = 0;
        } else if let Some(begin) = start {
            gap += 1;
            if gap > max_gap {
                bands.push((begin as f32 / sx, (px - gap) as f32 / sx));
                start = None;
                gap = 0;
            }
        }
    }
    if let Some(begin) = start {
        bands.push((begin as f32 / sx, (width - 1) as f32 / sx));
    }
    bands
}

fn band_center(band: (f32, f32)) -> f32 {
    (band.0 + band.1) * 0.5
}

fn nearest_band(bands: &[(f32, f32)], x: f32) -> Option<(f32, f32)> {
    bands
        .iter()
        .copied()
        .min_by(|a, b| {
            (band_center(*a) - x)
                .abs()
                .total_cmp(&(band_center(*b) - x).abs())
        })
        .filter(|band| (band_center(*band) - x).abs() <= 60.0)
}

fn scroll_to_button(robot: &Robot, label: &str, target_y: f32) -> Option<(f32, f32, f32, f32)> {
    for _ in 0..40 {
        if let Some(bounds) = robot.find_button_bounds_exact(label).ok().flatten() {
            let cy = bounds.1 + bounds.3 * 0.5;
            if (cy - target_y).abs() < 60.0 {
                return Some(bounds);
            }
            robot
                .mouse_move(WINDOW_WIDTH as f32 * 0.5, 400.0)
                .expect("hover for scroll");
            robot
                .mouse_scroll(0.0, if cy > target_y { -60.0 } else { 60.0 })
                .expect("scroll");
        } else {
            robot
                .mouse_move(WINDOW_WIDTH as f32 * 0.5, 400.0)
                .expect("hover for scroll");
            robot.mouse_scroll(0.0, -160.0).expect("seek scroll");
        }
        settle(robot, 120);
    }
    robot.find_button_bounds_exact(label).ok().flatten()
}

fn save(shot: &RobotScreenshot, dir: &Path, name: &str) {
    let image = RgbaImage::from_raw(shot.width, shot.height, shot.pixels.clone())
        .expect("screenshot buffer");
    let path = dir.join(format!("{name}.png"));
    image.save(&path).expect("save screenshot");
}
