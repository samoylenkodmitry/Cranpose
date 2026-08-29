//! A held tab-bar selection lens must keep transmitting its backdrop in a
//! dark-scheme app, recoloring only the glyph ink it rides over — not turn
//! into a fully opaque solid disc.
//!
//! `tab_flight_lens_material` (tab_bar.rs) recolors "dark ink" transmitted
//! through the traveling selection bubble toward the accent color, so the
//! glyph under the bubble reads in the accent while the bar surface beside
//! it keeps its honest look. The ink mask keyed on an ABSOLUTE luma cutoff
//! (content darker than ~0.3 counts as ink), calibrated against a light
//! reference bar where only glyphs are that dark. In a dark-scheme app the
//! surrounding backdrop is itself near-black everywhere behind the bar, not
//! only under the glyphs, so the absolute cutoff classifies almost the
//! whole lens as ink and recolors it solid — the reported "opaque disc with
//! zero backdrop transmission".
//!
//! This asserts the fix without depending on a rendered reference image: a
//! point inside the pressed lens but away from its glyph must NOT read as
//! the accent color once the fix defines "ink" relative to the theme's own
//! foreground (glyph) luma instead of an absolute cutoff, while the glyph
//! itself must still take the accent (the feature is fixed, not disabled).
//!
//! Run with:
//! ```bash
//! cargo run --package desktop-app --example robot_liquid_tab_flight_dark_scheme_ink_recolor --features desktop,robot-app
//! ```

use std::{
    path::{Path, PathBuf},
    process::ExitCode,
    sync::atomic::{AtomicBool, Ordering},
    time::Duration,
};

use cranpose::{
    liquid::prelude::*,
    rememberMutableStateOf,
    widgets::{Box as CBox, BoxSpec},
    AppLauncher, Color, Modifier, RobotScreenshot, Size,
};

const WINDOW_WIDTH: u32 = 400;
const WINDOW_HEIGHT: u32 = 200;
const TAB_COUNT: usize = 3;
/// Wide enough that a point well clear of the 32dp icon frame (see
/// `OFF_GLYPH_OFFSET`) is still safely inside the pressed lens, which
/// overhangs its cell by only a few dp.
const TAB_WIDTH: f32 = 90.0;
const BAR_HEIGHT: f32 = 64.0;
const BAR_WIDTH: f32 = TAB_WIDTH * TAB_COUNT as f32;
const BAR_LEFT: f32 = (WINDOW_WIDTH as f32 - BAR_WIDTH) * 0.5;
const BAR_TOP: f32 = 80.0;
/// Offset from a cell's center that clears the 32dp icon frame (half-width
/// 16dp) with margin, while staying inside the lens's rounded end (the cell
/// half-width is 45dp, and the lens overhangs that by only a few dp).
const OFF_GLYPH_OFFSET: f32 = 37.0;
const ICON_ROW_OFFSET_Y: f32 = -12.0;
const SETTLE_MS: u64 = 900;
/// A dark app background: the same near-black polarity CranOrbit reported
/// the defect against.
const APP_BACKGROUND: Color = Color(0.04, 0.04, 0.05, 1.0);
const ACCENT: Color = Color(0.0, 0.48, 1.0, 1.0);

static FAILED: AtomicBool = AtomicBool::new(false);

