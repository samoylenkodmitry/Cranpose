//! Liquid bubble physics contract: the tab-bar lens must FOLLOW the finger
//! while dragging (per-frame tracking, never a jump), and a tap must send it
//! FLYING through the intermediate tabs (continuous travel with the droplet
//! deformation law of `cranpose_liquid::dynamics`), settling clean.
//!
//! Programmatic pins here cover follow + flight continuity + settle hygiene;
//! the deformation SHAPE (stretch at cruise, launch compression, brake
//! bulge) is pinned by the dynamics unit tests and judged visually against
//! `example/target/tab-swipe` + the iphone17 recordings from the keyframes
//! this runner saves.
//!
//! Run with:
//! `cargo run --package desktop-app --example robot_liquid_bubble_physics --features desktop,robot-app`
//!
//! Screenshots land in `ROBOT_SHOT_DIR` (default `target/liquid-bubble`).

use cranpose::AppLauncher;
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
        std::env::var("ROBOT_SHOT_DIR").unwrap_or_else(|_| "target/liquid-bubble".to_string()),
    );
    std::fs::create_dir_all(&shot_dir).expect("create shot dir");

    AppLauncher::new()
        .with_title("Liquid Bubble Physics Contract")
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

            // ---------- Locate the floating bottom tab bar ----------
            // Exact BUTTON match: plain contains-text would hit list rows
            // whose copy mentions "Discover".
            let discover = robot
                .find_button_bounds_exact("Discover")
                .ok()
                .flatten()
                .unwrap_or_else(|| fail(&robot, "Discover tab button not in semantics"));
            let settings = robot
                .find_button_bounds_exact("Settings")
                .ok()
                .flatten()
                .unwrap_or_else(|| fail(&robot, "Settings tab button not in semantics"));
            let discover_cx = discover.0 + discover.2 * 0.5;
            let settings_cx = settings.0 + settings.2 * 0.5;
            let bar_cy = discover.1 + discover.3 * 0.5;
            let tab_w = (settings_cx - discover_cx).abs() / 3.0;
            // Lens band (logical): tall enough for the deformed lens poking
            // past the bar cell.
            let band = (
                (discover.1 - 34.0).max(0.0),
                (discover.1 + discover.3 + 10.0).min(WINDOW_HEIGHT as f32),
            );
            println!(
                "bar: discover_cx={discover_cx:.0} settings_cx={settings_cx:.0} tab_w={tab_w:.0} band=({:.0},{:.0})",
                band.0, band.1
            );

            // ---------- Leg 1: the lens follows the dragging finger ----------
            let baseline = robot.screenshot().expect("baseline");
            save(&baseline, &shot_dir, "drag-0-baseline");
            robot
                .touch_down(discover_cx, bar_cy)
                .expect("grab selected tab");
            std::thread::sleep(Duration::from_millis(160));
            let steps = 6usize;
            let mut follow_ok = 0usize;
            let mut checked = 0usize;
            for i in 1..=steps {
                let t = i as f32 / steps as f32;
                let x = discover_cx + (settings_cx - discover_cx) * t;
                robot.touch_move(x, bar_cy).expect("drag move");
                std::thread::sleep(Duration::from_millis(45));
                let shot = robot.screenshot().expect("drag shot");
                save(&shot, &shot_dir, &format!("drag-{i}"));
                // Activity vs the resting baseline: pill ghost stays near
                // Discover, so only judge samples ≥ 1.2 tabs out.
                if (x - discover_cx).abs() > tab_w * 1.2 {
                    let ghost_mask = (discover_cx - tab_w * 0.75, discover_cx + tab_w * 0.75);
                    checked += 1;
                    match activity_center(&baseline, &shot, band, ghost_mask) {
                        Some((cx, span)) => {
                            let err = (cx - x).abs();
                            println!(
                                "follow t={t:.2} finger={x:.0} lens_cx={cx:.0} err={err:.0} span={span:.0}"
                            );
                            if err <= tab_w * 0.75 {
                                follow_ok += 1;
                            }
                        }
                        None => println!("follow t={t:.2} finger={x:.0} NO LENS ACTIVITY"),
                    }
                }
            }
            robot
                .touch_up(settings_cx, bar_cy)
                .expect("release on Settings");
            if checked < 3 || follow_ok + 1 < checked {
                fail(
                    &robot,
                    &format!(
                        "lens does not track the finger: {follow_ok}/{checked} samples near the finger"
                    ),
                );
            }
            // Commit + full dissolve before the flight leg.
            settle(&robot, 1200);

            // ---------- Leg 2: tap → deterministic flight keyframes ----------
            robot.click(discover_cx, bar_cy).expect("tap Discover");
            // The glide transit runs ~330ms; sample launch, cruise, brake,
            // arrival swell, dissolve tail, and the fully settled end.
            let steps: Vec<(f32, bool)> = vec![
                (0.0, false),
                (1.0, false), // spring stamp frame
                (15.0, true),
                (20.0, true),
                (25.0, true),
                (30.0, true),
                (40.0, true),
                (60.0, true),
                (90.0, true),
                (140.0, true),
                (200.0, true),
                (280.0, true),
                (400.0, true),
                (700.0, true),
            ];
            let shots = robot
                .capture_keyframes(1.0, &steps)
                .expect("flight keyframes");
            // Wall clock must catch back up with the advanced animation clock.
            std::thread::sleep(Duration::from_millis(2100));
            let labels = [
                "0016ms", "0036ms", "0061ms", "0091ms", "0131ms", "0191ms", "0281ms", "0421ms",
                "0621ms", "0901ms", "1301ms", "2001ms",
            ];
            for (shot, label) in shots.iter().zip(labels.iter()) {
                save(shot, &shot_dir, &format!("flight-{label}"));
            }

            // Flight continuity via consecutive-keyframe diffs: static
            // recolors (selected icon/label) cancel, so only the MOVER — the
            // flying lens — registers. It must appear at ≥3 distinct
            // positions strictly between the tabs, sweeping monotonically
            // toward Discover.
            let mut centers: Vec<f32> = Vec::new();
            for k in 1..10usize {
                if let Some((cx, span)) =
                    activity_center(&shots[k - 1], &shots[k], band, (0.0, 0.0))
                {
                    println!(
                        "flight {}→{}: mover_cx={cx:.0} span={span:.0}",
                        labels[k - 1],
                        labels[k]
                    );
                    centers.push(cx);
                } else {
                    println!("flight {}→{}: no mover", labels[k - 1], labels[k]);
                }
            }
            if centers.len() < 4 {
                fail(
                    &robot,
                    &format!(
                        "flying lens invisible: only {} mover samples during the transit",
                        centers.len()
                    ),
                );
            }
            let interior: Vec<f32> = centers
                .iter()
                .copied()
                .filter(|cx| *cx < settings_cx - tab_w * 0.55 && *cx > discover_cx + tab_w * 0.55)
                .collect();
            if interior.len() < 3 {
                fail(
                    &robot,
                    &format!(
                        "flight teleports: only {} interior lens positions between the tabs",
                        interior.len()
                    ),
                );
            }
            if !centers.windows(2).all(|w| w[1] <= w[0] + tab_w * 0.25) {
                fail(&robot, &format!("flight not monotonic: {centers:?}"));
            }

            // Settle hygiene: the last two keyframes must agree (dissolve
            // complete, zero residue wobble).
            let residue = diff_count(
                &shots[shots.len() - 2],
                &shots[shots.len() - 1],
                band,
                (0.0, 0.0),
            );
            println!("settle residue={residue}");
            if residue > 600 {
                save(&shots[shots.len() - 1], &shot_dir, "settle-residue");
                fail(
                    &robot,
                    &format!("lens still moving 1.4s after the tap: {residue} px"),
                );
            }

            println!("PASS: liquid bubble physics contract");
            robot.exit().expect("exit");
        })
        .try_run(app::combined_app)
        .expect("launch liquid bubble physics runner");
    if FAILED.load(Ordering::Relaxed) {
        ExitCode::FAILURE
    } else {
        ExitCode::SUCCESS
    }
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

