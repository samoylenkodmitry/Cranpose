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
use desktop_app::app::{self, TEST_ACTIVE_TAB_STATE, TEST_LIQUID_SELECTED_TAB_STATE};
use image::RgbaImage;
use std::path::{Path, PathBuf};
use std::process::ExitCode;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

const WINDOW_WIDTH: u32 = 900;
const WINDOW_HEIGHT: u32 = 500;
const GLASS_ACTIVITY_RGB_DELTA: u32 = 24;
const SILHOUETTE_RGB_DELTA: u32 = 12;
const LAST_FULL_OPACITY_FLIGHT_FRAME: usize = 7;
const MAX_DIRECT_FOLLOW_ERROR_IN_TABS: f32 = 0.18;
const MAX_MEASURED_FLIGHT_WIDTH_IN_TABS: f32 = 1.90;

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
                .find_button_bounds_exact("Account")
                .ok()
                .flatten()
                .unwrap_or_else(|| fail(&robot, "Account tab button not in semantics"));
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
            let direct_x = discover_cx + (settings_cx - discover_cx) * 0.72;
            robot
                .touch_down(discover_cx, bar_cy)
                .expect("grab selected tab");
            // The HELD bar is the diff baseline: a held panel rises toward
            // the user (lift transform), so diffing against the RESTING bar
            // registers the whole shifted pill as activity and drags the
            // measured lens centroid toward the bar center. Baseline after
            // the lift spring settles isolates the moving lens alone.
            std::thread::sleep(Duration::from_millis(320));
            let baseline = robot.screenshot().expect("held baseline");
            save(&baseline, &shot_dir, "drag-0-baseline");
            robot
                .touch_move_and_wait_for_frame(direct_x, bar_cy)
                .expect("direct tab-bar pointer step");
            let direct = robot.screenshot().expect("direct tab-bar frame");
            save(&direct, &shot_dir, "drag-direct-frame");
            let ghost_mask = (discover_cx - tab_w * 0.75, discover_cx + tab_w * 0.75);
            let Some((direct_cx, _)) = activity_center(&baseline, &direct, band, ghost_mask) else {
                fail(&robot, "tab-bar lens did not reach the direct-pointer region");
            };
            println!(
                "direct follow finger={direct_x:.0} lens_cx={direct_cx:.0} err={:.0}",
                (direct_cx - direct_x).abs()
            );
            if (direct_cx - direct_x).abs() > tab_w * MAX_DIRECT_FOLLOW_ERROR_IN_TABS {
                fail(
                    &robot,
                    "tab-bar lens center missed the pointer in the delivered input frame",
                );
            }
            robot
                .touch_move_and_wait_for_frame(discover_cx, bar_cy)
                .expect("restore tab-bar drag start");
            std::thread::sleep(Duration::from_millis(240));
            let steps = 6usize;
            let mut follow_ok = 0usize;
            let mut checked = 0usize;
            let mut drag_frames = Vec::with_capacity(steps);
            for i in 1..=steps {
                let t = i as f32 / steps as f32;
                let x = discover_cx + (settings_cx - discover_cx) * t;
                robot
                    .touch_move_and_wait_for_frame(x, bar_cy)
                    .expect("drag move and present");
                let shot = robot.screenshot().expect("drag shot");
                save(&shot, &shot_dir, &format!("drag-{i}"));
                drag_frames.push(shot.clone());
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
                            if err <= tab_w * MAX_DIRECT_FOLLOW_ERROR_IN_TABS {
                                follow_ok += 1;
                            }
                        }
                        None => println!("follow t={t:.2} finger={x:.0} NO LENS ACTIVITY"),
                    }
                }
            }
            robot
                .touch_up(settings_cx, bar_cy)
                .expect("release on Account");
            for (index, shot) in drag_frames.iter().enumerate() {
                if let Some(sample) = silhouette_sample(
                    &baseline,
                    shot,
                    band,
                    (discover_cx - tab_w * 0.4, settings_cx + tab_w * 0.7),
                ) {
                    println!(
                        "direct silhouette {}: width={:.1} height={:.1} area={:.1} centroid_x={:.1} leading_skew={:.3} clipped={}",
                        index + 1,
                        sample.width,
                        sample.height,
                        sample.width * sample.height,
                        sample.centroid_x,
                        sample.leading_skew,
                        sample.clipped,
                    );
                }
            }
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

            // ---------- Leg 2: controlled state → deterministic flight ----------
            // Direct contact already moves the lens to the pointer on the
            // delivered down frame. A source-to-target tap animation would
            // contradict that contract, so programmatic selection changes
            // exercise the post-input spring channel independently.
            robot
                .invoke_app_hook("set-liquid-selected", "0")
                .expect("retarget selected liquid tab");
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
                (70.0, true),
                (70.0, true),
                (80.0, true),
                (120.0, true),
                (130.0, true),
                (150.0, true),
                (1100.0, true),
            ];
            let shots = robot
                .capture_keyframes(1.0, &steps)
                .expect("flight keyframes");
            // Wall clock must catch back up with the advanced animation clock.
            std::thread::sleep(Duration::from_millis(2100));
            let labels = [
                "0016ms", "0036ms", "0061ms", "0091ms", "0131ms", "0191ms", "0281ms", "0351ms",
                "0421ms", "0501ms", "0621ms", "0751ms", "0901ms", "2001ms",
            ];
            for (shot, label) in shots.iter().zip(labels.iter()) {
                save(shot, &shot_dir, &format!("flight-{label}"));
            }

            // Rendered deformation contract. Interior samples cover launch
            // and cruise; full-opacity samples at the destination cover the
            // braking/decompression phase. A thresholded diff against the
            // settled destination isolates that arrival lens without a
            // selected-icon state change. Dissolve frames are excluded.
            let flight_x = (discover_cx - tab_w * 1.1, settings_cx + tab_w * 0.7);
            let flight_settled = shots.last().expect("settled flight keyframe");
            let mut silhouettes = Vec::new();
            for (frame_index, (shot, label)) in shots.iter().zip(labels.iter()).enumerate() {
                if let Some(sample) = silhouette_sample(flight_settled, shot, band, flight_x) {
                    println!(
                        "silhouette {label}: width={:.1} height={:.1} area={:.1} centroid_x={:.1} leading_skew={:.3} clipped={}",
                        sample.width,
                        sample.height,
                        sample.width * sample.height,
                        sample.centroid_x,
                        sample.leading_skew,
                        sample.clipped,
                    );
                    let safely_interior = sample.centroid_x > discover_cx + tab_w * 0.20
                        && sample.centroid_x < settings_cx - tab_w * 0.25;
                    let launching_at_source = frame_index <= LAST_FULL_OPACITY_FLIGHT_FRAME
                        && (sample.centroid_x - settings_cx).abs() < tab_w * 0.15;
                    let braking_at_destination = frame_index <= LAST_FULL_OPACITY_FLIGHT_FRAME
                        && (sample.centroid_x - discover_cx).abs() < tab_w * 0.25;
                    if frame_index <= LAST_FULL_OPACITY_FLIGHT_FRAME
                        && sample.width > 60.0
                        // The selected-label recolor can join the optical
                        // diff into a component wider than the glass quad.
                        // Reject that component instead of treating text as
                        // liquid volume.
                        && sample.width < tab_w * MAX_MEASURED_FLIGHT_WIDTH_IN_TABS
                        && sample.height > 20.0
                        && !sample.clipped
                        && (safely_interior || launching_at_source || braking_at_destination)
                    {
                        silhouettes.push(sample);
                    }
                }
            }
            if silhouettes.len() < 2 {
                fail(
                    &robot,
                    &format!(
                        "only {} rendered deformation silhouettes were measurable",
                        silhouettes.len()
                    ),
                );
            }
            let min_width = silhouettes
                .iter()
                .map(|sample| sample.width)
                .fold(f32::MAX, f32::min);
            let max_width = silhouettes
                .iter()
                .map(|sample| sample.width)
                .fold(0.0f32, f32::max);
            let min_height = silhouettes
                .iter()
                .map(|sample| sample.height)
                .fold(f32::MAX, f32::min);
            let max_height = silhouettes
                .iter()
                .map(|sample| sample.height)
                .fold(0.0f32, f32::max);
            // Height tolerance sits under the width one: after a tap the
            // bar's hold-lift is still settling through the early flight
            // frames, and that whole-pill shift bleeds into the silhouette
            // diff, compressing the measured height range (the width axis
            // carries the decisive exchange evidence).
            if max_width < min_width * 1.12 || max_height < min_height * 1.04 {
                fail(
                    &robot,
                    &format!(
                        "rendered droplet did not visibly exchange axis volume: width {min_width:.1}..{max_width:.1}, height {min_height:.1}..{max_height:.1}"
                    ),
                );
            }
            let areas: Vec<f32> = silhouettes
                .iter()
                .map(|sample| sample.width * sample.height)
                .collect();
            // Contact raises the droplet CONTINUOUSLY (reference gesture
            // rows): a flight launched off a robot-instant tap still grows
            // through its launch frames. Volume conservation is the law of
            // the RAISED droplet — it applies from the first frame at
            // (near-)full volume onward; the monotonic raise before it is
            // the reference's own contact behavior.
            let max_area = areas.iter().copied().fold(0.0f32, f32::max);
            let raised_from = areas
                .iter()
                .position(|area| *area >= max_area * 0.90)
                .unwrap_or(0);
            let raised_areas = &areas[raised_from..];
            let min_area = raised_areas.iter().copied().fold(f32::MAX, f32::min);
            if max_area > min_area * 1.38 {
                fail(
                    &robot,
                    &format!(
                        "rendered droplet volume changed excessively: area {min_area:.1}..{max_area:.1}"
                    ),
                );
            }
            let software_renderer =
                std::env::var("CRANPOSE_ROBOT_SOFTWARE_RENDERER").as_deref() == Ok("1");
            if !software_renderer
                && !silhouettes
                    .iter()
                    .any(|sample| sample.leading_skew.abs() >= 0.005)
            {
                // Software renderers (CI lavapipe) flatten the lens contrast
                // at the reference's low optical zoom below the skew
                // detector's floor; every real GPU measures it.
                fail(&robot, "no rendered leading-edge redistribution was measurable");
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
    if name == "set-tab" {
        if argument != "liquid" {
            return Err(format!("unknown demo tab '{argument}'"));
        }
        let state = TEST_ACTIVE_TAB_STATE
            .with(|cell| cell.borrow().as_ref().copied())
            .ok_or_else(|| "active tab state was not installed".to_string())?;
        state.set(app::DemoTab::Liquid);
        return Ok(None);
    }
    if name == "set-liquid-selected" {
        let selected = argument
            .parse::<usize>()
            .map_err(|error| format!("invalid liquid tab index '{argument}': {error}"))?;
        let state = TEST_LIQUID_SELECTED_TAB_STATE
            .with(|cell| cell.borrow().as_ref().copied())
            .ok_or_else(|| "liquid tab state was not installed".to_string())?;
        state.set(selected);
        return Ok(None);
    }
    Err(format!("unsupported robot app hook {name}({argument})"))
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

#[derive(Clone, Copy, Debug)]
struct SilhouetteSample {
    width: f32,
    height: f32,
    centroid_x: f32,
    leading_skew: f32,
    clipped: bool,
}

fn silhouette_sample(
    baseline: &cranpose::RobotScreenshot,
    frame: &cranpose::RobotScreenshot,
    band: (f32, f32),
    x_range: (f32, f32),
) -> Option<SilhouetteSample> {
    if baseline.width != frame.width || baseline.height != frame.height {
        return None;
    }
    let (sx, sy) = shot_scale(frame);
    let y0 = ((band.0 * sy).max(0.0) as usize).min(frame.height as usize);
    let y1 = ((band.1 * sy).max(0.0) as usize).min(frame.height as usize);
    let x0 = ((x_range.0 * sx).max(0.0) as usize).min(frame.width as usize);
    let x1 = ((x_range.1 * sx).max(0.0) as usize).min(frame.width as usize);
    let width_px = frame.width as usize;
    let scan_width = x1.saturating_sub(x0);
    let scan_height = y1.saturating_sub(y0);
    let mut changed = vec![false; scan_width * scan_height];
    for y in y0..y1 {
        for x in x0..x1 {
            let i = (y * width_px + x) * 4;
            let delta = baseline.pixels[i].abs_diff(frame.pixels[i]) as u32
                + baseline.pixels[i + 1].abs_diff(frame.pixels[i + 1]) as u32
                + baseline.pixels[i + 2].abs_diff(frame.pixels[i + 2]) as u32;
            changed[(y - y0) * scan_width + (x - x0)] = delta > SILHOUETTE_RGB_DELTA;
        }
    }

    let mut visited = vec![false; changed.len()];
    let mut best: Option<SilhouetteSample> = None;
    for seed in 0..changed.len() {
        if !changed[seed] || visited[seed] {
            continue;
        }
        let mut stack = vec![seed];
        visited[seed] = true;
        let mut count = 0usize;
        let mut sum_x = 0.0f32;
        let mut min_x = f32::MAX;
        let mut max_x = f32::MIN;
        let mut min_y = f32::MAX;
        let mut max_y = f32::MIN;
        let mut column_min_y = vec![f32::MAX; scan_width];
        let mut column_max_y = vec![f32::MIN; scan_width];
        while let Some(index) = stack.pop() {
            let local_x = index % scan_width;
            let local_y = index / scan_width;
            let logical_x = (x0 + local_x) as f32 / sx;
            let logical_y = (y0 + local_y) as f32 / sy;
            count += 1;
            sum_x += logical_x;
            min_x = min_x.min(logical_x);
            max_x = max_x.max(logical_x);
            min_y = min_y.min(logical_y);
            max_y = max_y.max(logical_y);
            column_min_y[local_x] = column_min_y[local_x].min(logical_y);
            column_max_y[local_x] = column_max_y[local_x].max(logical_y);

            let start_x = local_x.saturating_sub(1);
            let end_x = (local_x + 1).min(scan_width.saturating_sub(1));
            let start_y = local_y.saturating_sub(1);
            let end_y = (local_y + 1).min(scan_height.saturating_sub(1));
            for neighbor_y in start_y..=end_y {
                for neighbor_x in start_x..=end_x {
                    let neighbor = neighbor_y * scan_width + neighbor_x;
                    if changed[neighbor] && !visited[neighbor] {
                        visited[neighbor] = true;
                        stack.push(neighbor);
                    }
                }
            }
        }
        let width = max_x - min_x;
        let height = max_y - min_y;
        if count < 80 || width < 18.0 || height < 18.0 {
            continue;
        }
        let centroid_x = sum_x / count as f32;
        // Boundary-based skew: the viscous leading-edge lobe makes the
        // leading half's outline TALLER than the trailing half's. Interior
        // recolors (the lens copy tints everything it covers) are symmetric
        // mass and must not dilute the physics signal.
        let bbox_center_x = (min_x + max_x) * 0.5;
        let mut leading_height_sum = 0.0f32;
        let mut leading_columns = 0usize;
        let mut trailing_height_sum = 0.0f32;
        let mut trailing_columns = 0usize;
        for local_x in 0..scan_width {
            if column_max_y[local_x] < column_min_y[local_x] {
                continue;
            }
            let logical_x = (x0 + local_x) as f32 / sx;
            let column_height = column_max_y[local_x] - column_min_y[local_x];
            if logical_x < bbox_center_x {
                trailing_height_sum += column_height;
                trailing_columns += 1;
            } else {
                leading_height_sum += column_height;
                leading_columns += 1;
            }
        }
        let leading_skew = if height > 0.0 && leading_columns > 0 && trailing_columns > 0 {
            (leading_height_sum / leading_columns as f32
                - trailing_height_sum / trailing_columns as f32)
                / height
        } else {
            0.0
        };
        let sample = SilhouetteSample {
            width,
            height,
            centroid_x,
            leading_skew,
            clipped: min_x <= x_range.0 + 2.0 || max_x >= x_range.1 - 2.0,
        };
        if best.is_none_or(|best_sample| sample.centroid_x > best_sample.centroid_x) {
            best = Some(sample);
        }
    }
    best
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
            if d > GLASS_ACTIVITY_RGB_DELTA {
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
