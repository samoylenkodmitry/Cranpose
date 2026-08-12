//! `request_exit` must actually close the app.
//!
//! An app that owns the affordance meaning "leave" -- a screen-level dismiss
//! gesture it draws itself, a Quit item in its own menu -- has to be able to
//! ask the platform to close it. This drives the desktop backend: run the demo,
//! ask, and let the process end on its own. If the loop keeps turning, the
//! driver says so and fails rather than hanging a CI run forever.
//!
//! Run headless with `CRANPOSE_HEADLESS=1`.

use cranpose::AppLauncher;
use std::time::Duration;

const WINDOW_WIDTH: u32 = 700;
const WINDOW_HEIGHT: u32 = 500;
/// How long the loop gets to notice. Two orders of magnitude more than the
/// frame it should be noticed on, so a slow machine cannot fail this.
const GRACE: Duration = Duration::from_secs(3);

fn main() {
    AppLauncher::new()
        .with_title("request_exit")
        .with_size(WINDOW_WIDTH, WINDOW_HEIGHT)
        .with_fonts(desktop_app::fonts::DEMO_FONTS)
        .with_headless(std::env::var("CRANPOSE_HEADLESS").as_deref() == Ok("1"))
        .with_test_driver(move |robot| {
            let _ = robot.wait_for_idle();

            cranpose::request_exit();

            // Nothing else to do: the backend drains the request on its next
            // turn of the loop and exits. If it is still going after the grace
            // period, the request was dropped -- kill the process here, since
            // an unclosed window would otherwise wait for a human.
            std::thread::sleep(GRACE);
            println!(
                "FAIL: still running {}s after request_exit; the desktop backend \
                 did not drain the request",
                GRACE.as_secs(),
            );
            std::process::exit(1);
        })
        .try_run(desktop_app::app::DesktopApp)
        .expect("launch the demo");

    // `try_run` returning is the loop having exited, which is the whole point.
    println!("PASS: request_exit closed the app");
}