/// Record the failure, ask the app to shut down cleanly, and let `main`
/// return the failing exit code (process::exit here races surface teardown).
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

fn settle(robot: &cranpose::Robot, ms: u64) {
    std::thread::sleep(Duration::from_millis(ms));
    let _ = robot.wait_for_idle();
}

fn save(shot: &cranpose::RobotScreenshot, dir: &Path, name: &str) {
    let image = RgbaImage::from_raw(shot.width, shot.height, shot.pixels.clone())
        .expect("screenshot buffer");
    let path = dir.join(format!("{name}.png"));
    image.save(&path).expect("save screenshot");
    println!("  saved {}", path.display());
}

/// Semantic coords are logical; captures physical.
fn shot_scale(shot: &cranpose::RobotScreenshot) -> (f32, f32) {
    (
        shot.width as f32 / shot.logical_width.max(1.0),
        shot.height as f32 / shot.logical_height.max(1.0),
    )
}

/// Activity-weighted center x and column span (logical units) of pixels
/// differing between `a` and `b` inside the logical y band, excluding the
/// masked logical x range. `None` when the diff is noise-level.
fn activity_center(
    a: &cranpose::RobotScreenshot,
    b: &cranpose::RobotScreenshot,
    band: (f32, f32),
    mask_x: (f32, f32),
) -> Option<(f32, f32)> {
    let (count, sum_x, min_x, max_x) = scan_diff(a, b, band, mask_x);
    if count < 60 {
        return None;
    }
    Some((sum_x / count as f32, max_x - min_x))
}

fn diff_count(
    a: &cranpose::RobotScreenshot,
    b: &cranpose::RobotScreenshot,
    band: (f32, f32),
    mask_x: (f32, f32),
) -> usize {
    scan_diff(a, b, band, mask_x).0
}

fn scan_diff(
    a: &cranpose::RobotScreenshot,
    b: &cranpose::RobotScreenshot,
    band: (f32, f32),
    mask_x: (f32, f32),
) -> (usize, f32, f32, f32) {
    if a.width != b.width || a.height != b.height {
        return (0, 0.0, 0.0, 0.0);
    }
    let (sx, sy) = shot_scale(a);
    let y0 = ((band.0 * sy).max(0.0) as usize).min(a.height as usize);
    let y1 = ((band.1 * sy).max(0.0) as usize).min(a.height as usize);
    let mut count = 0usize;
    let mut sum_x = 0.0f32;
    let mut min_x = f32::MAX;
    let mut max_x = f32::MIN;
    let width = a.width as usize;
    for y in y0..y1 {
        for x in 0..width {
            let logical_x = x as f32 / sx;
            if logical_x >= mask_x.0 && logical_x <= mask_x.1 {
                continue;
            }
            let i = (y * width + x) * 4;
            let d = a.pixels[i].abs_diff(b.pixels[i]) as u32
                + a.pixels[i + 1].abs_diff(b.pixels[i + 1]) as u32
                + a.pixels[i + 2].abs_diff(b.pixels[i + 2]) as u32;
            if d > 42 {
                count += 1;
                sum_x += logical_x;
                if logical_x < min_x {
                    min_x = logical_x;
                }
                if logical_x > max_x {
                    max_x = logical_x;
                }
            }
        }
    }
    (count, sum_x, min_x, max_x)
}
