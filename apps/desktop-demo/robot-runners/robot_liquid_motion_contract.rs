//! Liquid motion contracts, pinned to the reference recordings:
//! - the menu opens as a GROWING droplet (small early, full at settle, no
//!   fade-in of a full-size card), closes by deflating back into the anchor,
//!   and leaves zero residue;
//! - pressing the toggle dissolves the white thumb into a magnifying lens
//!   that overflows the track (grow + transparency, never shrink);
//! - releasing settles back to a white thumb;
//! - thumb shadows stay a whisper (nothing dark under resting thumbs);
//! - pressing a glass button GROWS it and ghosts its label.

use cranpose::AppLauncher;
use cranpose_testing::find_text_in_semantics;
use desktop_app::app::{self, TEST_ACTIVE_TAB_STATE};
use image::RgbaImage;
use std::path::{Path, PathBuf};
use std::process::ExitCode;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

const WINDOW_WIDTH: u32 = 900;
const WINDOW_HEIGHT: u32 = 800;

static FAILED: AtomicBool = AtomicBool::new(false);

fn main() -> ExitCode {
    let _ = env_logger::try_init();
    let shot_dir = PathBuf::from(
        std::env::var("ROBOT_SHOT_DIR").unwrap_or_else(|_| "target/liquid-motion".to_string()),
    );
    std::fs::create_dir_all(&shot_dir).expect("create shot dir");

    AppLauncher::new()
        .with_title("Liquid Motion Contract")
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

            // ---------- Menu morph: growing droplet, clean close ----------
            let clean = robot.screenshot().expect("clean shot");
            if find_text_in_semantics(&robot, "Only Unwatched").is_some() {
                fail(&robot, "menu items in semantics before opening the menu");
            }
            robot.click(858.0, 122.0).expect("open menu");
            // Exact-clock samples of the whole droplet growth (a wall-clock
            // sleep stretches under host load and lands past the
            // small-droplet phase): the 54ms sample feeds the fade-in guard,
            // the rest are the judge's A/B growth keyframes.
            let grow_steps: Vec<(f32, bool)> = vec![
                (0.0, false),
                (1.0, false),
                (20.0, true),
                (16.0, true),
                (17.0, true),
                (21.0, true),
                (25.0, true),
                (30.0, true),
                (35.0, true),
                (40.0, true),
                (60.0, true),
                (65.0, true),
                (170.0, true),
            ];
            let grow_shots = robot
                .capture_keyframes(1.0, &grow_steps)
                .expect("grow keyframes");
            std::thread::sleep(Duration::from_millis(600));
            let grow_labels = [
                "021ms", "037ms", "054ms", "075ms", "100ms", "130ms", "165ms", "205ms", "265ms",
                "330ms", "500ms",
            ];
            for (shot, label) in grow_shots.iter().zip(grow_labels.iter()) {
                save(shot, &shot_dir, &format!("menu-grow-{label}"));
            }
            let early = grow_shots.into_iter().nth(2).expect("54ms keyframe");
            // Materialization: the popup's items land in semantics once it
            // is composed — poll that instead of a fixed wall-clock wait so
            // host throttling cannot race the capture. Pixel diffs stay for
            // the shape checks, but the menu card is white-on-white against
            // the page, so diff area measures content/shadow, not presence.
            let mut menu_seen = false;
            for _ in 0..40 {
                if find_text_in_semantics(&robot, "Only Unwatched").is_some() {
                    menu_seen = true;
                    break;
                }
                std::thread::sleep(Duration::from_millis(100));
            }
            if !menu_seen {
                fail(&robot, "menu items never appeared in semantics after click");
            }
            settle(&robot, 500);
            let open = robot.screenshot().expect("open shot");
            // The menu region (right-aligned card under the nav trailing
            // button): compare against the clean page.
            let region = (560usize, 90usize, 890usize, 420usize);
            let early_area = diff_area(&clean, &early, region);
            let open_area = diff_area(&clean, &open, region);
            println!("menu-grow early_area={early_area} open_area={open_area}");
            if open_area < 800 {
                save(&open, &shot_dir, "menu-open");
                fail(&robot, "open menu drew almost no pixels over the clean page");
            }
            if early_area * 2 > open_area {
                save(&early, &shot_dir, "menu-early");
                save(&open, &shot_dir, "menu-open");
                fail(
                    &robot,
                    &format!(
                        "menu open is a fade-in, not a growing droplet: 55ms area {early_area} vs settled {open_area}"
                    ),
                );
            }
            // Close: deflates back into the anchor, then zero residue. The
            // ~140ms deflate MUST be sampled on the exact animation clock —
            // a wall-clock sleep stretches under host load and the first
            // pumped frame then advances the spring past the whole morph
            // (the capture lands on an already-closed menu and flakes).
            robot.click(200.0, 500.0).expect("dismiss menu");
            let close_shots = robot
                .capture_keyframes(
                    1.0,
                    &[(0.0, false), (1.0, false), (45.0, true), (800.0, true)],
                )
                .expect("close keyframes");
            // Let wall time catch back up with the advanced animation clock.
            settle(&robot, 900);
            let closing = &close_shots[0];
            let closed = &close_shots[1];
            let closing_area = diff_area(&clean, closing, region);
            let closed_area = diff_area(&clean, closed, region);
            println!("menu-close closing_area={closing_area} closed_area={closed_area}");
            if closing_area < 200 {
                fail(
                    &robot,
                    &format!(
                        "menu vanished (almost) instantly on dismiss — no close morph ({closing_area})"
                    ),
                );
            }
            if closing_area * 10 > open_area * 9 {
                save(closing, &shot_dir, "menu-closing");
                fail(
                    &robot,
                    &format!(
                        "menu not deflating 46ms after dismiss: area {closing_area} vs open {open_area}"
                    ),
                );
            }
            if closed_area > 160 {
                save(closed, &shot_dir, "menu-closed-residue");
                fail(
                    &robot,
                    &format!("menu left {closed_area} changed pixels after closing"),
                );
            }

            // ---------- Toggle: press = lens (grow + dissolve thumb) ----------
            scroll(&robot, 450.0, 500.0, -520.0);
            settle(&robot, 900);
            let Some((_, wy, _, wh)) = find_text_in_semantics(&robot, "Wi-Fi") else {
                fail(&robot, "Wi-Fi row not found in semantics");
            };
            let toggle_y = wy + wh * 0.5;
            // The switch hugs the card's trailing edge; the track's right
            // edge sits at the card padding (window 900: card right ≈ 844).
            let track_right = 844.0f32;
            let track_left = track_right - 63.0;
            let rest = robot.screenshot().expect("toggle rest");
            // The ON thumb: white capsule at the track's right.
            let thumb_probe = (
                (track_right - 20.0) as usize,
                (toggle_y - 8.0) as usize,
                (track_right - 4.0) as usize,
                (toggle_y + 8.0) as usize,
            );
            let rest_white = count_white(&rest, thumb_probe);
            println!("toggle rest_white={rest_white}");
            if rest_white < 40 {
                save(&rest, &shot_dir, "toggle-rest");
                fail(&robot, "resting toggle thumb is not a white capsule");
            }
            // Shadow contract: the band under the track must stay bright on
            // the white card (no dark blob under the thumb).
            let under = (
                track_left as usize,
                (toggle_y + 16.0) as usize,
                track_right as usize,
                (toggle_y + 26.0) as usize,
            );
            let darkest = min_luma(&rest, under);
            println!("toggle under-band darkest={darkest}");
            if darkest < 175 {
                save(&rest, &shot_dir, "toggle-shadow");
                fail(
                    &robot,
                    &format!("dark shadow under resting toggle thumb (min luma {darkest})"),
                );
            }
            robot
                .mouse_move(track_right - 20.0, toggle_y)
                .expect("hover thumb");
            robot.mouse_down().expect("press thumb");
            // Exact-clock press-grow keyframes: judge frames for the lens
            // materialization; the 321ms sample doubles as the pressed shot.
            let toggle_steps: Vec<(f32, bool)> = vec![
                (0.0, false),
                (1.0, false),
                (20.0, true),
                (20.0, true),
                (25.0, true),
                (35.0, true),
                (60.0, true),
                (80.0, true),
                (80.0, true),
            ];
            let toggle_shots = robot
                .capture_keyframes(1.0, &toggle_steps)
                .expect("toggle grow keyframes");
            std::thread::sleep(Duration::from_millis(500));
            let toggle_labels = [
                "021ms", "041ms", "066ms", "101ms", "161ms", "241ms", "321ms",
            ];
            for (shot, label) in toggle_shots.iter().zip(toggle_labels.iter()) {
                save(shot, &shot_dir, &format!("toggle-press-{label}"));
            }
            let pressed = toggle_shots.into_iter().last().expect("321ms keyframe");
            let pressed_white = count_white(&pressed, thumb_probe);
            // Lens overflow: pixels just above the track must change while
            // the lens is up (it pokes past the track edge — its rim arc
            // sits ~5-7dp above the track top).
            let above = (
                (track_right - 52.0) as usize,
                (toggle_y - 25.0) as usize,
                (track_right + 6.0) as usize,
                (toggle_y - 12.0) as usize,
            );
            let overflow = region_diff(&rest, &pressed, above);
            println!("toggle pressed_white={pressed_white} overflow={overflow}");
            if pressed_white * 3 > rest_white {
                save(&pressed, &shot_dir, "toggle-pressed");
                fail(
                    &robot,
                    &format!(
                        "pressed toggle keeps its opaque white thumb ({pressed_white} white px) — the lens must dissolve it"
                    ),
                );
            }
            if overflow < 30 {
                save(&pressed, &shot_dir, "toggle-pressed");
                fail(
                    &robot,
                    "pressed toggle lens does not overflow the track (no growth visible)",
                );
            }
            robot.mouse_up().expect("release thumb");
            // Release-settle keyframes: the reference lens lingers ~0.6s
            // while the thumb flies and the white capsule rematerializes.
            let release_steps: Vec<(f32, bool)> = vec![
                (0.0, false),
                (1.0, false),
                (50.0, true),
                (70.0, true),
                (110.0, true),
                (170.0, true),
                (250.0, true),
                (550.0, true),
            ];
            let release_shots = robot
                .capture_keyframes(1.0, &release_steps)
                .expect("toggle release keyframes");
            settle(&robot, 1600);
            let release_labels = ["051ms", "121ms", "231ms", "401ms", "651ms", "1201ms"];
            for (shot, label) in release_shots.iter().zip(release_labels.iter()) {
                save(shot, &shot_dir, &format!("toggle-release-{label}"));
            }
            let after = release_shots.into_iter().last().expect("settled keyframe");
            // The tap flipped it OFF: the white thumb now rests at the LEFT.
            let thumb_probe_off = (
                (track_left + 4.0) as usize,
                (toggle_y - 8.0) as usize,
                (track_left + 20.0) as usize,
                (toggle_y + 8.0) as usize,
            );
            let settled_white = count_white(&after, thumb_probe_off);
            println!("toggle settled_white={settled_white}");
            if settled_white < 40 {
                save(&after, &shot_dir, "toggle-settled");
                fail(&robot, "toggle thumb did not rematerialize after release");
            }

            // ---------- Toggle: the lens FOLLOWS the finger every frame ----
            // Reference: the glass bubble rides the drag continuously (a
            // frozen/stuck bubble was the density-vs-render-scale packing
            // bug). Drag OFF -> ON in steps and check the changed-region
            // centroid tracks the pointer monotonically.
            let rest_off = robot.screenshot().expect("toggle drag rest");
            let track_region = (
                (track_left - 30.0) as usize,
                (toggle_y - 30.0) as usize,
                (track_right + 30.0) as usize,
                (toggle_y + 30.0) as usize,
            );
            robot
                .mouse_move(track_left + 12.0, toggle_y)
                .expect("hover off thumb");
            robot.mouse_down().expect("press off thumb");
            std::thread::sleep(Duration::from_millis(260));
            let mut centroids = Vec::new();
            for step in 0..3 {
                let x = track_left + 12.0 + (step as f32 + 1.0) * 13.0;
                robot.mouse_move(x, toggle_y).expect("drag step");
                std::thread::sleep(Duration::from_millis(120));
                let shot = robot.screenshot().expect("drag shot");
                // Judge frames: OFF-side press dragged toward ON — the lens
                // magnifies the split-track boundary mid-face (the reference
                // segment's money shot).
                save(&shot, &shot_dir, &format!("toggle-drag-{step}"));
                let Some(cx) = diff_centroid_x(&rest_off, &shot, track_region) else {
                    save(&shot, &shot_dir, "toggle-drag-blank");
                    fail(&robot, "toggle drag produced no visible lens change");
                };
                centroids.push(cx);
            }
            robot.mouse_up().expect("release drag");
            // Release linger keyframes for the judge (exact clock).
            let drag_release_shots = robot
                .capture_keyframes(
                    1.0,
                    &[
                        (0.0, false),
                        (1.0, false),
                        (60.0, true),
                        (120.0, true),
                        (180.0, true),
                        (240.0, true),
                    ],
                )
                .expect("drag release keyframes");
            settle(&robot, 1200);
            for (shot, label) in drag_release_shots
                .iter()
                .zip(["061ms", "181ms", "361ms", "601ms"].iter())
            {
                save(shot, &shot_dir, &format!("toggle-drag-release-{label}"));
            }
            println!("toggle drag centroids={centroids:?}");
            if !(centroids[1] > centroids[0] + 2.0 && centroids[2] > centroids[1] + 2.0) {
                fail(
                    &robot,
                    &format!(
                        "toggle lens does not follow the finger (centroids {centroids:?})"
                    ),
                );
            }

            // ---------- Segmented control: swipe + glass lens ----------
            let Some((seg_x, seg_y, seg_w, seg_h)) = find_text_in_semantics(&robot, "Receipts")
            else {
                fail(&robot, "'Receipts' segment not found in semantics");
            };
            let seg_cy = seg_y + seg_h * 0.5;
            let control_left = seg_x - seg_w;
            let control_right = seg_x + seg_w * 2.0;
            let seg_region = (
                control_left as usize,
                (seg_cy - 30.0) as usize,
                control_right as usize,
                (seg_cy + 30.0) as usize,
            );
            let seg_rest = robot.screenshot().expect("segmented rest");
            // Press on the selected "All" third and drag to the "Docs" third.
            robot
                .mouse_move(control_left + seg_w * 0.5, seg_cy)
                .expect("hover segmented");
            robot.mouse_down().expect("press segmented");
            std::thread::sleep(Duration::from_millis(260));
            let seg_down = robot.screenshot().expect("segmented down");
            let down_diff = region_diff(&seg_rest, &seg_down, seg_region);
            for step in 1..=5 {
                robot
                    .mouse_move(control_left + seg_w * 0.5 + step as f32 * seg_w * 0.4, seg_cy)
                    .expect("segmented drag");
                std::thread::sleep(Duration::from_millis(40));
            }
            std::thread::sleep(Duration::from_millis(180));
            let seg_mid = robot.screenshot().expect("segmented mid");
            // Measure the lens in the control's RIGHT half only: the left
            // half also changed (the resting indicator vanished), which
            // would dilute a whole-control centroid.
            let seg_right_region = (
                (control_left + seg_w * 1.2) as usize,
                seg_region.1,
                seg_region.2,
                seg_region.3,
            );
            let mid_cx = diff_centroid_x(&seg_rest, &seg_mid, seg_right_region);
            robot.mouse_up().expect("release segmented");
            settle(&robot, 1200);
            println!("segmented down_diff={down_diff} mid_cx={mid_cx:?}");
            if down_diff < 200 {
                save(&seg_down, &shot_dir, "segmented-down");
                fail(
                    &robot,
                    &format!("segmented press shows no lens (diff {down_diff})"),
                );
            }
            if mid_cx.is_none() {
                save(&seg_mid, &shot_dir, "segmented-mid");
                fail(
                    &robot,
                    "segmented lens did not chase the drag into the right half",
                );
            }
            // The drag committed a new segment: restore "All" for idempotence.
            robot
                .mouse_move(control_left + seg_w * 0.5, seg_cy)
                .expect("restore segmented");
            robot.click(control_left + seg_w * 0.5, seg_cy).expect("restore all");
            settle(&robot, 800);

            // ---------- Slider: the thumb liquifies on touch and drag ------
            let Some((_, air_y, _, air_h)) = find_text_in_semantics(&robot, "Airplane Mode")
            else {
                fail(&robot, "'Airplane Mode' row not found in semantics");
            };
            // The slider sits below the Airplane row inside the same card.
            let slider_y = air_y + air_h + 26.0;
            let slider_region = (
                60usize,
                (slider_y - 26.0) as usize,
                840usize,
                (slider_y + 26.0) as usize,
            );
            let slider_rest = robot.screenshot().expect("slider rest");
            // The demo slider starts at 0.55: the thumb is near mid-track.
            let thumb_guess = 56.0 + 0.55 * (844.0 - 56.0);
            robot
                .mouse_move(thumb_guess, slider_y)
                .expect("hover slider thumb");
            robot.mouse_down().expect("press slider");
            std::thread::sleep(Duration::from_millis(260));
            let slider_down = robot.screenshot().expect("slider down");
            let slider_diff = region_diff(&slider_rest, &slider_down, slider_region);
            for step in 1..=4 {
                robot
                    .mouse_move(thumb_guess + step as f32 * 22.0, slider_y)
                    .expect("slider drag");
                std::thread::sleep(Duration::from_millis(40));
            }
            std::thread::sleep(Duration::from_millis(180));
            let slider_mid = robot.screenshot().expect("slider mid");
            // Only right of the press point: the old thumb spot also changed.
            let slider_right_region = (
                (thumb_guess + 26.0) as usize,
                slider_region.1,
                slider_region.2,
                slider_region.3,
            );
            let slider_cx = diff_centroid_x(&slider_rest, &slider_mid, slider_right_region);
            robot.mouse_up().expect("release slider");
            settle(&robot, 1200);
            println!("slider down_diff={slider_diff} mid_cx={slider_cx:?}");
            if slider_diff < 120 {
                save(&slider_down, &shot_dir, "slider-down");
                fail(
                    &robot,
                    &format!("slider press shows no lens (diff {slider_diff})"),
                );
            }
            if slider_cx.is_none() {
                save(&slider_mid, &shot_dir, "slider-mid");
                fail(&robot, "slider lens did not ride the drag rightward");
            }

            // ---------- Glass button press: grow + label ghost ----------
            let Some((bx, by, bw, bh)) = find_text_in_semantics(&robot, "Glass") else {
                fail(&robot, "'Glass' button not found in semantics");
            };
            let pad = 14.0;
            let button_region = (
                (bx - pad) as usize,
                (by - pad) as usize,
                (bx + bw + pad) as usize,
                (by + bh + pad) as usize,
            );
            let rest = robot.screenshot().expect("button rest");
            let rest_ink = count_dark(&rest, button_region);
            robot
                .mouse_move(bx + bw * 0.5, by + bh * 0.5)
                .expect("hover button");
            robot.mouse_down().expect("press button");
            std::thread::sleep(Duration::from_millis(300));
            let pressed = robot.screenshot().expect("button pressed");
            let pressed_ink = count_dark(&pressed, button_region);
            let grew = region_diff(&rest, &pressed, button_region);
            robot.mouse_up().expect("release button");
            println!("button rest_ink={rest_ink} pressed_ink={pressed_ink} diff={grew}");
            if pressed_ink * 10 > rest_ink * 9 {
                save(&pressed, &shot_dir, "button-pressed");
                fail(
                    &robot,
                    &format!(
                        "pressed glass button label did not ghost (ink {rest_ink} -> {pressed_ink})"
                    ),
                );
            }
            if grew < 60 {
                save(&pressed, &shot_dir, "button-pressed");
                fail(&robot, "pressed glass button shows no growth");
            }

            // ---------- Nav bar: large title never rests mid-band ----------
            // Reference behavior: releasing (or wheel-idling) inside the
            // large-title collapse band snaps to an edge — the scroll can only
            // settle at 0 (fully expanded) or the collapse range (52), never
            // between (which showed both WWDC titles half-faded at once).
            // Scroll offset is measured via the first card's semantic y.
            scroll(&robot, 450.0, 400.0, 2400.0);
            settle(&robot, 900);
            let Some((_, card_y_top, _, _)) = find_text_in_semantics(&robot, "iPadOS") else {
                fail(&robot, "'iPadOS' card not found in semantics");
            };
            let inline_region = (380usize, 102usize, 520usize, 142usize);
            let expanded = robot.screenshot().expect("nav expanded");
            let expanded_inline = count_dark(&expanded, inline_region);
            println!("nav expanded card_y={card_y_top} inline_ink={expanded_inline}");
            if expanded_inline > 120 {
                save(&expanded, &shot_dir, "nav-expanded");
                fail(&robot, "expanded nav must not show the inline title");
            }

            // Drag up into the band's upper half, rest, release: must snap
            // COLLAPSED (and the rest before lift must not phantom-fling).
            robot.mouse_move(450.0, 400.0).expect("nav drag hover");
            robot.mouse_down().expect("nav drag press");
            for step in 1..=6 {
                robot
                    .mouse_move(450.0, 400.0 - step as f32 * 7.0)
                    .expect("nav drag move");
                std::thread::sleep(Duration::from_millis(16));
            }
            std::thread::sleep(Duration::from_millis(260));
            robot.mouse_up().expect("nav drag release");
            settle(&robot, 1100);
            let Some((_, card_y_snapped, _, _)) = find_text_in_semantics(&robot, "iPadOS") else {
                fail(&robot, "'iPadOS' card lost after snap drag");
            };
            let snapped_offset = card_y_top - card_y_snapped;
            let snapped = robot.screenshot().expect("nav snapped");
            let snapped_inline = count_dark(&snapped, inline_region);
            println!("nav snapped offset={snapped_offset} inline_ink={snapped_inline}");
            if (snapped_offset - 52.0).abs() > 1.5 {
                save(&snapped, &shot_dir, "nav-midband");
                fail(
                    &robot,
                    &format!(
                        "release in the band's upper half must settle at the collapse range (52), rested at {snapped_offset}"
                    ),
                );
            }
            if snapped_inline < 120 {
                save(&snapped, &shot_dir, "nav-snapped");
                fail(
                    &robot,
                    &format!(
                        "collapsed nav must show the inline title (ink {snapped_inline})"
                    ),
                );
            }

            // Drag back down inside the band's lower half: must snap EXPANDED.
            robot.mouse_move(450.0, 400.0).expect("nav return hover");
            robot.mouse_down().expect("nav return press");
            for step in 1..=5 {
                robot
                    .mouse_move(450.0, 400.0 + step as f32 * 7.0)
                    .expect("nav return move");
                std::thread::sleep(Duration::from_millis(16));
            }
            std::thread::sleep(Duration::from_millis(260));
            robot.mouse_up().expect("nav return release");
            settle(&robot, 1100);
            let Some((_, card_y_expanded, _, _)) = find_text_in_semantics(&robot, "iPadOS") else {
                fail(&robot, "'iPadOS' card lost after return drag");
            };
            let return_offset = card_y_top - card_y_expanded;
            let expanded_again = robot.screenshot().expect("nav re-expanded");
            let again_inline = count_dark(&expanded_again, inline_region);
            println!("nav re-expanded offset={return_offset} inline_ink={again_inline}");
            if return_offset.abs() > 1.5 {
                save(&expanded_again, &shot_dir, "nav-return");
                fail(
                    &robot,
                    &format!(
                        "release in the band's lower half must settle expanded (0), rested at {return_offset}"
                    ),
                );
            }
            if again_inline > 120 {
                save(&expanded_again, &shot_dir, "nav-return-inline");
                fail(
                    &robot,
                    "expanded nav must not show the inline title after the return drag",
                );
            }

            println!("PASS: liquid motion contract");
            robot.exit().expect("exit");
        })
        .try_run(app::combined_app)
        .expect("launch liquid motion runner");
    if FAILED.load(Ordering::Relaxed) {
        ExitCode::FAILURE
    } else {
        ExitCode::SUCCESS
    }
}

