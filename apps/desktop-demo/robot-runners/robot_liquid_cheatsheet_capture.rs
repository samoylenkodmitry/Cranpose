//! Capture-only runner behind `liquid_cheatsheets.sh`: drives every
//! `example/target` reference case against its matching Liquid UI demo stage
//! and saves reference-timed animation frames, one subdirectory per target
//! dir, for the TARGET|ACTUAL vision cheatsheets. No assertions — the
//! cheatsheets are judged by eye.

use cranpose::AppLauncher;
use desktop_app::app::{self, TEST_ACTIVE_TAB_STATE};
use image::RgbaImage;
use std::path::{Path, PathBuf};
use std::process::ExitCode;
use std::time::Duration;

const WINDOW_WIDTH: u32 = 900;
const WINDOW_HEIGHT: u32 = 800;

fn main() -> ExitCode {
    let _ = env_logger::try_init();
    let shot_dir = PathBuf::from(
        std::env::var("ROBOT_SHOT_DIR")
            .unwrap_or_else(|_| "target/liquid-cheatsheets/capture".to_string()),
    );
    std::fs::create_dir_all(&shot_dir).expect("create shot dir");

    AppLauncher::new()
        .with_title("Liquid Cheatsheet Capture")
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
            settle(&robot, 900);

            capture_toggle_press(&robot, &shot_dir);
            capture_menu_open(&robot, &shot_dir);
            capture_tab_swipe_and_form(&robot, &shot_dir);
            capture_on_white(&robot, &shot_dir);
            capture_touched_up(&robot, &shot_dir);

            println!("PASS: liquid cheatsheet capture");
            let _ = robot.exit();
        })
        .try_run(app::combined_app)
        .expect("launch cheatsheet capture runner");
    ExitCode::SUCCESS
}

/// Reference `toggle-press`: 54 frames @60fps (0.9 s) — press the OFF
/// toggle, flip, settle. Nine frames across the same window.
fn capture_toggle_press(robot: &cranpose::Robot, shot_dir: &Path) {
    let Some(toggle) = scroll_to_button(robot, "Wi-Fi switch", 320.0) else {
        eprintln!("SKIP toggle-press: stage not found");
        return;
    };
    let (cx, cy) = center(toggle);
    // The stage toggle starts ON; flip it OFF first so the captured
    // sequence is the reference's OFF -> ON flight.
    robot.click(cx, cy).expect("prime toggle");
    settle(robot, 1400);

    robot.mouse_move(cx, cy).expect("hover toggle");
    robot.mouse_down().expect("press toggle");
    let held = robot
        .capture_keyframes(1.0, &[(0.0, true), (120.0, true), (130.0, true)])
        .expect("press keyframes");
    robot.mouse_up().expect("release toggle");
    let flight = robot
        .capture_keyframes(
            1.0,
            &[
                (0.0, true),
                (110.0, true),
                (110.0, true),
                (110.0, true),
                (110.0, true),
                (200.0, true),
            ],
        )
        .expect("flight keyframes");
    settle(robot, 900);
    let crop = (cx - 62.0, cy - 32.0, 113.0, 63.0);
    save_series(
        shot_dir,
        "toggle-press",
        crop,
        held.iter().chain(flight.iter()),
    );
    // Restore the stage's resting state for later sections.
    robot.click(cx, cy).expect("restore toggle");
    settle(robot, 1200);
}

/// Reference `menu-open`: 108 frames @60fps (1.8 s) — the nav "..." button
/// swells into the droplet; content materializes near settle. The reference
/// droplet grows over WHITE list content, so park the session cards under
/// the nav anchor first.
fn capture_menu_open(robot: &cranpose::Robot, shot_dir: &Path) {
    // Rest scroll: the Featured videos stage sits right under the nav's
    // filter/"..." circles, composing the reference page.
    for _ in 0..30 {
        robot
            .mouse_move(450.0, 400.0)
            .expect("hover for scroll top");
        robot.mouse_scroll(0.0, 200.0).expect("scroll to top");
        settle(robot, 60);
    }
    settle(robot, 400);
    robot.click(858.0, 122.0).expect("open menu");
    let grow = robot
        .capture_keyframes(
            1.0,
            &[
                (0.0, true),
                (60.0, true),
                (60.0, true),
                (80.0, true),
                (100.0, true),
                (150.0, true),
                (250.0, true),
                (400.0, true),
                (700.0, true),
            ],
        )
        .expect("menu keyframes");
    save_series(
        shot_dir,
        "menu-open",
        (540.0, 70.0, 350.0, 260.0),
        grow.iter(),
    );
    settle(robot, 500);
    robot.click(450.0, 640.0).expect("dismiss menu");
    settle(robot, 900);
}

