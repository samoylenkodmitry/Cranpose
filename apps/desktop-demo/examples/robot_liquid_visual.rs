//! Liquid UI visual walkthrough: opens the Liquid tab, captures the resting
//! state, scrolls to the materials lab, exercises the tab-bar blob and the
//! morphing menu, and writes numbered PNGs into `CRANPOSE_LIQUID_SHOT_DIR`.

use cranpose::AppLauncher;
use desktop_app::app::{self, TEST_ACTIVE_TAB_STATE};
use image::RgbaImage;
use std::path::{Path, PathBuf};
use std::time::Duration;

const WINDOW_WIDTH: u32 = 900;
const WINDOW_HEIGHT: u32 = 800;
const TARGET_TOGGLE_WIDTH: u32 = 340;
const TARGET_TOGGLE_HEIGHT: u32 = 190;
const TARGET_TOGGLE_TRACK_LEFT: u32 = 37;
const TARGET_TOGGLE_TRACK_TOP: u32 = 52;
const TARGET_TAB_WIDTH: u32 = 1320;
const TARGET_TAB_HEIGHT: u32 = 400;
const TARGET_TAB_BAR_TOP: u32 = 258;
const TARGET_TAB_VIEWPORT_WIDTH_DP: f32 = 440.0;
const TAB_BAR_CONTENT_INSET_DP: f32 = 8.0;
const TARGET_TAB_DRAG_FRACTION: f32 = 0.50;

#[derive(Clone, Copy, Debug)]
struct PixelCrop {
    left: u32,
    top: u32,
    width: u32,
    height: u32,
}

