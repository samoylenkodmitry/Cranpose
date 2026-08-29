//! Robot test: the receipts tab's fixed glass chrome must re-blur the
//! scrolling list underneath it when the list moves by a single pixel.
//!
//! This only reproduces through the real windowed, threaded present
//! pipeline `AppLauncher` drives here — the offscreen synchronous
//! `capture_frame` path the wgpu crate's own integration tests use shows no
//! staleness at all for the identical scene and widget, so the defect is
//! specific to interactive presentation, not to the backdrop cache's key
//! math (which a unit-level sweep confirms is already position-sensitive at
//! every single pixel — see `backdrop_cache_key_changes_at_every_single_pixel_of_prior_child_motion`
//! in `crates/cranpose-render/wgpu/src/surface_executor/render_paths.rs`).
//!
//! Run with:
//! ```bash
//! ./run_robot_test.sh --sequential --example robot_glass_scroll_continuity
//! ```

use std::time::Duration;

use cranpose::AppLauncher;
use cranpose_testing::robot_helpers::changed_pixel_count_in_region;
use desktop_app::app::{self, DemoTab};

const WINDOW_WIDTH: u32 = 400;
const WINDOW_HEIGHT: u32 = 700;
// Well inside the top bar's glass footprint (FeedChrome pads 8pt on three
// sides, TopBar is 56pt tall — see apps/desktop-demo/src/app/glass_feed.rs),
// clear of its rounded corners and the "Library" label / "..." button.
const GLASS_PATCH: (f32, f32, f32, f32) = (150.0, 20.0, 90.0, 20.0);
// A patch of raw (unblurred) list content, well below the chrome, as a
// control: if this ever failed to change too, the bug would be a general
// scroll stall rather than something specific to the glass backdrop.
const RAW_PATCH: (f32, f32, f32, f32) = (150.0, 400.0, 90.0, 20.0);

fn main() {
    let _ = env_logger::try_init();

    AppLauncher::new()
        .with_title("Robot Glass Scroll Continuity")
        .with_size(WINDOW_WIDTH, WINDOW_HEIGHT)
        .with_fonts(desktop_app::fonts::DEMO_FONTS)
        .with_headless(true)
        .with_test_driver(move |robot| {
            std::thread::sleep(Duration::from_millis(500));
            let _ = robot.wait_for_idle();

            // Settle into the list first: the sampled glass patch must
            // already show real, continuously-varying card content (not
            // the still-empty area above row 0) before the measured step,
            // or "no change yet" would just mean nothing is there yet to
            // change.
            robot
                .mouse_scroll(0.0, -40.0)
                .expect("settle scroll should succeed");
            std::thread::sleep(Duration::from_millis(900));
            let _ = robot.wait_for_idle();
            let before = robot.screenshot().expect("before screenshot");

            // The measured step: exactly one logical pixel.
            robot
                .mouse_scroll(0.0, -1.0)
                .expect("one-pixel scroll should succeed");
            std::thread::sleep(Duration::from_millis(900));
            let _ = robot.wait_for_idle();
            let after = robot.screenshot().expect("after screenshot");

            let glass_changed = changed_pixel_count_in_region(&before, &after, GLASS_PATCH, 0);
            let raw_changed = changed_pixel_count_in_region(&before, &after, RAW_PATCH, 0);
            println!("glass_changed_pixels={glass_changed} raw_changed_pixels={raw_changed}");

            assert!(
                raw_changed > 0,
                "the one-pixel scroll must move the raw list underneath the glass \
                 (raw_changed_pixels=0 means the scroll itself did not register)"
            );
            assert!(
                glass_changed > 0,
                "the glass chrome's pixels held over after a one-pixel scroll of the \
                 list underneath it instead of re-blurring (raw list patch changed \
                 {raw_changed} pixels in the same step, so the list itself did move)"
            );

            println!("PASS: glass chrome repainted after a one-pixel scroll");
            robot.exit().expect("exit");
        })
        .run(|| {
            app::combined_app_with_initial_tab(Some(DemoTab::GlassFeed));
        });
}