/// Record the failure, ask the app to shut down cleanly, and let `main`
/// return the failing exit code. Calling `process::exit` from this driver
/// thread races the main thread's surface teardown and dumps core (139),
/// masking the real failure. The watchdog covers a hung shutdown.
fn fail(robot: &cranpose::Robot, message: &str) -> ! {
    println!("FATAL: {message}");
    FAILED.store(true, Ordering::Relaxed);
    std::thread::spawn(|| {
        std::thread::sleep(Duration::from_secs(15));
        std::process::exit(1);
    });
    let _ = robot.exit();
    loop {
        std::thread::sleep(Duration::from_secs(60));
    }
}

/// Scaled sampling helpers: semantic coords are logical; captures physical.
fn scale(shot: &cranpose::RobotScreenshot) -> (f32, f32) {
    (
        shot.width as f32 / shot.logical_width.max(1.0),
        shot.height as f32 / shot.logical_height.max(1.0),
    )
}

fn diff_area(
    a: &cranpose::RobotScreenshot,
    b: &cranpose::RobotScreenshot,
    region: (usize, usize, usize, usize),
) -> usize {
    region_diff(a, b, region)
}

/// Count pixels in the LOGICAL region whose color differs noticeably.
fn region_diff(
    a: &cranpose::RobotScreenshot,
    b: &cranpose::RobotScreenshot,
    region: (usize, usize, usize, usize),
) -> usize {
    let (sx, sy) = scale(a);
    let (x0, y0, x1, y1) = region;
    let (x0, y0, x1, y1) = (
        (x0 as f32 * sx) as usize,
        (y0 as f32 * sy) as usize,
        ((x1 as f32 * sx) as usize).min(a.width as usize),
        ((y1 as f32 * sy) as usize).min(a.height as usize),
    );
    let mut diff = 0usize;
    for y in (y0..y1).step_by(2) {
        for x in (x0..x1).step_by(2) {
            let i = (y * a.width as usize + x) * 4;
            let d = a.pixels[i]
                .abs_diff(b.pixels[i])
                .max(a.pixels[i + 1].abs_diff(b.pixels[i + 1]))
                .max(a.pixels[i + 2].abs_diff(b.pixels[i + 2]));
            // Glass on a light page moves pixels subtly — a low threshold
            // is required to see the material itself, not just the text.
            if d > 12 {
                diff += 1;
            }
        }
    }
    diff
}