fn main() {
    let _ = env_logger::try_init();
    let shot_dir = PathBuf::from(
        std::env::var("CRANPOSE_LIQUID_SHOT_DIR")
            .unwrap_or_else(|_| "target/liquid-shots".to_string()),
    );
    std::fs::create_dir_all(&shot_dir).expect("create shot dir");

    AppLauncher::new()
        .with_title("Liquid Visual")
        .with_size(WINDOW_WIDTH, WINDOW_HEIGHT)
        .with_fonts(desktop_app::fonts::DEMO_FONTS)
        .with_headless(std::env::var("CRANPOSE_HEADLESS").as_deref() != Ok("0"))
        .with_robot_app_hook(set_tab_hook)
        .with_test_driver(move |robot| {
            std::thread::sleep(Duration::from_millis(700));
            let _ = robot.wait_for_idle();
            robot
                .invoke_app_hook("set-tab", "liquid")
                .expect("select liquid tab");
            settle(&robot, 1000);
            let selector = cranpose_testing::find_text_in_semantics(&robot, "wcKSRD OPTICS")
                .expect("single wcKSRD optics heading");
            assert!(
                selector.1 >= 100.0 && selector.1 + selector.3 <= WINDOW_HEIGHT as f32,
                "the supplied-shader selector must be visible in the initial viewport: {selector:?}"
            );
            shot(&robot, &shot_dir, "01-top");

            // Toggle lens: press-and-hold the Wi-Fi thumb — the whole thumb
            // lifts into the transparent magnifying capsule (reference
            // "toggle in action"); then drag it mid-track. The controls card
            // must be scrolled into comfortable view first.
            scroll_text_to_y(&robot, "TOGGLE PRESS", 300.0);
            settle(&robot, 800);
            if let Some(toggle) =
                cranpose_testing::find_button_exact_in_semantics(&robot, "Wi-Fi switch")
            {
                assert!(
                    (toggle.2 - 63.0).abs() <= 1.0 && (toggle.3 - 28.0).abs() <= 1.0,
                    "Wi-Fi switch geometry drifted before optical capture: {toggle:?}"
                );
                let toggle_y = toggle.1 + toggle.3 * 0.5;
                let thumb_x = toggle.0 + toggle.2 - 20.0;
                robot.mouse_move(thumb_x, toggle_y).expect("hover thumb");
                robot.mouse_down().expect("press thumb");
                std::thread::sleep(Duration::from_millis(380));
                shot_scaled_aligned(
                    &robot,
                    &shot_dir,
                    "01a-toggle-press-lens",
                    "01a-toggle-press-lens-aligned",
                    3.0,
                    PixelCrop {
                        left: aligned_origin(toggle.0, 3.0, TARGET_TOGGLE_TRACK_LEFT),
                        top: aligned_origin(toggle.1, 3.0, TARGET_TOGGLE_TRACK_TOP),
                        width: TARGET_TOGGLE_WIDTH,
                        height: TARGET_TOGGLE_HEIGHT,
                    },
                );
                for step in 1..=6 {
                    robot
                        .mouse_move(thumb_x - step as f32 * 4.0, toggle_y)
                        .expect("drag thumb");
                    std::thread::sleep(Duration::from_millis(30));
                }
                // Hold until the lens fully materializes.
                std::thread::sleep(Duration::from_millis(420));
                shot_scaled(&robot, &shot_dir, "01b-toggle-drag-lens", 3.0);
                robot.mouse_up().expect("release thumb");
                // The lens lingers through the settle flight.
                std::thread::sleep(Duration::from_millis(220));
                shot(&robot, &shot_dir, "01b2-toggle-settle-lens");
                settle(&robot, 900);
                shot(&robot, &shot_dir, "01b3-toggle-settled");
            }

            let account = cranpose_testing::find_button_exact_in_semantics(&robot, "Account")
                .expect("account tab in semantics");
            let account_x = account.0 + account.2 * 0.5;
            let bar_y = account.1 + account.3 * 0.5;

            // Align the authored 440dp scene with the source frame's crop,
            // keeping identical content under the target and current lenses.
            scroll_text_to_y_with_tolerance(
                &robot,
                "Apple Developer Program",
                target_tab_reference_title_y(account.1, 3.0),
                1.0,
            );
            settle(&robot, 700);

            // Match the target's final swipe: Account is selected, then the
            // live lens moves left across WWDC while the finger stays down.
            {
                robot.click(account_x, bar_y).expect("select account");
                settle(&robot, 700);
                shot_scaled(&robot, &shot_dir, "01c0-tabbar-account-rest", 3.0);

                robot.mouse_move(account_x, bar_y).expect("hover account");
                robot.mouse_down().expect("press account");
                std::thread::sleep(Duration::from_millis(80));
                // f_055 is the canonical fast-flight frame: its lens center
                // is half a cell pitch left of Account.
                let target_drag = account.2 * TARGET_TAB_DRAG_FRACTION;
                for step in 1..=2 {
                    robot
                        .mouse_move(account_x - target_drag * step as f32 / 2.0, bar_y)
                        .expect("drag tab lens left");
                    if step < 2 {
                        std::thread::sleep(Duration::from_millis(30));
                    }
                }
                std::thread::sleep(Duration::from_millis(16));
                let bar_top = account.1 - TAB_BAR_CONTENT_INSET_DP;
                shot_scaled_aligned(
                    &robot,
                    &shot_dir,
                    "01c-tabbar-drag-lens",
                    "01c-tabbar-drag-lens-aligned",
                    3.0,
                    PixelCrop {
                        left: (((WINDOW_WIDTH as f32 - TARGET_TAB_VIEWPORT_WIDTH_DP) * 0.5) * 3.0)
                            .round() as u32,
                        top: aligned_origin(bar_top, 3.0, TARGET_TAB_BAR_TOP),
                        width: TARGET_TAB_WIDTH,
                        height: TARGET_TAB_HEIGHT,
                    },
                );
                std::thread::sleep(Duration::from_millis(420));
                shot_scaled(&robot, &shot_dir, "01c1-tabbar-held-lens", 3.0);
                robot.mouse_up().expect("release account drag");
                settle(&robot, 700);

                // A separate full-width drag checks the same lens joining the
                // detached search surface near the bar's trailing edge.
                let discover = cranpose_testing::find_button_exact_in_semantics(&robot, "Discover")
                    .expect("discover tab in semantics");
                let start_x = discover.0 + discover.2 * 0.5;
                robot.click(start_x, bar_y).expect("select discover");
                settle(&robot, 700);
                robot.mouse_move(start_x, bar_y).expect("hover tab");
                robot.mouse_down().expect("press tab");
                std::thread::sleep(Duration::from_millis(80));
                for step in 1..=8 {
                    robot
                        .mouse_move(start_x + step as f32 * 22.0, bar_y)
                        .expect("drag tab lens");
                    if step < 8 {
                        std::thread::sleep(Duration::from_millis(30));
                    }
                }
                // Keep dragging to the bar's end: the lens necks with the
                // search circle through the liquid field.
                for step in 1..=12 {
                    robot
                        .mouse_move(start_x + 176.0 + step as f32 * 22.0, bar_y)
                        .expect("drag toward search");
                    std::thread::sleep(Duration::from_millis(30));
                }
                std::thread::sleep(Duration::from_millis(380));
                shot_scaled(&robot, &shot_dir, "01c2-lens-glues-search", 3.0);
                robot.mouse_up().expect("release tab");
                settle(&robot, 700);
            }
            if std::env::var_os("CRANPOSE_LIQUID_OPTICS_ONLY").is_some() {
                println!(
                    "PASS: liquid optical comparison written to {}",
                    shot_dir.display()
                );
                robot.exit().expect("exit");
                return;
            }
            scroll_text_to_y(&robot, "wcKSRD OPTICS", 220.0);
            settle(&robot, 700);
            shot(&robot, &shot_dir, "01d-after-interactions");

            shot(&robot, &shot_dir, "02-list-and-shaders");
            shot_scaled(&robot, &shot_dir, "02b-wcksrd-optics", 2.0);

            scroll(&robot, 450.0, 500.0, -850.0);
            settle(&robot, 900);
            shot(&robot, &shot_dir, "03-materials-lab");

            // Tab-bar blob: hop from Discover to Account mid-flight capture.
            // The tab cell is resolved from semantics — the pill stretches
            // tabs when width allows, so fixed coordinates would drift.
            let account = cranpose_testing::find_button_exact_in_semantics(&robot, "Account")
                .expect("account tab in semantics");
            robot
                .click(account.0 + account.2 * 0.5, account.1 + account.3 * 0.5)
                .expect("tap account tab");
            std::thread::sleep(Duration::from_millis(120));
            shot(&robot, &shot_dir, "04-blob-mid-flight");
            settle(&robot, 900);
            shot(&robot, &shot_dir, "05-blob-settled");

            // Morphing menu from the nav trailing button: the droplet inflates
            // out of the "…" circle (mid-flight frames), settles with a size
            // overshoot, then deflates back into the button on dismiss. Park
            // the colorful gradient stripe under the menu region first so the
            // glass transparency reads against real content.
            scroll(&robot, 450.0, 400.0, 4200.0);
            settle(&robot, 900);
            scroll(&robot, 450.0, 400.0, -260.0);
            settle(&robot, 700);
            robot.click(858.0, 122.0).expect("open menu");
            std::thread::sleep(Duration::from_millis(50));
            shot(&robot, &shot_dir, "06a-menu-morph-early");
            std::thread::sleep(Duration::from_millis(70));
            shot(&robot, &shot_dir, "06b-menu-morph-mid");
            std::thread::sleep(Duration::from_millis(90));
            shot(&robot, &shot_dir, "06c-menu-morph-late");
            settle(&robot, 700);
            shot(&robot, &shot_dir, "07-menu-open");
            // Dismiss: tap outside — the droplet sucks back into the anchor.
            robot.click(200.0, 400.0).expect("dismiss menu");
            std::thread::sleep(Duration::from_millis(60));
            shot(&robot, &shot_dir, "08a-menu-close-mid");
            settle(&robot, 600);
            shot(&robot, &shot_dir, "08b-menu-closed");

            println!(
                "PASS: liquid visual walkthrough written to {}",
                shot_dir.display()
            );
            robot.exit().expect("exit");
        })
        .run(app::combined_app);
}