fn main() -> ExitCode {
    let _ = env_logger::try_init();
    let shot_dir = PathBuf::from(
        std::env::var("ROBOT_SHOT_DIR")
            .unwrap_or_else(|_| "target/liquid-tab-flight-dark-ink-recolor".to_string()),
    );
    std::fs::create_dir_all(&shot_dir).expect("create shot dir");

    AppLauncher::new()
        .with_title("Liquid Tab Flight Dark Scheme Ink Recolor")
        .with_size(WINDOW_WIDTH, WINDOW_HEIGHT)
        .with_fonts(desktop_app::fonts::DEMO_FONTS)
        .with_headless(std::env::var("CRANPOSE_HEADLESS").as_deref() != Ok("0"))
        .with_test_driver(move |robot| {
            const TEST_TIMEOUT_SECS: u64 = 180;
            std::thread::spawn(|| {
                std::thread::sleep(Duration::from_secs(TEST_TIMEOUT_SECS));
                println!("\n✗ Test timed out after {TEST_TIMEOUT_SECS} seconds");
                std::process::exit(1);
            });
            std::thread::sleep(Duration::from_millis(700));
            settle(&robot, SETTLE_MS);

            let rest = robot.screenshot().expect("rest shot");
            save(&rest, &shot_dir, "0-rest.png");
            let scale = rest.width as f32 / rest.logical_width;

            let press_index = 1usize;
            let cell_cx = BAR_LEFT + TAB_WIDTH * (press_index as f32 + 0.5);
            let cell_cy = BAR_TOP + BAR_HEIGHT * 0.5;
            let off_glyph = (cell_cx + OFF_GLYPH_OFFSET, cell_cy);
            let glyph = (cell_cx, cell_cy + ICON_ROW_OFFSET_Y);

            robot.touch_down(cell_cx, cell_cy).expect("press tab");
            settle(&robot, SETTLE_MS);
            let pressed = robot.screenshot().expect("pressed shot");
            save(&pressed, &shot_dir, "1-pressed.png");
            robot.touch_up(cell_cx, cell_cy).expect("release tab");

            let off_glyph_rest = sample(&rest, off_glyph, scale);
            let off_glyph_pressed = sample(&pressed, off_glyph, scale);
            let glyph_rest = sample(&rest, glyph, scale);
            let glyph_pressed = sample(&pressed, glyph, scale);

            let accent_rgb = (
                (ACCENT.r() * 255.0) as u8,
                (ACCENT.g() * 255.0) as u8,
                (ACCENT.b() * 255.0) as u8,
            );
            println!(
                "off-glyph point {off_glyph:?}: rest={off_glyph_rest:?} pressed={off_glyph_pressed:?} \
                 (accent={accent_rgb:?}, distance-to-accent={:.1})",
                distance(off_glyph_pressed, accent_rgb)
            );
            println!(
                "glyph point {glyph:?}: rest={glyph_rest:?} pressed={glyph_pressed:?} \
                 (distance-to-accent rest={:.1} pressed={:.1})",
                distance(glyph_rest, accent_rgb),
                distance(glyph_pressed, accent_rgb)
            );

            // The core assertion: a point inside the pressed lens but off
            // its glyph must not have been swallowed into the accent. Under
            // the absolute-cutoff bug this point sits deep in dark-ink
            // territory (the whole dark backdrop qualifies) and reads as
            // near-solid accent.
            const NOT_ACCENT_FLOOR: f32 = 60.0;
            let off_glyph_distance = distance(off_glyph_pressed, accent_rgb);
            if off_glyph_distance < NOT_ACCENT_FLOOR {
                fail(
                    &robot,
                    &format!(
                        "a point inside the pressed lens but off its glyph reads as the \
                         accent color (distance {off_glyph_distance:.1} < {NOT_ACCENT_FLOOR}): \
                         {off_glyph_pressed:?} against accent {accent_rgb:?}. The lens has gone \
                         opaque solid instead of transmitting its backdrop beside the glyph."
                    ),
                );
            }

            // Sanity: the fix must not have disabled ink_recolor outright —
            // the glyph itself should still move markedly toward the accent
            // once pressed, versus its honest resting color.
            const GLYPH_SHOULD_MOVE_FLOOR: f32 = 40.0;
            let glyph_movement = distance(glyph_rest, glyph_pressed);
            if glyph_movement < GLYPH_SHOULD_MOVE_FLOOR {
                fail(
                    &robot,
                    &format!(
                        "the glyph under the pressed lens barely changed (moved {glyph_movement:.1} \
                         < {GLYPH_SHOULD_MOVE_FLOOR}): rest={glyph_rest:?} pressed={glyph_pressed:?}. \
                         ink_recolor should still be recoloring the glyph it rides over."
                    ),
                );
            }

            println!(
                "PASS: the pressed lens keeps transmitting its dark-scheme backdrop beside the \
                 glyph it recolors"
            );
            robot.exit().expect("exit");
        })
        .try_run(move || {
            LiquidTheme(
                LiquidThemeSpec {
                    scheme: SchemeMode::Dark,
                    accent: ACCENT,
                    ..LiquidThemeSpec::default()
                },
                || {
                    CBox(
                        Modifier::empty()
                            .size(Size::new(WINDOW_WIDTH as f32, WINDOW_HEIGHT as f32))
                            .background(APP_BACKGROUND),
                        BoxSpec::default(),
                        move || {
                            let selected = rememberMutableStateOf(|| 0usize);
                            LiquidTabBar(
                                Modifier::empty()
                                    .absolute_offset(BAR_LEFT, BAR_TOP)
                                    .size(Size::new(BAR_WIDTH, BAR_HEIGHT)),
                                LiquidTabBarSpec::new(TAB_WIDTH),
                                selected.get(),
                                move |index| selected.set(index),
                                |scope| {
                                    for (icon, label) in TABS {
                                        scope.tab(icon, label);
                                    }
                                },
                            );
                        },
                    );
                },
            );
        })
        .expect("launch tab flight dark ink recolor runner");

    if FAILED.load(Ordering::Relaxed) {
        ExitCode::FAILURE
    } else {
        ExitCode::SUCCESS
    }
}

const TABS: [(&str, &str); TAB_COUNT] = [
    (cranpose::liquid::icons::STAR, "A"),
    (cranpose::liquid::icons::LIST_OUTLINE, "B"),
    (cranpose::liquid::icons::SEARCH, "C"),
];

fn sample(shot: &RobotScreenshot, (lx, ly): (f32, f32), scale: f32) -> (u8, u8, u8) {
    let x = (lx * scale).round().clamp(0.0, shot.width as f32 - 1.0) as u32;
    let y = (ly * scale).round().clamp(0.0, shot.height as f32 - 1.0) as u32;
    let idx = ((y * shot.width + x) * 4) as usize;
    (shot.pixels[idx], shot.pixels[idx + 1], shot.pixels[idx + 2])
}

fn distance(a: (u8, u8, u8), b: (u8, u8, u8)) -> f32 {
    let dr = a.0 as f32 - b.0 as f32;
    let dg = a.1 as f32 - b.1 as f32;
    let db = a.2 as f32 - b.2 as f32;
    (dr * dr + dg * dg + db * db).sqrt()
}

fn save(shot: &RobotScreenshot, directory: &Path, name: &str) {
    if let Some(image) = image::RgbaImage::from_raw(shot.width, shot.height, shot.pixels.clone()) {
        let _ = image.save(directory.join(name));
    }
}

fn settle(robot: &cranpose::Robot, millis: u64) {
    let _ = robot.wait_for_idle();
    std::thread::sleep(Duration::from_millis(millis));
    let _ = robot.wait_for_idle();
}

fn fail(robot: &cranpose::Robot, message: &str) -> ! {
    println!("\n✗ {message}");
    FAILED.store(true, Ordering::Relaxed);
    let _ = robot.exit();
    std::process::exit(1);
}
