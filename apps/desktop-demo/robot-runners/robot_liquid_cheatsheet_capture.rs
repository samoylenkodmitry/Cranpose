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

            // CRANPOSE_CHEATSHEET_STAGES=menu-expand,segmented narrows a
            // recapture to the named stage groups for fast tuning loops.
            let stage_filter = std::env::var("CRANPOSE_CHEATSHEET_STAGES").ok();
            let run_stage = |name: &str| {
                stage_filter
                    .as_deref()
                    .is_none_or(|filter| filter.split(',').any(|s| s.trim() == name))
            };
            if run_stage("toggle-press") {
                capture_toggle_press(&robot, &shot_dir);
            }
            if run_stage("menu-open") {
                capture_menu_open(&robot, &shot_dir);
            }
            if run_stage("tab-swipe") {
                capture_tab_swipe_and_form(&robot, &shot_dir);
            }
            if run_stage("segmented") {
                capture_segmented(&robot, &shot_dir);
            }
            if run_stage("menu-expand") {
                capture_menu_expand(&robot, &shot_dir);
            }
            if run_stage("on-white") {
                capture_on_white(&robot, &shot_dir);
            }
            if run_stage("touched-up") {
                capture_touched_up(&robot, &shot_dir);
            }

            println!("PASS: liquid cheatsheet capture");
            let _ = robot.exit();
        })
        .try_run(app::combined_app)
        .expect("launch cheatsheet capture runner");
    ExitCode::SUCCESS
}

/// Reference `toggle-press`: 54 frames @60fps (0.9 s) — press the OFF
/// toggle, flip, settle. Captured at ~33 ms steps (every 2nd reference
/// frame) so the flight physics reads off the sheet.
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
    let mut frames = keyframe_series(robot, &dense(33.0, 5), 0.0);
    robot.mouse_up().expect("release toggle");
    let mut flight_offsets = dense(33.0, 17);
    flight_offsets.extend_from_slice(&[(100.0, true), (150.0, true)]);
    frames.extend(keyframe_series(robot, &flight_offsets, 132.0));
    settle(robot, 900);
    let crop = (cx - 62.0, cy - 32.0, 113.0, 63.0);
    save_series(shot_dir, "toggle-press", crop, frames.iter());
    // Restore the stage's resting state for later sections.
    robot.click(cx, cy).expect("restore toggle");
    settle(robot, 1200);
}

/// Reference `menu-open`: 108 frames @60fps (1.8 s) — the nav "..." button
/// swells into the droplet; content materializes near settle. The reference
/// droplet grows over WHITE list content, so park the session cards under
/// the nav anchor first.
fn capture_menu_open(robot: &cranpose::Robot, shot_dir: &Path) {
    // Rest scroll: the Featured videos card hosts the filter/"..." anchor
    // circles in its header, composing the reference page.
    for _ in 0..30 {
        robot
            .mouse_move(450.0, 400.0)
            .expect("hover for scroll top");
        robot.mouse_scroll(0.0, 200.0).expect("scroll to top");
        settle(robot, 60);
    }
    settle(robot, 400);
    let Some(card) = robot
        .find_button_bounds_exact("Featured videos")
        .ok()
        .flatten()
    else {
        eprintln!("SKIP menu-open: featured videos card not found");
        return;
    };
    // The "..." circle sits at the header row's right edge.
    robot
        .click(card.0 + card.2 - 34.0, card.1 + 34.0)
        .expect("open menu");
    // Open: dense 25 ms sweep over the droplet growth + materialize tail.
    let mut grow_offsets = dense(25.0, 13);
    grow_offsets.extend_from_slice(&[(50.0, true), (50.0, true), (100.0, true), (200.0, true)]);
    let mut frames = keyframe_series(robot, &grow_offsets, 0.0);
    settle(robot, 500);
    // Close: the reference segment ends with the ~220 ms suck-back; put it
    // on the same timeline after the hold (labels ~1200 ms+).
    robot.click(450.0, 640.0).expect("dismiss menu");
    frames.extend(keyframe_series(robot, &dense(25.0, 11), 1200.0));
    save_series(
        shot_dir,
        "menu-open",
        (card.0 - 10.0, card.1 - 40.0, card.2 + 20.0, card.3 + 60.0),
        frames.iter(),
    );
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
    let clock = std::time::Instant::now();
    std::thread::sleep(Duration::from_millis(220));
    // The reference drag spans ~1.9 s; each screenshot costs ~250-300 ms of
    // wall clock, so the step count IS the drag speed — no extra sleeps.
    // Real elapsed time lands on each tile.
    let steps = 6usize;
    let mut frames = Vec::new();
    for step in 0..=steps {
        let t = step as f32 / steps as f32;
        let x = start_x + (end_x - start_x) * t;
        robot.mouse_move(x, bar_y).expect("drag main lens");
        let shot = robot.screenshot().expect("swipe frame");
        frames.push((clock.elapsed().as_millis() as f32, shot));
    }
    robot.mouse_up().expect("release main lens");
    for wait in [120u64, 230] {
        std::thread::sleep(Duration::from_millis(wait));
        let shot = robot.screenshot().expect("swipe settle");
        frames.push((clock.elapsed().as_millis() as f32, shot));
    }
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
    save(&rest, shot_dir, "bottom-bar-form", bar_crop, 0, 0.0);
    robot.click(today_x, y).expect("select today");
    settle(robot, 1500);
    let today_selected = robot.screenshot().expect("bar form today");
    save(
        &today_selected,
        shot_dir,
        "bottom-bar-form",
        bar_crop,
        1,
        1500.0,
    );
    settle(robot, 400);

    // Headers-fold still: the bar over the colored section headers whose
    // titles cross its top edge (bar_headers_folded reference).
    let Some(stage) = scroll_to_button(robot, "Headers fold stage", 430.0) else {
        eprintln!("SKIP bottom-bar-form headers: stage not found");
        return;
    };
    settle(robot, 600);
    let folded = robot.screenshot().expect("bar headers folded");
    save(
        &folded,
        shot_dir,
        "bottom-bar-form",
        (stage.0, stage.1, stage.2, stage.3),
        2,
        3000.0,
    );
}

