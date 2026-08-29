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
const STEP_COUNT: usize = 20;
/// Interior of the "Library" glass `TopBar`. NOT derived analytically this
/// time — the first version of this constant (160, 20, 200, 30) was an
/// analytic guess from `glass_feed.rs`'s layout constants that turned out
/// wrong: it landed squarely in the outer, unrelated, always-static
/// tab-bar strip (`combined_app`'s "Shaders / Shader Rect / ..." row sits
/// above `TabContent`, so the real chrome starts well below y=20 once that
/// row's own height and padding are counted). Caught by dumping
/// `step_00_full.png`, annotating both rectangles, and looking — the same
/// rule this investigation has needed twice before. These coordinates are
/// pixel-sampled directly from that capture: a smoothly-varying blurred-
/// content band confirmed to sit inside the purple glass bar, clear of the
/// "Library" text (ends well before x=120) and the round icon button
/// (starts after x=780).
const GLASS_REGION: (f32, f32, f32, f32) = (300.0, 125.0, 200.0, 35.0);
const CHANGE_CHANNEL_THRESHOLD: u8 = 6;
/// A single real one-pixel scroll must move some pixel in the sampled patch
/// by more than the noise floor. Zero is the freeze this test exists to
/// catch; this floor only forgives capture/compression jitter.
const MIN_CHANGED_PIXELS_PER_STEP: usize = 4;

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

            let baseline_path = output_dir.join("step_00_full.png");
            let mut prev_shot = capture_x11_window_screenshot(
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