fn set_tab_hook(name: String, argument: String) -> Result<Option<String>, String> {
    if name != "set-tab" {
        return Err(format!("unsupported robot app hook {name}({argument})"));
    }
    if argument != "liquid" {
        return Err(format!("unknown demo tab '{argument}'"));
    }
    let tab = app::DemoTab::Liquid;
    let state = TEST_ACTIVE_TAB_STATE
        .with(|cell| cell.borrow().as_ref().copied())
        .ok_or_else(|| "active tab state was not installed before selecting a tab".to_string())?;
    state.set(tab);
    Ok(None)
}

fn settle(robot: &cranpose::Robot, ms: u64) {
    std::thread::sleep(Duration::from_millis(ms));
    let _ = robot.wait_for_idle();
}

fn scroll(robot: &cranpose::Robot, x: f32, y: f32, delta_y: f32) {
    robot.mouse_move(x, y).expect("move cursor");
    robot.mouse_scroll(0.0, delta_y).expect("scroll");
}

fn scroll_text_to_y(robot: &cranpose::Robot, text: &str, target_y: f32) {
    scroll_text_to_y_with_tolerance(robot, text, target_y, 24.0);
}

fn scroll_text_to_y_with_tolerance(
    robot: &cranpose::Robot,
    text: &str,
    target_y: f32,
    alignment_tolerance: f32,
) {
    for _ in 0..8 {
        let rect = cranpose_testing::find_text_in_semantics(robot, text)
            .unwrap_or_else(|| panic!("{text} semantics missing while scrolling"));
        let delta = rect.1 - target_y;
        if delta.abs() <= alignment_tolerance {
            return;
        }
        scroll(robot, 450.0, 500.0, -delta.clamp(-700.0, 700.0));
        settle(robot, 240);
    }

    let rect = cranpose_testing::find_text_in_semantics(robot, text)
        .unwrap_or_else(|| panic!("{text} semantics missing after scrolling"));
    assert!(
        (rect.1 - target_y).abs() <= alignment_tolerance,
        "failed to scroll {text} to y={target_y}: {rect:?}"
    );
}