/// Reference `segmented` (Receiving | Sending | Errored @60fps): a tap
/// flight across cells, then a continuous ride Receiving <-> Sending with
/// the lens magnifying the glyphs under it.
fn capture_segmented(robot: &cranpose::Robot, shot_dir: &Path) {
    let Some(receiving) = scroll_to_button(robot, "Receiving", 380.0) else {
        eprintln!("SKIP segmented: control not found");
        return;
    };
    let Some(sending) = robot.find_button_bounds_exact("Sending").ok().flatten() else {
        eprintln!("SKIP segmented: sending cell not found");
        return;
    };
    let Some(errored) = robot.find_button_bounds_exact("Errored").ok().flatten() else {
        eprintln!("SKIP segmented: errored cell not found");
        return;
    };
    let (rx, y) = center(receiving);
    let (sx, _) = center(sending);
    let (ex, _) = center(errored);
    let crop = (
        receiving.0 - 20.0,
        y - 34.0,
        (ex - rx) + errored.2 + 40.0,
        68.0,
    );

    // Prime: select Errored so the tap flight crosses Errored -> Sending
    // like the reference.
    robot.click(ex, y).expect("prime errored");
    settle(robot, 900);
    // The reference tap is a HELD press (~380ms finger-down: the selected
    // pill charges in place for the whole hold — tap-flight f_0000..0383 —
    // and the flight leaves on release). Reproduce the recorded finger,
    // not an instant click.
    robot.mouse_move(sx, y).expect("hover sending");
    robot.mouse_down().expect("press sending");
    let mut frames = keyframe_series(robot, &dense(25.0, 16), 0.0);
    robot.mouse_up().expect("release sending");
    let mut flight_offsets = dense(25.0, 12);
    flight_offsets.extend_from_slice(&[(50.0, true), (100.0, true), (150.0, true)]);
    frames.extend(keyframe_series(robot, &flight_offsets, 400.0));
    save_series(shot_dir, "segmented-tap-flight", crop, frames.iter());
    settle(robot, 700);

    // Ride: grab Sending, drag to Receiving, back toward Sending, release.
    robot.mouse_move(sx, y).expect("hover sending");
    robot.mouse_down().expect("grab segment lens");
    let clock = std::time::Instant::now();
    std::thread::sleep(Duration::from_millis(150));
    let mut ride = Vec::new();
    let waypoints = [0.25f32, 0.5, 0.75, 1.0, 0.75, 0.5, 0.25, 0.0, 0.35, 0.7];
    for t in waypoints {
        let x = sx + (rx - sx) * t;
        robot.mouse_move(x, y).expect("ride segment lens");
        let shot = robot.screenshot().expect("segment ride frame");
        ride.push((clock.elapsed().as_millis() as f32, shot));
    }
    robot.mouse_up().expect("release segment lens");
    let release_base = clock.elapsed().as_millis() as f32;
    ride.extend(keyframe_series(
        robot,
        &[(0.0, true), (60.0, true), (120.0, true), (250.0, true)],
        release_base,
    ));
    save_series(shot_dir, "segmented-drag", crop, ride.iter());
    settle(robot, 600);
    robot.click(rx, y).expect("restore receiving");
    settle(robot, 800);
}

