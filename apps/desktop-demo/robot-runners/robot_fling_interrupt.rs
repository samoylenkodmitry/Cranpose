//! Robot test for fling + button click race condition
//!
//! Run with:
//! cargo run --package desktop-app --example robot_fling_interrupt --features robot-app

use cranpose::AppLauncher;
use cranpose_testing::{find_button_in_semantics, find_text_in_semantics};
use cranpose_ui::reset_last_fling_velocity;
use desktop_app::app;
use std::time::Duration;

fn main() {
    println!("=== Robot Fling Interrupt Test ===\n");

    AppLauncher::new()
        .with_title("Robot Fling Interrupt Test")
        .with_size(800, 600)
        .with_headless(true)
        .with_test_driver(|robot| {
            std::thread::sleep(Duration::from_millis(500));
            let _ = robot.wait_for_idle();

            // Navigate to Lazy List tab
            println!("Navigating to Lazy List Tab...");
            if let Some((x, y, w, h)) = find_button_in_semantics(&robot, "Lazy List")
                .or_else(|| find_text_in_semantics(&robot, "Lazy List"))
            {
                let _ = robot.click(x + w / 2.0, y + h / 2.0);
                std::thread::sleep(Duration::from_millis(300));
                let _ = robot.wait_for_idle();
            } else {
                println!("FATAL: Could not find Lazy List tab");
                let _ = robot.exit();
                return;
            }

            let start_button_bounds = {
                let mut bounds = None;
                for _ in 0..10 {
                    bounds = find_button_in_semantics(&robot, "Start")
                        .or_else(|| find_text_in_semantics(&robot, "Start"));
                    if bounds.is_some() {
                        break;
                    }
                    std::thread::sleep(Duration::from_millis(50));
                }
                bounds
            };

            let Some((start_x, start_y, start_w, start_h)) = start_button_bounds else {
                println!("FATAL: Could not find Start button");
                let _ = robot.exit();
                return;
            };

            let click_x = start_x + start_w / 2.0;
            let click_y = start_y + start_h / 2.0;

            // Run fling + interrupt cycles
            for cycle in 0..8 {
                println!("--- Cycle {} ---", cycle + 1);

                reset_last_fling_velocity();

                // Perform fling gesture
                let fling_x = 400.0;
                let _ = robot.drag(fling_x, 450.0, fling_x, 200.0);
                println!("  Fling released");

                // Immediately click Start button multiple times
                for _ in 0..5 {
                    let _ = robot.click(click_x, click_y);
                    std::thread::sleep(Duration::from_millis(5));
                }

                // Allow UI to process without blocking on a full idle wait.
                std::thread::sleep(Duration::from_millis(120));
                println!("  Cycle {} complete", cycle + 1);
            }

            println!("\n=== Test Complete - No crash ===");
            let _ = robot.exit();
        })
        .run(|| {
            app::combined_app();
        });
}