/// Mean x (logical) of the pixels that changed noticeably between the shots
/// inside the region — where the moving lens currently sits.
fn diff_centroid_x(
    a: &cranpose::RobotScreenshot,
    b: &cranpose::RobotScreenshot,
    region: (usize, usize, usize, usize),
) -> Option<f32> {
    let (sx, sy) = scale(a);
    let (x0, y0, x1, y1) = region;
    let (px0, py0, px1, py1) = (
        (x0 as f32 * sx) as usize,
        (y0 as f32 * sy) as usize,
        ((x1 as f32 * sx) as usize).min(a.width as usize),
        ((y1 as f32 * sy) as usize).min(a.height as usize),
    );
    let mut sum_x = 0f64;
    let mut count = 0usize;
    for y in (py0..py1).step_by(2) {
        for x in (px0..px1).step_by(2) {
            let i = (y * a.width as usize + x) * 4;
            let d = a.pixels[i]
                .abs_diff(b.pixels[i])
                .max(a.pixels[i + 1].abs_diff(b.pixels[i + 1]))
                .max(a.pixels[i + 2].abs_diff(b.pixels[i + 2]));
            if d > 12 {
                sum_x += x as f64;
                count += 1;
            }
        }
    }
    (count >= 20).then(|| (sum_x / count as f64) as f32 / sx)
}