fn shot(robot: &cranpose::Robot, dir: &Path, name: &str) {
    let screenshot = robot.screenshot().expect("screenshot");
    let image = RgbaImage::from_raw(screenshot.width, screenshot.height, screenshot.pixels)
        .expect("screenshot buffer size");
    let path = dir.join(format!("{name}.png"));
    image.save(&path).expect("save screenshot");
    println!("captured {name} -> {}", path.display());
}

fn shot_scaled(robot: &cranpose::Robot, dir: &Path, name: &str, scale: f32) {
    let screenshot = robot
        .screenshot_with_scale(scale)
        .expect("scaled screenshot");
    let image = RgbaImage::from_raw(screenshot.width, screenshot.height, screenshot.pixels)
        .expect("scaled screenshot buffer size");
    let path = dir.join(format!("{name}.png"));
    image.save(&path).expect("save scaled screenshot");
    println!("captured {name} at {scale}x -> {}", path.display());
}

fn aligned_origin(anchor_dp: f32, scale: f32, target_anchor_px: u32) -> u32 {
    let origin = (anchor_dp * scale).round() as i64 - i64::from(target_anchor_px);
    u32::try_from(origin).expect("aligned crop origin must remain inside the screenshot")
}

fn target_tab_reference_title_y(account_top: f32, scale: f32) -> f32 {
    account_top - TAB_BAR_CONTENT_INSET_DP - TARGET_TAB_BAR_TOP as f32 / scale
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tab_reference_title_aligns_with_the_aligned_crop_origin() {
        let account_top = 714.0;
        let expected = account_top - TAB_BAR_CONTENT_INSET_DP - TARGET_TAB_BAR_TOP as f32 / 3.0;
        assert_eq!(target_tab_reference_title_y(account_top, 3.0), expected);
    }
}

fn shot_scaled_aligned(
    robot: &cranpose::Robot,
    dir: &Path,
    name: &str,
    aligned_name: &str,
    scale: f32,
    crop: PixelCrop,
) -> RgbaImage {
    let screenshot = robot
        .screenshot_with_scale(scale)
        .expect("scaled screenshot");
    let image = RgbaImage::from_raw(screenshot.width, screenshot.height, screenshot.pixels)
        .expect("scaled screenshot buffer size");
    let full_path = dir.join(format!("{name}.png"));
    image.save(&full_path).expect("save scaled screenshot");
    assert!(
        crop.left + crop.width <= image.width() && crop.top + crop.height <= image.height(),
        "aligned crop {crop:?} exceeds {}x{} capture",
        image.width(),
        image.height()
    );
    let aligned =
        image::imageops::crop_imm(&image, crop.left, crop.top, crop.width, crop.height).to_image();
    let aligned_path = dir.join(format!("{aligned_name}.png"));
    aligned
        .save(&aligned_path)
        .expect("save aligned screenshot");
    println!(
        "captured {name} at {scale}x -> {}; aligned -> {}",
        full_path.display(),
        aligned_path.display()
    );
    aligned
}
