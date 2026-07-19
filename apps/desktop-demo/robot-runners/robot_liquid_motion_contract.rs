//! Liquid motion contracts, pinned to the reference recordings:
//! - the menu opens as a GROWING droplet (height arrives before width, no
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
const TOGGLE_TRACK_WIDTH: f32 = 63.0;
const TOGGLE_TRACK_HEIGHT: f32 = 28.0;
const TOGGLE_LENS_WIDTH: f32 = 58.0;

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
            // The menu's "..." anchor lives in the Featured videos card
            // header at rest scroll.
            let card = robot
                .find_button_bounds_exact("Featured videos")
                .ok()
                .flatten()
                .unwrap_or_else(|| fail(&robot, "featured videos card not in semantics"));
            let menu_anchor = (card.0 + card.2 - 34.0, card.1 + 34.0);
            let clean = robot.screenshot().expect("clean shot");
            if find_text_in_semantics(&robot, "Grid").is_some() {
                fail(&robot, "menu items in semantics before opening the menu");
            }
            robot
                .click(menu_anchor.0, menu_anchor.1)
                .expect("open menu");
            // Exact-clock samples of the whole droplet growth (a wall-clock
            // sleep stretches under host load and lands past the opening
            // phase): 75ms pins the departing source, 104ms the horizontal
            // pill, 181ms the front-loaded body, and 459ms settle.
            let grow_steps: Vec<(f32, bool)> = vec![
                // The reference-true grow clock (springs 62/26) stretches
                // spring time by sqrt(120/62) ≈ 1.39 over the old pins;
                // these offsets sample the SAME spring phases as before, so
                // the fraction ranges below keep their meaning.
                (0.0, false),
                (1.0, false),
                (28.0, true),
                (22.0, true),
                (24.0, true),
                (29.0, true),
                (35.0, true),
                (42.0, true),
                (21.0, true),
                (28.0, true),
                (56.0, true),
                (83.0, true),
                (90.0, true),
                (236.0, true),
            ];
            let grow_shots = robot
                .capture_keyframes(1.0, &grow_steps)
                .expect("grow keyframes");
            std::thread::sleep(Duration::from_millis(600));
            let grow_labels = [
                "029ms", "051ms", "075ms", "104ms", "139ms", "181ms", "202ms", "230ms",
                "286ms", "369ms", "459ms", "695ms",
            ];
            for (shot, label) in grow_shots.iter().zip(grow_labels.iter()) {
                save(shot, &shot_dir, &format!("menu-grow-{label}"));
            }
            let absorbed = &grow_shots[2];
            let wide_oval = &grow_shots[3];
            let mid_growth = &grow_shots[5];
            let near_settle = &grow_shots[8];
            let settled_growth = &grow_shots[10];
            // Materialization: the popup's items land in semantics once it
            // is composed — poll that instead of a fixed wall-clock wait so
            // host throttling cannot race the capture. Pixel diffs stay for
            // the shape checks, but the menu card is white-on-white against
            // the page, so diff area measures content/shadow, not presence.
            let mut menu_seen = false;
            for _ in 0..40 {
                if find_text_in_semantics(&robot, "Grid").is_some() {
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
            let early_area = diff_area(&clean, absorbed, region);
            let open_area = diff_area(&clean, &open, region);
            let early_extent = diff_extent(&clean, absorbed, region)
                .unwrap_or_else(|| fail(&robot, "early menu surface had no measurable extent"));
            let oval_extent = diff_extent(&clean, wide_oval, region)
                .unwrap_or_else(|| fail(&robot, "wide menu oval had no measurable extent"));
            let mid_extent = diff_extent(&clean, mid_growth, region)
                .unwrap_or_else(|| fail(&robot, "mid-growth menu had no measurable extent"));
            let near_settle_extent = diff_extent(&clean, near_settle, region)
                .unwrap_or_else(|| fail(&robot, "near-settle menu had no measurable extent"));
            let settle_extent = diff_extent(&clean, settled_growth, region)
                .unwrap_or_else(|| fail(&robot, "settled menu had no measurable extent"));
            let open_extent = diff_extent(&clean, &open, region)
                .unwrap_or_else(|| fail(&robot, "settled menu surface had no measurable extent"));
            let early_width_fraction = early_extent.0 / open_extent.0;
            let early_height_fraction = early_extent.1 / open_extent.1;
            let oval_width_fraction = oval_extent.0 / open_extent.0;
            let oval_height_fraction = oval_extent.1 / open_extent.1;
            let oval_center_y = glass_extent_center_y(&clean, wide_oval, region)
                .unwrap_or_else(|| fail(&robot, "wide menu oval had no measurable center"));
            let settle_center_y = glass_extent_center_y(&clean, settled_growth, region)
                .unwrap_or_else(|| fail(&robot, "settled menu had no measurable center"));
            let mid_width_fraction = mid_extent.0 / open_extent.0;
            let mid_height_fraction = mid_extent.1 / open_extent.1;
            let near_settle_width_fraction = near_settle_extent.0 / open_extent.0;
            let near_settle_height_fraction = near_settle_extent.1 / open_extent.1;
            let settle_width_fraction = settle_extent.0 / open_extent.0;
            let settle_height_fraction = settle_extent.1 / open_extent.1;
            println!(
                "menu-grow early_area={early_area} open_area={open_area} early_extent={early_extent:?} oval_extent={oval_extent:?} mid_extent={mid_extent:?} near_settle_extent={near_settle_extent:?} settle_extent={settle_extent:?} open_extent={open_extent:?} early_fractions=({early_width_fraction:.3}, {early_height_fraction:.3}) oval_fractions=({oval_width_fraction:.3}, {oval_height_fraction:.3}) mid_fractions=({mid_width_fraction:.3}, {mid_height_fraction:.3}) near_settle_fractions=({near_settle_width_fraction:.3}, {near_settle_height_fraction:.3}) settle_fractions=({settle_width_fraction:.3}, {settle_height_fraction:.3}) centers=({oval_center_y:.1}, {settle_center_y:.1})"
            );
            if open_area < 600 {
                save(&open, &shot_dir, "menu-open");
                fail(&robot, "open menu drew almost no pixels over the clean page");
            }
            let early_aspect = early_extent.0 / early_extent.1.max(1.0);
            // Ratios re-pinned for the 40dp card-header anchor (was the
            // 54dp nav circle): height still arrives well before width.
            if !(0.30..=0.48).contains(&early_width_fraction)
                || !(0.68..=0.95).contains(&early_height_fraction)
                || !(1.02..=1.60).contains(&early_aspect)
            {
                save(absorbed, &shot_dir, "menu-early");
                save(&open, &shot_dir, "menu-open");
                fail(
                    &robot,
                    &format!(
                        "menu missed the departing-source phase at 75ms: early extent {early_extent:?}, settled extent {open_extent:?}"
                    ),
                );
            }
            // This diff bbox includes the departed source position once the
            // body follows its measured downward rebound. Exact live contour
            // dimensions are pinned in the menu geometry unit test; here we
            // verify that the shader rendered the expected width progression
            // without mistaking travel distance for contour height. The 30dp
            // frost and luma-compressed material intentionally make the outer
            // white-on-white shoulder fall below this pixel-diff threshold,
            // so retain a material-aware height floor and require visible
            // growth from the preceding source frame instead of pinning the
            // old 12dp-frost footprint.
            if !(0.36..=0.65).contains(&oval_width_fraction)
                || !(0.72..=1.12).contains(&oval_height_fraction)
                || oval_extent.1 < early_extent.1
            {
                save(wide_oval, &shot_dir, "menu-wide-oval");
                fail(
                    &robot,
                    &format!(
                        "menu missed the rendered oval trajectory at 104ms: extent {oval_extent:?}, settled {open_extent:?}"
                    ),
                );
            }
            // Exact width is tested from the live SDF geometry. This pixel
            // bbox runs over a nearly white region, so the low-contrast outer
            // shoulder can fall below the diff threshold even while the
            // rendered contour is at the measured 205..216dp footprint.
            if !(0.74..=0.94).contains(&mid_width_fraction) {
                save(mid_growth, &shot_dir, "menu-mid-growth");
                fail(
                    &robot,
                    &format!(
                        "menu was not front-loaded by 181ms: extent {mid_extent:?}, settled {open_extent:?}"
                    ),
                );
            }
            if near_settle_width_fraction < 0.90 || near_settle_height_fraction < 0.90 {
                save(near_settle, &shot_dir, "menu-near-settle");
                fail(
                    &robot,
                    &format!(
                        "menu did not approach its card footprint by 286ms: extent {near_settle_extent:?}, settled {open_extent:?}"
                    ),
                );
            }
            if settle_width_fraction < 0.88 || settle_height_fraction < 0.95 {
                save(settled_growth, &shot_dir, "menu-settle");
                fail(
                    &robot,
                    &format!(
                        "menu did not settle by 459ms: extent {settle_extent:?}, settled {open_extent:?}"
                    ),
                );
            }
            // Close: deflates back into the anchor, then zero residue. The
            // ~200ms deflate MUST be sampled on the exact animation clock —
            // a wall-clock sleep stretches under host load and the first
            // pumped frame then advances the spring past the whole morph
            // (the capture lands on an already-closed menu and flakes).
            robot.click(200.0, 500.0).expect("dismiss menu");
            let close_shots = robot
                .capture_keyframes(
                    1.0,
                    &[
                        (0.0, false),
                        (1.0, false),
                        (15.0, true),
                        (17.0, true),
                        (17.0, true),
                        (33.0, true),
                        (33.0, true),
                        (50.0, true),
                        (50.0, true),
                        (234.0, true),
                    ],
                )
                .expect("close keyframes");
            // Let wall time catch back up with the advanced animation clock.
            settle(&robot, 900);
            let closing = &close_shots[2];
            let closed = &close_shots[7];
            for (shot, label) in close_shots
                .iter()
                .zip([
                    "016ms", "033ms", "050ms", "083ms", "116ms", "166ms", "216ms", "450ms",
                ])
            {
                save(shot, &shot_dir, &format!("menu-closing-{label}"));
            }
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
            if closing_area * 100 < open_area * 55 {
                save(closing, &shot_dir, "menu-closing");
                fail(
                    &robot,
                    &format!(
                        "menu collapsed too far 50ms after dismiss: area {closing_area} vs open {open_area}"
                    ),
                );
            }
            if closing_area * 100 > open_area * 160 {
                save(closing, &shot_dir, "menu-closing-overshoot");
                fail(
                    &robot,
                    &format!(
                        "menu close leaf expanded excessively: area {closing_area} vs open {open_area}"
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

            // The actual Liquid UI menu must also use one continuous mouse
            // gesture: hold the anchor, slide the still-owned pointer onto a
            // row, and release to fire it. No second press is allowed.
            robot
                .mouse_move(menu_anchor.0, menu_anchor.1)
                .expect("hover menu trigger");
            robot.mouse_down().expect("hold menu trigger");
            std::thread::sleep(Duration::from_millis(700));
            let grid = find_text_in_semantics(&robot, "Grid")
                .unwrap_or_else(|| fail(&robot, "menu did not open during the held mouse press"));
            let grid_center = (grid.0 + grid.2 * 0.5, grid.1 + grid.3 * 0.5);
            robot
                .mouse_move(grid_center.0, grid_center.1)
                .expect("slide held pointer onto menu row");
            std::thread::sleep(Duration::from_millis(80));
            let menu_hover = robot.screenshot().expect("menu held-hover frame");
            save(&menu_hover, &shot_dir, "menu-single-gesture-hover");
            let black_damage = count_new_black(&clean, &menu_hover, (20, 200, 600, 740));
            if black_damage > 100 {
                fail(
                    &robot,
                    &format!(
                        "opening the menu painted {black_damage} black pixels outside its bounds"
                    ),
                );
            }
            robot.mouse_up().expect("release held pointer on menu row");
            settle(&robot, 700);
            if find_text_in_semantics(&robot, "Grid").is_some() {
                fail(
                    &robot,
                    "held mouse release over a menu row did not fire and dismiss it",
                );
            }
            robot
                .click(menu_anchor.0, menu_anchor.1)
                .expect("reopen fired menu state");
            settle(&robot, 600);
            let list_after = find_text_in_semantics(&robot, "List")
                .unwrap_or_else(|| fail(&robot, "List row missing after gesture fired"));
            let grid_after = find_text_in_semantics(&robot, "Grid")
                .unwrap_or_else(|| fail(&robot, "Grid row missing after gesture fired"));
            let fired = robot.screenshot().expect("menu fired-state frame");
            save(&fired, &shot_dir, "menu-single-gesture-fired");
            let check_region = |row: (f32, f32, f32, f32)| {
                (
                    (row.0 + 10.0) as usize,
                    row.1 as usize,
                    (row.0 + 36.0) as usize,
                    (row.1 + row.3) as usize,
                )
            };
            let list_check_ink = count_dark(&fired, check_region(list_after));
            let grid_check_ink = count_dark(&fired, check_region(grid_after));
            if grid_check_ink <= list_check_ink + 2 {
                fail(
                    &robot,
                    &format!(
                        "single-gesture lift dismissed without firing Grid: check ink list={list_check_ink}, grid={grid_check_ink}"
                    ),
                );
            }
            robot.click(400.0, 500.0).expect("dismiss fired-state menu");
            settle(&robot, 300);

            // ---------- Toggle: press = lens (grow + dissolve thumb) ----------
            scroll_button_to_y(&robot, "Wi-Fi settings switch", 300.0);
            settle(&robot, 900);
            let Some((track_left, track_top, track_width, track_height)) =
                find_button_exact(&robot, "Wi-Fi settings switch")
            else {
                fail(&robot, "Wi-Fi switch not found in semantics");
            };
            if (track_width - TOGGLE_TRACK_WIDTH).abs() > 1.0
                || (track_height - TOGGLE_TRACK_HEIGHT).abs() > 1.0
            {
                fail(
                    &robot,
                    &format!(
                        "Wi-Fi switch semantics changed size: {track_width:.1}x{track_height:.1}"
                    ),
                );
            }
            let track_right = track_left + track_width;
            let toggle_y = track_top + track_height * 0.5;
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
            let green_endpoint = rightmost_matching_x(
                &pressed,
                track_left,
                track_right + 12.0,
                toggle_y,
                |r, g, b| r < 100 && g > 150 && b < 130 && g > r.saturating_add(80),
            )
            .unwrap_or_else(|| fail(&robot, "pressed toggle lost its active track sample"));
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
            let endpoint_inset = track_right - green_endpoint;
            println!(
                "toggle pressed_white={pressed_white} overflow={overflow} green_endpoint={green_endpoint:.1} inset={endpoint_inset:.1}"
            );
            if pressed_white * 3 > rest_white {
                save(&pressed, &shot_dir, "toggle-pressed");
                fail(
                    &robot,
                    &format!(
                        "pressed toggle keeps its opaque white thumb ({pressed_white} white px) — the lens must dissolve it"
                    ),
                );
            }
            if overflow < 20 {
                save(&pressed, &shot_dir, "toggle-pressed");
                fail(
                    &robot,
                    "pressed toggle lens does not overflow the track (no growth visible)",
                );
            }
            if !(-1.0..=5.0).contains(&endpoint_inset) {
                save(&pressed, &shot_dir, "toggle-shifted-track");
                fail(
                    &robot,
                    &format!(
                        "pressed toggle shifts the fixed track endpoint: endpoint={green_endpoint:.1}, raw={track_right:.1}, inset={endpoint_inset:.1}"
                    ),
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

            // ---------- Toggle: lens follows the finger every frame --------
            // Reference: the glass bubble rides the drag continuously (a
            // frozen/stuck bubble was the density-vs-render-scale packing
            // bug). Drag OFF -> ON in steps and check that its upper edge
            // tracks the pointer monotonically.
            let rest_off = robot.screenshot().expect("toggle drag rest");
            // Only the lens protrudes above the 28dp track. Measuring this
            // band excludes the accumulated active-track recolor, whose
            // centroid naturally saturates as the fill approaches the end.
            let lens_edge_region = (
                (track_left - 30.0) as usize,
                (toggle_y - 25.0) as usize,
                (track_right + 30.0) as usize,
                (toggle_y - TOGGLE_TRACK_HEIGHT * 0.3) as usize,
            );
            robot
                .mouse_move(track_left + 12.0, toggle_y)
                .expect("hover off thumb");
            robot.mouse_down().expect("press off thumb");
            std::thread::sleep(Duration::from_millis(260));
            robot
                .mouse_move(track_left + 13.0, toggle_y)
                .expect("prime toggle motion sample");
            std::thread::sleep(Duration::from_millis(16));
            let direct_pointer_x = track_left + 21.0;
            robot
                .mouse_move(direct_pointer_x, toggle_y)
                .expect("toggle direct pointer step");
            let direct_toggle = robot.screenshot().expect("toggle immediate pointer frame");
            save(&direct_toggle, &shot_dir, "toggle-direct-follow");
            save_with_pointer(
                &direct_toggle,
                &shot_dir,
                "toggle-direct-follow-proof",
                direct_pointer_x,
                toggle_y,
            );
            let direct_edge = diff_right_edge_x(&rest_off, &direct_toggle, lens_edge_region)
                .unwrap_or_else(|| fail(&robot, "toggle direct step produced no lens edge"));
            let direct_center = direct_edge - TOGGLE_LENS_WIDTH * 0.5;
            println!(
                "toggle direct pointer={direct_pointer_x:.1} lens_center={direct_center:.1}"
            );
            // The thumb is grabbed WHERE the finger lands (grab offset
            // preserved), so the lens tracks pointer DELTAS 1:1 rather than
            // centering under the pointer; the grab in this phase lands
            // within the thumb, so the residual offset stays under a thumb
            // half-width.
            if (direct_center - direct_pointer_x).abs() > 22.0 {
                fail(
                    &robot,
                    &format!(
                        "toggle lens did not track the pointer in the input frame: pointer={direct_pointer_x:.1}, lens={direct_center:.1}"
                    ),
                );
            }
            let mut right_edges = Vec::new();
            let mut transition_track = [0u8; 3];
            let mut mid_track_samples = None;
            let mut prism_pixels = 0usize;
            for step in 0..3 {
                let x = track_left + 12.0 + (step as f32 + 1.0) * 10.0;
                robot.mouse_move(x, toggle_y).expect("drag step");
                let shot = robot.screenshot().expect("drag shot");
                // Judge frames: OFF-side press dragged toward ON. The capsule
                // stays fixed while its complete backdrop transitions through
                // gray and sage into the active green.
                save(&shot, &shot_dir, &format!("toggle-drag-{step}"));
                save_with_pointer(
                    &shot,
                    &shot_dir,
                    &format!("toggle-drag-{step}-proof"),
                    x,
                    toggle_y,
                );
                if step == 1 {
                    mid_track_samples = Some((
                        sample_rgb(&shot, track_left + 5.0, toggle_y),
                        sample_rgb(&shot, track_right - 15.0, toggle_y),
                    ));
                    let material_shot = robot
                        .screenshot_with_scale(4.0)
                        .expect("high-resolution toggle material proof");
                    save(&material_shot, &shot_dir, "toggle-drag-1-4x");
                }
                if step == 2 {
                    transition_track = sample_rgb(&shot, track_left + 5.0, toggle_y);
                    prism_pixels = count_prismatic(
                        &shot,
                        (
                            (track_left + 20.0) as usize,
                            (toggle_y - 22.0) as usize,
                            (track_right + 25.0) as usize,
                            (toggle_y - 14.0) as usize,
                        ),
                    ) + count_prismatic(
                        &shot,
                        (
                            (track_left + 20.0) as usize,
                            (toggle_y + 14.0) as usize,
                            (track_right + 25.0) as usize,
                            (toggle_y + 22.0) as usize,
                        ),
                    );
                }
                let Some(edge_x) = diff_right_edge_x(&rest_off, &shot, lens_edge_region) else {
                    save(&shot, &shot_dir, "toggle-drag-blank");
                    fail(&robot, "toggle drag produced no visible lens change");
                };
                right_edges.push(edge_x);
                std::thread::sleep(Duration::from_millis(16));
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
            let restored_on = robot.screenshot().expect("toggle restored ON");
            let restored_on_white = count_white(&restored_on, thumb_probe);
            let restored_track = sample_rgb(&restored_on, track_left + 5.0, toggle_y);
            println!("toggle drag right_edges={right_edges:?}");
            println!("toggle transition track rgb={transition_track:?}");
            println!("toggle mid-track samples={mid_track_samples:?}");
            println!("toggle prism pixels={prism_pixels}");
            println!("toggle restored_on_white={restored_on_white}");
            println!("toggle restored track rgb={restored_track:?}");
            // The 58dp lens's right edge saturates near the 63dp track end
            // almost immediately, so edge GROWTH is not the follow signal
            // (the saved proof frames show the ride + fill progressing);
            // regression is: the edge retreating while the finger advances.
            if right_edges[1] < right_edges[0] - 2.0 || right_edges[2] < right_edges[1] - 2.0 {
                fail(
                    &robot,
                    &format!(
                        "toggle lens retreated against the finger (right edges {right_edges:?})"
                    ),
                );
            }
            if transition_track[0] < 75
                || transition_track[0] > 175
                || transition_track[1] < 170
                || transition_track[1] > 210
                || transition_track[2] < 90
                || transition_track[2] > 175
                || transition_track[1] <= transition_track[0].saturating_add(20)
                || transition_track[1] <= transition_track[2].saturating_add(20)
            {
                fail(
                    &robot,
                    &format!(
                        "toggle skips the target's whole-track sage interpolation ({transition_track:?})"
                    ),
                );
            }
            let (mid_left, mid_right) = mid_track_samples.expect("middle drag sample");
            let mid_spread = mid_left[0]
                .abs_diff(mid_right[0])
                .max(mid_left[1].abs_diff(mid_right[1]))
                .max(mid_left[2].abs_diff(mid_right[2]));
            if mid_spread > 42 {
                fail(
                    &robot,
                    &format!(
                        "toggle paints a traveling fill instead of interpolating the complete fixed track: left={mid_left:?}, right={mid_right:?}, spread={mid_spread}"
                    ),
                );
            }
            // The target spectrum is split into disconnected caustic lobes;
            // a high whole-perimeter pixel quota rewards the rejected ring.
            if prism_pixels < 80 {
                fail(
                    &robot,
                    &format!("toggle meniscus has too little chromatic rim energy ({prism_pixels})"),
                );
            }
            if restored_on_white < 40 {
                save(&restored_on, &shot_dir, "toggle-failed-to-toggle-back");
                fail(
                    &robot,
                    "OFF-to-ON drag did not commit; the toggle cannot be toggled back",
                );
            }
            if restored_track[0] >= 75
                || restored_track[1] <= 170
                || restored_track[2] >= 105
            {
                fail(
                    &robot,
                    &format!(
                        "toggle track did not finish its full-field transition after settle ({restored_track:?})"
                    ),
                );
            }

            // ---------- Segmented control: swipe + glass lens ----------
            // Keep the control clear of the floating tab bar. Measuring a
            // lens through a second refractive surface biases both axes and
            // cannot prove pointer coordination.
            scroll(&robot, 450.0, 500.0, -220.0);
            settle(&robot, 500);
            let Some((seg_x, seg_y, seg_w, seg_h)) = find_text_in_semantics(&robot, "Sending")
            else {
                fail(&robot, "'Sending' segment not found in semantics");
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
            save(&seg_rest, &shot_dir, "segmented-rest");
            // Press on the selected "Receiving" third and drag to the "Errored" third.
            robot
                .mouse_move(control_left + seg_w * 0.5, seg_cy)
                .expect("hover segmented");
            robot.mouse_down().expect("press segmented");
            std::thread::sleep(Duration::from_millis(260));
            let seg_down = robot.screenshot().expect("segmented down");
            save(&seg_down, &shot_dir, "segmented-down");
            let down_diff = region_diff(&seg_rest, &seg_down, seg_region);
            let seg_pointer_x = control_left + seg_w * 2.5;
            robot
                .mouse_move(seg_pointer_x, seg_cy)
                .expect("segmented direct jump");
            let seg_mid = robot.screenshot().expect("segmented immediate jump frame");
            save(&seg_mid, &shot_dir, "segmented-direct-follow");
            // Measure only where the destination lens belongs: source-pill
            // disappearance is outside this region and cannot fake tracking.
            let seg_right_region = (
                (control_left + seg_w * 1.45) as usize,
                seg_region.1,
                seg_region.2,
                seg_region.3,
            );
            let mid_cx = diff_centroid_x(&seg_rest, &seg_mid, seg_right_region);
            let mid_cy = glass_extent_center_y(&seg_rest, &seg_mid, seg_right_region);
            robot.mouse_up().expect("release segmented");
            let segmented_release = robot
                .capture_keyframes(
                    1.0,
                    &[
                        (0.0, false),
                        (1.0, false),
                        (50.0, true),
                        (100.0, true),
                        (200.0, true),
                        (400.0, true),
                    ],
                )
                .expect("segmented release keyframes");
            for (shot, label) in segmented_release
                .iter()
                .zip(["051ms", "151ms", "351ms", "751ms"].iter())
            {
                save(shot, &shot_dir, &format!("segmented-release-{label}"));
            }
            settle(&robot, 500);
            let segmented_settled = robot.screenshot().expect("segmented settled frame");
            save(&segmented_settled, &shot_dir, "segmented-settled");
            println!(
                "segmented down_diff={down_diff} pointer=({seg_pointer_x:.2},{seg_cy:.2}) frame=({mid_cx:?},{mid_cy:?})"
            );
            if down_diff < 200 {
                save(&seg_down, &shot_dir, "segmented-down");
                fail(
                    &robot,
                    &format!("segmented press shows no lens (diff {down_diff})"),
                );
            }
            if mid_cx.is_none_or(|x| (x - seg_pointer_x).abs() > seg_w * 0.30) {
                fail(
                    &robot,
                    "segmented lens animated toward the pointer instead of following the input frame",
                );
            }
            if mid_cy.is_none_or(|y| (y - seg_cy).abs() > 4.0) {
                fail(
                    &robot,
                    "segmented lens is vertically offset from the pointer/control center",
                );
            }
            // The drag committed a new segment: restore "Receiving" for idempotence.
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
            save(&slider_rest, &shot_dir, "slider-rest");
            // The demo slider starts at 0.55: the thumb is near mid-track.
            let thumb_guess = 56.0 + 0.55 * (844.0 - 56.0);
            robot
                .mouse_move(thumb_guess, slider_y)
                .expect("hover slider thumb");
            robot.mouse_down().expect("press slider");
            std::thread::sleep(Duration::from_millis(260));
            let slider_down = robot.screenshot().expect("slider down");
            save(&slider_down, &shot_dir, "slider-down");
            let slider_diff = region_diff(&slider_rest, &slider_down, slider_region);
            let mut slider_mid = None;
            for step in 1..=4 {
                let slider_pointer_x = thumb_guess + step as f32 * 22.0;
                robot
                    .mouse_move(slider_pointer_x, slider_y)
                    .expect("slider drag");
                if step == 4 {
                    let shot = robot.screenshot().expect("slider immediate drag frame");
                    save(&shot, &shot_dir, "slider-drag");
                    save_with_pointer(
                        &shot,
                        &shot_dir,
                        "slider-direct-follow-proof",
                        slider_pointer_x,
                        slider_y,
                    );
                    slider_mid = Some(shot);
                }
                std::thread::sleep(Duration::from_millis(16));
            }
            let slider_mid = slider_mid.expect("slider drag frame");
            // Only right of the press point: the old thumb spot also changed.
            let slider_right_region = (
                (thumb_guess + 26.0) as usize,
                slider_region.1,
                slider_region.2,
                slider_region.3,
            );
            let slider_cx = diff_centroid_x(&slider_rest, &slider_mid, slider_right_region);
            robot.mouse_up().expect("release slider");
            let slider_release = robot
                .capture_keyframes(
                    1.0,
                    &[
                        (0.0, false),
                        (1.0, false),
                        (50.0, true),
                        (100.0, true),
                        (200.0, true),
                        (400.0, true),
                    ],
                )
                .expect("slider release keyframes");
            for (shot, label) in slider_release
                .iter()
                .zip(["051ms", "151ms", "351ms", "751ms"].iter())
            {
                save(shot, &shot_dir, &format!("slider-release-{label}"));
            }
            settle(&robot, 500);
            let slider_settled = robot.screenshot().expect("slider settled frame");
            save(&slider_settled, &shot_dir, "slider-settled");
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
            scroll_button_to_y(&robot, "Glass", 300.0);
            settle(&robot, 500);
            let Some((bx, by, bw, bh)) = find_button_exact(&robot, "Glass") else {
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
            save(&rest, &shot_dir, "button-rest");
            let rest_ink = count_dark(&rest, button_region);
            robot
                .mouse_move(bx + bw * 0.5, by + bh * 0.5)
                .expect("hover button");
            robot.mouse_down().expect("press button");
            std::thread::sleep(Duration::from_millis(300));
            let pressed = robot.screenshot().expect("button pressed");
            save(&pressed, &shot_dir, "button-pressed");
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
            let nav_drag_x = WINDOW_WIDTH as f32 - 50.0;
            let nav_drag_y = WINDOW_HEIGHT as f32 * 0.31;
            // Scroll until the page tops out (the demo page keeps growing;
            // a fixed delta goes stale).
            for _ in 0..4 {
                scroll(&robot, nav_drag_x, nav_drag_y, 2400.0);
                settle(&robot, 300);
            }
            settle(&robot, 900);
            let Some((_, card_y_top, _, _)) = find_text_in_semantics(&robot, "iPadOS") else {
                fail(&robot, "'iPadOS' card not found in semantics");
            };
            let inline_region = (380usize, 102usize, 520usize, 142usize);
            let expanded = robot.screenshot().expect("nav expanded");
            save(&expanded, &shot_dir, "nav-expanded");
            let expanded_inline = count_dark(&expanded, inline_region);
            println!("nav expanded card_y={card_y_top} inline_ink={expanded_inline}");
            if expanded_inline > 120 {
                save(&expanded, &shot_dir, "nav-expanded");
                fail(&robot, "expanded nav must not show the inline title");
            }

            // Drag up into the band's upper half, rest, release: must snap
            // COLLAPSED (and the rest before lift must not phantom-fling).
            robot
                .mouse_move(nav_drag_x, nav_drag_y)
                .expect("nav drag hover");
            robot.mouse_down().expect("nav drag press");
            for step in 1..=6 {
                robot
                    .mouse_move(nav_drag_x, nav_drag_y - step as f32 * 7.0)
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
            save(&snapped, &shot_dir, "nav-collapsed");
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
            robot
                .mouse_move(nav_drag_x, nav_drag_y)
                .expect("nav return hover");
            robot.mouse_down().expect("nav return press");
            for step in 1..=5 {
                robot
                    .mouse_move(nav_drag_x, nav_drag_y + step as f32 * 7.0)
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
            save(&expanded_again, &shot_dir, "nav-reexpanded");
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

fn sample_rgb(shot: &cranpose::RobotScreenshot, x: f32, y: f32) -> [u8; 3] {
    let (sx, sy) = scale(shot);
    let x = (x * sx) as usize;
    let y = (y * sy) as usize;
    let index = (y * shot.width as usize + x) * 4;
    [
        shot.pixels[index],
        shot.pixels[index + 1],
        shot.pixels[index + 2],
    ]
}

fn diff_area(
    a: &cranpose::RobotScreenshot,
    b: &cranpose::RobotScreenshot,
    region: (usize, usize, usize, usize),
) -> usize {
    region_diff(a, b, region)
}

/// Width and height, in logical pixels, of the noticeably changed silhouette.
fn diff_extent(
    a: &cranpose::RobotScreenshot,
    b: &cranpose::RobotScreenshot,
    region: (usize, usize, usize, usize),
) -> Option<(f32, f32)> {
    let (sx, sy) = scale(a);
    let (x0, y0, x1, y1) = region;
    let (x0, y0, x1, y1) = (
        (x0 as f32 * sx) as usize,
        (y0 as f32 * sy) as usize,
        ((x1 as f32 * sx) as usize).min(a.width as usize),
        ((y1 as f32 * sy) as usize).min(a.height as usize),
    );
    let mut bounds: Option<(usize, usize, usize, usize)> = None;
    for y in (y0..y1).step_by(2) {
        for x in (x0..x1).step_by(2) {
            let i = (y * a.width as usize + x) * 4;
            let d = a.pixels[i]
                .abs_diff(b.pixels[i])
                .max(a.pixels[i + 1].abs_diff(b.pixels[i + 1]))
                .max(a.pixels[i + 2].abs_diff(b.pixels[i + 2]));
            if d > 12 {
                bounds = Some(match bounds {
                    Some((min_x, min_y, max_x, max_y)) => {
                        (min_x.min(x), min_y.min(y), max_x.max(x), max_y.max(y))
                    }
                    None => (x, y, x, y),
                });
            }
        }
    }
    bounds.map(|(min_x, min_y, max_x, max_y)| {
        (
            (max_x.saturating_sub(min_x) + 2) as f32 / sx,
            (max_y.saturating_sub(min_y) + 2) as f32 / sy,
        )
    })
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

/// Center of the complete changed silhouette. Weighting only bright or
/// chromatic pixels biases the result toward whichever side of the signed
/// optical profile happens to carry the current backdrop color.
fn glass_extent_center_y(
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
    let mut min_y = usize::MAX;
    let mut max_y = 0usize;
    let mut count = 0usize;
    for y in py0..py1 {
        for x in px0..px1 {
            let i = (y * a.width as usize + x) * 4;
            let d = a.pixels[i]
                .abs_diff(b.pixels[i])
                .max(a.pixels[i + 1].abs_diff(b.pixels[i + 1]))
                .max(a.pixels[i + 2].abs_diff(b.pixels[i + 2]));
            if d > 12 {
                min_y = min_y.min(y);
                max_y = max_y.max(y);
                count += 1;
            }
        }
    }
    (count >= 20).then(|| (min_y + max_y) as f32 * 0.5 / sy)
}

/// Rightmost changed pixel in a logical region. Used on a band outside the
/// toggle track, where every changed pixel belongs to the glass lens itself.
fn diff_right_edge_x(
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
    let mut right_edge = None;
    let mut count = 0usize;
    for y in py0..py1 {
        for x in px0..px1 {
            let i = (y * a.width as usize + x) * 4;
            let d = a.pixels[i]
                .abs_diff(b.pixels[i])
                .max(a.pixels[i + 1].abs_diff(b.pixels[i + 1]))
                .max(a.pixels[i + 2].abs_diff(b.pixels[i + 2]));
            if d > 12 {
                right_edge = Some(right_edge.map_or(x, |current: usize| current.max(x)));
                count += 1;
            }
        }
    }
    (count >= 12).then(|| right_edge.expect("changed pixel exists") as f32 / sx)
}

fn rightmost_matching_x(
    shot: &cranpose::RobotScreenshot,
    x0: f32,
    x1: f32,
    y: f32,
    predicate: impl Fn(u8, u8, u8) -> bool,
) -> Option<f32> {
    let (sx, sy) = scale(shot);
    let px0 = (x0 * sx).max(0.0) as usize;
    let px1 = (x1 * sx).min(shot.width as f32) as usize;
    let center_y = (y * sy).clamp(1.0, shot.height as f32 - 2.0) as usize;
    let mut rightmost = None;
    for py in center_y - 1..=center_y + 1 {
        for px in px0..px1 {
            let i = (py * shot.width as usize + px) * 4;
            if predicate(shot.pixels[i], shot.pixels[i + 1], shot.pixels[i + 2]) {
                rightmost = Some(rightmost.map_or(px, |current: usize| current.max(px)));
            }
        }
    }
    rightmost.map(|px| px as f32 / sx)
}

fn count_white(shot: &cranpose::RobotScreenshot, region: (usize, usize, usize, usize)) -> usize {
    count_matching(shot, region, |r, g, b| r > 243 && g > 243 && b > 243)
}

fn count_dark(shot: &cranpose::RobotScreenshot, region: (usize, usize, usize, usize)) -> usize {
    count_matching(shot, region, |r, g, b| {
        (r as u16 + g as u16 + b as u16) < 420
    })
}

fn count_new_black(
    before: &cranpose::RobotScreenshot,
    after: &cranpose::RobotScreenshot,
    region: (usize, usize, usize, usize),
) -> usize {
    let (sx, sy) = scale(before);
    let (x0, y0, x1, y1) = region;
    let (x0, y0, x1, y1) = (
        (x0 as f32 * sx) as usize,
        (y0 as f32 * sy) as usize,
        ((x1 as f32 * sx) as usize).min(before.width as usize),
        ((y1 as f32 * sy) as usize).min(before.height as usize),
    );
    let mut count = 0;
    for y in y0..y1 {
        for x in x0..x1 {
            let i = (y * before.width as usize + x) * 4;
            let before_luma =
                before.pixels[i] as u16 + before.pixels[i + 1] as u16 + before.pixels[i + 2] as u16;
            let after_luma =
                after.pixels[i] as u16 + after.pixels[i + 1] as u16 + after.pixels[i + 2] as u16;
            if before_luma > 540 && after_luma < 72 {
                count += 1;
            }
        }
    }
    count
}

fn count_prismatic(
    shot: &cranpose::RobotScreenshot,
    region: (usize, usize, usize, usize),
) -> usize {
    count_matching(shot, region, |r, g, b| {
        let min = r.min(g).min(b);
        let max = r.max(g).max(b);
        max - min > 15
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

fn scroll_button_to_y(robot: &cranpose::Robot, text: &str, target_y: f32) {
    scroll_bounds_to_y(robot, text, target_y, |robot| {
        find_button_exact(robot, text)
    });
}

fn find_button_exact(robot: &cranpose::Robot, text: &str) -> Option<(f32, f32, f32, f32)> {
    robot
        .find_button_bounds_exact(text)
        .unwrap_or_else(|error| fail(robot, &format!("failed to query {text} button: {error}")))
}

fn scroll_bounds_to_y(
    robot: &cranpose::Robot,
    description: &str,
    target_y: f32,
    find_bounds: impl Fn(&cranpose::Robot) -> Option<(f32, f32, f32, f32)>,
) {
    const ALIGNMENT_TOLERANCE: f32 = 24.0;
    for _ in 0..8 {
        let rect = find_bounds(robot).unwrap_or_else(|| {
            fail(
                robot,
                &format!("{description} semantics missing while scrolling"),
            )
        });
        let delta = rect.1 - target_y;
        if delta.abs() <= ALIGNMENT_TOLERANCE {
            return;
        }
        scroll(robot, 450.0, 500.0, -delta.clamp(-700.0, 700.0));
        settle(robot, 240);
    }

    let rect = find_bounds(robot).unwrap_or_else(|| {
        fail(
            robot,
            &format!("{description} semantics missing after scrolling"),
        )
    });
    if (rect.1 - target_y).abs() > ALIGNMENT_TOLERANCE {
        fail(
            robot,
            &format!("failed to scroll {description} to y={target_y}: {rect:?}"),
        );
    }
}

fn save(shot: &cranpose::RobotScreenshot, dir: &Path, name: &str) {
    let image = RgbaImage::from_raw(shot.width, shot.height, shot.pixels.clone())
        .expect("screenshot buffer");
    let path = dir.join(format!("{name}.png"));
    image.save(&path).expect("save screenshot");
    println!("saved {}", path.display());
}

fn save_with_pointer(
    shot: &cranpose::RobotScreenshot,
    dir: &Path,
    name: &str,
    pointer_x: f32,
    pointer_y: f32,
) {
    let mut image = RgbaImage::from_raw(shot.width, shot.height, shot.pixels.clone())
        .expect("screenshot buffer");
    let center_x = pointer_x.round() as i32;
    let center_y = pointer_y.round() as i32;
    let red = image::Rgba([224, 32, 32, 255]);
    for offset in -7..=7 {
        for (x, y) in [(center_x + offset, center_y), (center_x, center_y + offset)] {
            if x >= 0 && y >= 0 && x < shot.width as i32 && y < shot.height as i32 {
                image.put_pixel(x as u32, y as u32, red);
            }
        }
    }
    let path = dir.join(format!("{name}.png"));
    image.save(&path).expect("save pointer proof");
    println!("saved {}", path.display());
}