fn count_white(shot: &cranpose::RobotScreenshot, region: (usize, usize, usize, usize)) -> usize {
    count_matching(shot, region, |r, g, b| r > 243 && g > 243 && b > 243)
}

fn count_dark(shot: &cranpose::RobotScreenshot, region: (usize, usize, usize, usize)) -> usize {
    count_matching(shot, region, |r, g, b| {
        (r as u16 + g as u16 + b as u16) < 420
    })
}

fn min_luma(shot: &cranpose::RobotScreenshot, region: (usize, usize, usize, usize)) -> u8 {
    let mut min = u8::MAX;
    let (sx, sy) = scale(shot);
    let (x0, y0, x1, y1) = region;
    let (x0, y0, x1, y1) = (
        (x0 as f32 * sx) as usize,
        (y0 as f32 * sy) as usize,
        ((x1 as f32 * sx) as usize).min(shot.width as usize),
        ((y1 as f32 * sy) as usize).min(shot.height as usize),
    );
    for y in y0..y1 {
        for x in x0..x1 {
            let i = (y * shot.width as usize + x) * 4;
            let luma = ((shot.pixels[i] as u16 * 54
                + shot.pixels[i + 1] as u16 * 183
                + shot.pixels[i + 2] as u16 * 19)
                / 256) as u8;
            min = min.min(luma);
        }
    }
    min
}

