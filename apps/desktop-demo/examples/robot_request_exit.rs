use std::time::Duration;

use cranpose::AppLauncher;

const WINDOW_WIDTH: u32 = 700;
const WINDOW_HEIGHT: u32 = 500;
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

    println!("PASS: request_exit closed the app");
}