/// Reference `tab-swipe` (57 frames @30fps, 1.9 s drag): the MAIN floating
/// bar dragged over the Enroll Now reference backdrop, exactly like the
/// App Store recording. `bottom-bar-form` stills come from the store bar
/// over the vivid tiles.
fn capture_tab_swipe_and_form(robot: &cranpose::Robot, shot_dir: &Path) {
    // Park the TAB SWIPE reference backdrop behind the main bar.
    let _ = scroll_to_button(robot, "Wi-Fi switch", 300.0);
    settle(robot, 400);
    let Some(discover) = robot.find_button_bounds_exact("Discover").ok().flatten() else {
        eprintln!("SKIP tab-swipe: main bar not found");
        return;
    };
    let Some(account) = robot.find_button_bounds_exact("Account").ok().flatten() else {
        eprintln!("SKIP tab-swipe: account cell not found");
        return;
    };
    let (start_x, bar_y) = center(discover);
    let (end_x, _) = center(account);
    let main_bar_crop = (0.0, bar_y - 72.0, WINDOW_WIDTH as f32, 144.0);

    robot.mouse_move(start_x, bar_y).expect("hover discover");
    robot.mouse_down().expect("grab main lens");
    std::thread::sleep(Duration::from_millis(220));
    let steps = 7usize;
    let mut frames = Vec::new();
    for step in 0..=steps {
        let t = step as f32 / steps as f32;
        let x = start_x + (end_x - start_x) * t;
        robot.mouse_move(x, bar_y).expect("drag main lens");
        std::thread::sleep(Duration::from_millis(200));
        frames.push(robot.screenshot().expect("swipe frame"));
    }
    robot.mouse_up().expect("release main lens");
    std::thread::sleep(Duration::from_millis(350));
    frames.push(robot.screenshot().expect("swipe settle"));
    save_series(shot_dir, "tab-swipe", main_bar_crop, frames.iter());
    settle(robot, 800);
    // Return the selection home for later sections.
    robot.click(start_x, bar_y).expect("restore discover");
    settle(robot, 900);

    // Form stills: the store bar resting over the vivid tiles.
    let Some(today) = scroll_to_button(robot, "Today", 430.0) else {
        eprintln!("SKIP bottom-bar-form: store bar not found");
        return;
    };
    let (today_x, y) = center(today);
    let bar_crop = (0.0, y - 66.0, WINDOW_WIDTH as f32, 133.0);
    let rest = robot.screenshot().expect("bar form rest");
    save(&rest, shot_dir, "bottom-bar-form", bar_crop, 0);
    robot.click(today_x, y).expect("select today");
    settle(robot, 1500);
    let today_selected = robot.screenshot().expect("bar form today");
    save(&today_selected, shot_dir, "bottom-bar-form", bar_crop, 1);
    settle(robot, 400);
}

/// Reference `on-white/bottom-bar-click` (tap transfer, ~0.9 s) and
/// `on-white/bottom-bar-click-hold` (raise, hold, drag, release).
fn capture_on_white(robot: &cranpose::Robot, shot_dir: &Path) {
    let Some(conversation) = scroll_to_button(robot, "Conversation", 480.0) else {
        eprintln!("SKIP on-white: bar not found");
        return;
    };
    let Some(camera) = robot.find_button_bounds_exact("Camera").ok().flatten() else {
        eprintln!("SKIP on-white: camera cell not found");
        return;
    };
    let (conv_x, y) = center(conversation);
    let (cam_x, _) = center(camera);
    let Some(translate) = robot.find_button_bounds_exact("Translate").ok().flatten() else {
        eprintln!("SKIP on-white: translate cell not found");
        return;
    };
    let (tr_x, _) = center(translate);

    // The reference transfer starts from Translate; prime that state so the
    // captured click actually flies.
    robot.click(tr_x, y).expect("prime translate");
    settle(robot, 1500);
    robot.click(conv_x, y).expect("click conversation");
    let transfer = robot
        .capture_keyframes(
            1.0,
            &[
                (0.0, true),
                (80.0, true),
                (80.0, true),
                (80.0, true),
                (120.0, true),
                (120.0, true),
                (150.0, true),
                (150.0, true),
                (200.0, true),
            ],
        )
        .expect("click transfer keyframes");
    let bar_crop = (cam_x - 175.0, y - 62.0, 380.0, 124.0);
    save_series(shot_dir, "on-white-click", bar_crop, transfer.iter());
    settle(robot, 900);

    robot.mouse_move(cam_x, y).expect("hover camera");
    robot.mouse_down().expect("press camera");
    let hold = robot
        .capture_keyframes(1.0, &[(0.0, true), (150.0, true), (350.0, true)])
        .expect("hold keyframes");
    let mut held: Vec<_> = hold;
    for step in 1..=3 {
        let t = step as f32 / 3.0;
        let x = cam_x + (conv_x - cam_x) * t;
        robot.mouse_move(x, y).expect("drag held lens");
        std::thread::sleep(Duration::from_millis(180));
        held.push(robot.screenshot().expect("held drag frame"));
    }
    robot.mouse_up().expect("release held lens");
    let release = robot
        .capture_keyframes(1.0, &[(30.0, true), (150.0, true), (400.0, true)])
        .expect("release keyframes");
    held.extend(release);
    save_series(shot_dir, "on-white-click-hold", bar_crop, held.iter());
    settle(robot, 900);
}

