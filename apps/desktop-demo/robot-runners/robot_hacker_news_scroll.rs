//! Robot test for Hacker News tab lazy list scroll behavior.
//!
//! Validates:
//! 1. The Hacker News list is constrained to the viewport (no infinite parent height).
//! 2. The list can scroll far enough to reveal later items.
//!
//! Run with:
//! ```bash
//! cargo run --package desktop-app --example robot_hacker_news_scroll --features robot-app
//! ```

mod robot_test_utils;

use cranpose::AppLauncher;
use cranpose_testing::{find_button_in_semantics, find_text_in_semantics};
use desktop_app::app;
use robot_test_utils::{find_element_by_text_exact, print_semantics_with_bounds};
use std::time::Duration;

fn main() {
    env_logger::init();
    println!("=== Hacker News Scroll Robot Test ===");

    AppLauncher::new()
        .with_title("Hacker News Scroll Test")
        .with_size(1200, 800)
        .with_headless(true)
        .with_test_driver(|robot| {
            std::thread::sleep(Duration::from_millis(500));
            let _ = robot.wait_for_idle();

            let click_button = |name: &str| -> bool {
                if let Some((x, y, w, h)) = find_button_in_semantics(&robot, name) {
                    robot.click(x + w / 2.0, y + h / 2.0).ok();
                    std::thread::sleep(Duration::from_millis(200));
                    return true;
                }
                println!("  ✗ Button '{}' not found!", name);
                false
            };

            let wait_for_text = |text: &str| -> bool {
                for _ in 0..40 {
                    if find_text_in_semantics(&robot, text).is_some() {
                        return true;
                    }
                    std::thread::sleep(Duration::from_millis(100));
                }
                false
            };

            // Navigate to Hacker News tab.
            if !click_button("Hacker News") {
                println!("FATAL: Could not find 'Hacker News' tab button");
                robot.exit().ok();
                std::process::exit(1);
            }

            // Wait for mocked stories to appear (robot-app feature).
            if !wait_for_text("Mock Story #1") {
                println!("FATAL: Mock stories did not appear");
                if let Ok(elements) = robot.get_semantics() {
                    print_semantics_with_bounds(&elements, 0);
                }
                robot.exit().ok();
                std::process::exit(1);
            }

            // Ensure list viewport is constrained.
            let semantics = robot.get_semantics().ok();
            let list_bounds = semantics
                .as_deref()
                .and_then(|elements| find_element_by_text_exact(elements, "HackerNewsList"))
                .map(|elem| {
                    (
                        elem.bounds.x,
                        elem.bounds.y,
                        elem.bounds.width,
                        elem.bounds.height,
                    )
                });

            let (list_x, list_y, list_w, list_h) = if let Some(bounds) = list_bounds {
                bounds
            } else {
                println!("  ✗ FAIL: HackerNewsList semantics not found");
                if let Some(elements) = semantics.as_deref() {
                    print_semantics_with_bounds(elements, 0);
                }
                robot.exit().ok();
                std::process::exit(1);
            };

            {
                let (x, y, w, h) = (list_x, list_y, list_w, list_h);
                println!(
                    "  ✓ HackerNewsList bounds=({:.1},{:.1},{:.1},{:.1})",
                    x, y, w, h
                );
                if h > 780.0 {
                    println!(
                        "  ✗ FAIL: HackerNewsList height {:.1} exceeds viewport expectations",
                        h
                    );
                    if let Some(elements) = semantics.as_deref() {
                        print_semantics_with_bounds(elements, 0);
                    }
                    robot.exit().ok();
                    std::process::exit(1);
                }
            }

            // Scroll to reveal later stories.
            let start_x = list_x + list_w / 2.0;
            let start_y = list_y + list_h * 0.75;
            let end_y = list_y + list_h * 0.25;

            for _ in 0..3 {
                robot.drag(start_x, start_y, start_x, end_y).ok();
                std::thread::sleep(Duration::from_millis(250));
                let _ = robot.wait_for_idle();
            }

            let story1_visible = find_text_in_semantics(&robot, "Mock Story #1").is_some();
            let story12_visible = find_text_in_semantics(&robot, "Mock Story #12").is_some();

            if story1_visible && !story12_visible {
                println!("  ✗ FAIL: Scroll did not reveal later stories");
                if let Ok(elements) = robot.get_semantics() {
                    print_semantics_with_bounds(&elements, 0);
                }
                robot.exit().ok();
                std::process::exit(1);
            }

            println!("  ✓ Scroll revealed later stories");
            let _ = robot.exit();
        })
        .run(app::combined_app);
}