/// Reference `menu-expand` (dark sort/filter menu @60fps): open from the
/// magenta pill, expand "Sort by" in place (accordion size morph), collapse
/// back, then close into the pill. One ms timeline: open 0+, expand 1000+,
/// collapse 2000+, close 3000+.
fn capture_menu_expand(robot: &cranpose::Robot, shot_dir: &Path) {
    let Some(pill) = scroll_to_button(robot, "Sort filter pill", 260.0) else {
        eprintln!("SKIP menu-expand: stage not found");
        return;
    };
    let (px, py) = center(pill);
    // The stage is 440x300 with the pill inset 16dp from its top-right.
    let crop = (
        px + pill.2 * 0.5 + 16.0 - 440.0,
        pill.1 - 17.0,
        440.0,
        300.0,
    );

    // The reference open is a HELD press (~200ms finger-down: the pill
    // charges bright magenta while pressed — open strip 200-366ms — and
    // the droplet leaves on release). Reproduce the recorded finger.
    robot.mouse_move(px, py).expect("hover sort pill");
    robot.mouse_down().expect("press sort pill");
    let mut frames = keyframe_series(robot, &dense(25.0, 9), 0.0);
    robot.mouse_up().expect("release sort pill");
    let mut open_offsets = dense(25.0, 13);
    open_offsets.extend_from_slice(&[(50.0, true), (100.0, true), (200.0, true)]);
    frames.extend(keyframe_series(robot, &open_offsets, 225.0));
    settle(robot, 500);

    let Some(sort_row) = robot.find_button_bounds_exact("Sort by").ok().flatten() else {
        eprintln!("SKIP menu-expand: sort row not found");
        return;
    };
    let (sx, sy) = center(sort_row);
    robot.click(sx, sy).expect("expand sort section");
    let mut expand_offsets = dense(25.0, 13);
    expand_offsets.extend_from_slice(&[(50.0, true), (100.0, true), (200.0, true)]);
    frames.extend(keyframe_series(robot, &expand_offsets, 1000.0));
    settle(robot, 500);

    robot.click(sx, sy).expect("collapse sort section");
    frames.extend(keyframe_series(robot, &expand_offsets.clone(), 2000.0));
    settle(robot, 500);

    // Dismiss with a tap on the stage's light page, outside the menu.
    robot
        .click(crop.0 + 50.0, crop.1 + 270.0)
        .expect("dismiss sort menu");
    frames.extend(keyframe_series(robot, &dense(25.0, 12), 3000.0));
    save_series(shot_dir, "menu-expand", crop, frames.iter());
    settle(robot, 800);
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
    let mut transfer_offsets = dense(25.0, 13);
    transfer_offsets.extend_from_slice(&[(50.0, true), (100.0, true), (150.0, true)]);
    let transfer = keyframe_series(robot, &transfer_offsets, 0.0);
    let bar_crop = (cam_x - 175.0, y - 62.0, 380.0, 124.0);
    save_series(shot_dir, "on-white-click", bar_crop, transfer.iter());
    settle(robot, 900);

    robot.mouse_move(cam_x, y).expect("hover camera");
    robot.mouse_down().expect("press camera");
    let clock = std::time::Instant::now();
    let mut raise_offsets = dense(40.0, 5);
    raise_offsets.extend_from_slice(&[(75.0, true), (115.0, true)]);
    let mut held = keyframe_series(robot, &raise_offsets, 0.0);
    for step in 1..=6 {
        let t = step as f32 / 6.0;
        let x = cam_x + (conv_x - cam_x) * t;
        robot.mouse_move(x, y).expect("drag held lens");
        std::thread::sleep(Duration::from_millis(90));
        let shot = robot.screenshot().expect("held drag frame");
        held.push((clock.elapsed().as_millis() as f32, shot));
    }
    robot.mouse_up().expect("release held lens");
    let release_base = clock.elapsed().as_millis() as f32;
    let mut release_offsets = dense(25.0, 5);
    release_offsets.extend_from_slice(&[(100.0, true), (250.0, true)]);
    held.extend(keyframe_series(robot, &release_offsets, release_base));
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
    let clock = std::time::Instant::now();
    let mut swell_offsets = dense(40.0, 5);
    swell_offsets.push((90.0, true));
    let mut frames = keyframe_series(robot, &swell_offsets, 0.0);
    for step in 1..=4 {
        let t = step as f32 / 4.0;
        let x = confirm_x + (more_x - confirm_x) * 0.55 * t;
        robot.mouse_move(x, y).expect("drag action");
        std::thread::sleep(Duration::from_millis(80));
        let shot = robot.screenshot().expect("action drag frame");
        frames.push((clock.elapsed().as_millis() as f32, shot));
    }
    robot.mouse_up().expect("release action");
    let release_base = clock.elapsed().as_millis() as f32;
    let mut release_offsets = dense(25.0, 5);
    release_offsets.extend_from_slice(&[(50.0, true), (100.0, true), (200.0, true)]);
    frames.extend(keyframe_series(robot, &release_offsets, release_base));
    settle(robot, 600);
    // Confirming collapses the pair down to the lone "..." (the reference
    // dissolve tail); pressing "..." expands the actions again. Collapse
    // labels continue the timeline from 2000 ms.
    robot.click(confirm_x, y).expect("confirm action");
    let mut collapse_offsets = dense(30.0, 6);
    collapse_offsets.extend_from_slice(&[(100.0, true), (250.0, true)]);
    frames.extend(keyframe_series(robot, &collapse_offsets, 2000.0));
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

/// Runs `capture_keyframes` and pairs every captured shot with its
/// millisecond position on the interaction timeline (`base_ms` + the
/// cumulative exact-clock offsets), so cheatsheet tiles carry real timing.
/// Capture density for every sheet frame: CRANPOSE_ROBOT_CAPTURE_SCALE
/// (default 1). Sub-dp optics (the glass bevel band) are invisible at
/// scale 1 — judge edge light at 2x like the reference recordings.
fn capture_scale() -> f32 {
    std::env::var("CRANPOSE_ROBOT_CAPTURE_SCALE")
        .ok()
        .and_then(|value| value.parse::<f32>().ok())
        .filter(|scale| scale.is_finite() && *scale >= 0.5 && *scale <= 4.0)
        .unwrap_or(1.0)
}

fn keyframe_series(
    robot: &cranpose::Robot,
    offsets: &[(f32, bool)],
    base_ms: f32,
) -> Vec<(f32, cranpose::RobotScreenshot)> {
    let shots = robot
        .capture_keyframes(capture_scale(), offsets)
        .expect("capture keyframes");
    let mut shot_iter = shots.into_iter();
    let mut t = base_ms;
    let mut out = Vec::new();
    for (delay, capture) in offsets {
        t += delay;
        if *capture {
            if let Some(shot) = shot_iter.next() {
                out.push((t, shot));
            }
        }
    }
    out
}

/// Dense keyframe schedule: `count` captures spaced `step_ms` apart,
/// starting at t=0 of the capture window.
fn dense(step_ms: f32, count: usize) -> Vec<(f32, bool)> {
    let mut offsets = vec![(0.0, true)];
    offsets.extend(std::iter::repeat_n(
        (step_ms, true),
        count.saturating_sub(1),
    ));
    offsets
}

fn save_series<'a>(
    shot_dir: &Path,
    component: &str,
    crop: (f32, f32, f32, f32),
    frames: impl Iterator<Item = &'a (f32, cranpose::RobotScreenshot)>,
) {
    for (index, (ms, frame)) in frames.enumerate() {
        save(frame, shot_dir, component, crop, index, *ms);
    }
}

/// Saves the frame cropped to the stage region so cheatsheet tiles frame the
/// component exactly like the reference crops, wherever the page scrolled.
/// Filenames carry the timeline position (`NN-TTTTms.png`) so the cheatsheet
/// labels every tile with its millisecond for physics matching.
fn save(
    shot: &cranpose::RobotScreenshot,
    shot_dir: &Path,
    component: &str,
    crop: (f32, f32, f32, f32),
    index: usize,
    ms: f32,
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
    let path = dir.join(format!("{index:02}-{:04}ms.png", ms.round() as i64));
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