/// Reference `on-white/touched-up-state`: the circular action group —
/// touch, merge toward the neighbor, release through translucent states.
fn capture_touched_up(robot: &cranpose::Robot, shot_dir: &Path) {
    let Some(more) = scroll_to_button(robot, "More grouped action", 400.0) else {
        eprintln!("SKIP touched-up: action group not found");
        return;
    };
    let Some(confirm) = robot
        .find_button_bounds_exact("Confirm grouped action")
        .ok()
        .flatten()
    else {
        eprintln!("SKIP touched-up: confirm action not found");
        return;
    };
    let (more_x, y) = center(more);
    let (confirm_x, _) = center(confirm);

    // The reference presses the CONFIRM disc: it swells, then necks into
    // the neighboring "..." circle as the finger drifts toward it.
    robot.mouse_move(confirm_x, y).expect("hover action");
    robot.mouse_down().expect("press action");
    let mut frames = robot
        .capture_keyframes(1.0, &[(0.0, true), (120.0, true), (250.0, true)])
        .expect("touch keyframes");
    for step in 1..=2 {
        let t = step as f32 / 2.0;
        let x = confirm_x + (more_x - confirm_x) * 0.55 * t;
        robot.mouse_move(x, y).expect("drag action");
        std::thread::sleep(Duration::from_millis(160));
        frames.push(robot.screenshot().expect("action drag frame"));
    }
    robot.mouse_up().expect("release action");
    let release = robot
        .capture_keyframes(
            1.0,
            &[(30.0, true), (120.0, true), (250.0, true), (450.0, true)],
        )
        .expect("action release keyframes");
    frames.extend(release);
    settle(robot, 600);
    // Confirming collapses the pair down to the lone "..." (the reference
    // dissolve tail); pressing "..." expands the actions again.
    robot.click(confirm_x, y).expect("confirm action");
    let collapse = robot
        .capture_keyframes(1.0, &[(60.0, true), (250.0, true), (500.0, true)])
        .expect("collapse keyframes");
    frames.extend(collapse);
    let crop = (more_x - 90.0, y - 55.0, 260.0, 110.0);
    save_series(shot_dir, "touched-up-state", crop, frames.iter());
    settle(robot, 600);
    robot.click(more_x, y).expect("restore actions");
    settle(robot, 900);
}

// ---------------------------------------------------------------------------

fn center(bounds: (f32, f32, f32, f32)) -> (f32, f32) {
    (bounds.0 + bounds.2 * 0.5, bounds.1 + bounds.3 * 0.5)
}

/// Scrolls the page until the named semantics button sits near `target_y`.
fn scroll_to_button(
    robot: &cranpose::Robot,
    label: &str,
    target_y: f32,
) -> Option<(f32, f32, f32, f32)> {
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

fn save_series<'a>(
    shot_dir: &Path,
    component: &str,
    crop: (f32, f32, f32, f32),
    frames: impl Iterator<Item = &'a cranpose::RobotScreenshot>,
) {
    for (index, frame) in frames.enumerate() {
        save(frame, shot_dir, component, crop, index);
    }
}

/// Saves the frame cropped to the stage region so cheatsheet tiles frame the
/// component exactly like the reference crops, wherever the page scrolled.
fn save(
    shot: &cranpose::RobotScreenshot,
    shot_dir: &Path,
    component: &str,
    crop: (f32, f32, f32, f32),
    index: usize,
) {
    let dir = shot_dir.join(component);
    std::fs::create_dir_all(&dir).expect("create component dir");
    let image = RgbaImage::from_raw(shot.width, shot.height, shot.pixels.clone())
        .expect("frame buffer size");
    let scale = shot.width as f32 / WINDOW_WIDTH as f32;
    let x = ((crop.0 * scale).max(0.0) as u32).min(shot.width.saturating_sub(1));
    let y = ((crop.1 * scale).max(0.0) as u32).min(shot.height.saturating_sub(1));
    let w = ((crop.2 * scale) as u32).min(shot.width - x);
    let h = ((crop.3 * scale) as u32).min(shot.height - y);
    let cropped = image::imageops::crop_imm(&image, x, y, w.max(1), h.max(1)).to_image();
    let path = dir.join(format!("{index:02}.png"));
    cropped.save(&path).expect("save frame");
    println!("saved {}", path.display());
}

fn set_tab_hook(name: String, argument: String) -> Result<Option<String>, String> {
    if name != "set-tab" {
        return Err(format!("unsupported robot app hook {name}({argument})"));
    }
    if argument != "liquid" {
        return Err(format!("unknown demo tab '{argument}'"));
    }
    let state = TEST_ACTIVE_TAB_STATE
        .with(|cell| cell.borrow().as_ref().copied())
        .ok_or_else(|| "active tab state was not installed".to_string())?;
    state.set(app::DemoTab::Liquid);
    Ok(None)
}

fn settle(robot: &cranpose::Robot, ms: u64) {
    std::thread::sleep(Duration::from_millis(ms));
    let _ = robot.wait_for_idle();
}