fn count_matching(
    shot: &cranpose::RobotScreenshot,
    region: (usize, usize, usize, usize),
    predicate: impl Fn(u8, u8, u8) -> bool,
) -> usize {
    let (sx, sy) = scale(shot);
    let (x0, y0, x1, y1) = region;
    let (x0, y0, x1, y1) = (
        (x0 as f32 * sx) as usize,
        (y0 as f32 * sy) as usize,
        ((x1 as f32 * sx) as usize).min(shot.width as usize),
        ((y1 as f32 * sy) as usize).min(shot.height as usize),
    );
    let mut count = 0usize;
    for y in y0..y1 {
        for x in x0..x1 {
            let i = (y * shot.width as usize + x) * 4;
            if predicate(shot.pixels[i], shot.pixels[i + 1], shot.pixels[i + 2]) {
                count += 1;
            }
        }
    }
    count
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

fn scroll(robot: &cranpose::Robot, x: f32, y: f32, delta_y: f32) {
    robot.mouse_move(x, y).expect("move cursor");
    robot.mouse_scroll(0.0, delta_y).expect("scroll");
}

fn save(shot: &cranpose::RobotScreenshot, dir: &Path, name: &str) {
    let image = RgbaImage::from_raw(shot.width, shot.height, shot.pixels.clone())
        .expect("screenshot buffer");
    let path = dir.join(format!("{name}.png"));
    image.save(&path).expect("save screenshot");
    println!("saved {}", path.display());
}
