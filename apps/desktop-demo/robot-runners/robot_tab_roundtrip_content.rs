//! Robot test for repeated tab round-trips across restored scrollable tabs.
//!
//! Scenario:
//! 1. CompositionLocal Test
//! 2. Web Fetch
//! 3. CompositionLocal Test
//! 4. Web Fetch
//! 5. Counter App
//!
//! Every tab must show its expected visible marker after each switch.

use cranpose::AppLauncher;
use cranpose_testing::{find_button_in_semantics, find_text_in_semantics};
use desktop_app::app;
use std::time::Duration;

fn wait_for_text(robot: &cranpose::Robot, text: &str, attempts: usize, delay: Duration) -> bool {
    for _ in 0..attempts {
        if find_text_in_semantics(robot, text).is_some() {
            return true;
        }
        let _ = robot.wait_for_idle();
        std::thread::sleep(delay);
    }
    false
}

fn click_tab(robot: &cranpose::Robot, label: &str) -> bool {
    let Some((x, y, w, h)) = find_button_in_semantics(robot, label) else {
        return false;
    };
    robot.click(x + w * 0.5, y + h * 0.5).ok();
    let _ = robot.wait_for_idle();
    true
}

fn fail(robot: &cranpose::Robot, message: &str) -> ! {
    let _ = robot;
    panic!("{message}");
}

fn main() {
    env_logger::init();
    println!("=== Robot Tab Roundtrip Content Test ===");

    AppLauncher::new()
        .with_title("Robot Tab Roundtrip Content Test")
        .with_size(1024, 768)
        .with_headless(true)
        .with_test_driver(|robot| {
            std::thread::sleep(Duration::from_millis(500));
            let _ = robot.wait_for_idle();

            let tab_walk = [
                ("CompositionLocal Test", "READING local:"),
                ("Web Fetch", "Fetch data from the web"),
                ("CompositionLocal Test", "READING local:"),
                ("Web Fetch", "Fetch data from the web"),
                ("Counter App", "Cranpose Playground"),
            ];

            for (tab, marker) in tab_walk {
                println!("--- Switching to '{}' ---", tab);
                if !click_tab(&robot, tab) {
                    fail(&robot, &format!("tab '{}' not found", tab));
                }
                if !wait_for_text(&robot, marker, 30, Duration::from_millis(100)) {
                    fail(
                        &robot,
                        &format!(
                            "marker '{}' did not appear after switching to '{}'",
                            marker, tab
                        ),
                    );
                }
            }

            println!("✓ ALL TESTS PASSED");
            robot.exit().ok();
        })
        .run(app::combined_app);
}
