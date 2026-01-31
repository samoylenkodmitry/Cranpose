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
            } else {
                println!("FATAL: Could not find Lazy List tab");
                let _ = robot.exit();
                return;
            }

            // Run fling + interrupt cycles
            for cycle in 0..10 {
                println!("--- Cycle {} ---", cycle + 1);

                reset_last_fling_velocity();

                // Perform fling gesture
                let fling_x = 400.0;
                let _ = robot.mouse_move(fling_x, 450.0);
                std::thread::sleep(Duration::from_millis(20));
                let _ = robot.mouse_down();
                std::thread::sleep(Duration::from_millis(10));

                for i in 1..=5 {
                    let new_y = 450.0 - (250.0 * i as f32 / 5.0);
                    let _ = robot.mouse_move(fling_x, new_y);
                    std::thread::sleep(Duration::from_millis(8));
                }

                let _ = robot.mouse_up();
                println!("  Fling released");

                // Immediately click Start button multiple times
                for _ in 0..5 {
                    if let Some((x, y, w, h)) = find_button_in_semantics(&robot, "Start")
                        .or_else(|| find_text_in_semantics(&robot, "Start"))
                    {
                        let _ = robot.mouse_move(x + w / 2.0, y + h / 2.0);
                        let _ = robot.mouse_down();
                        let _ = robot.mouse_up();
                    }
                    std::thread::sleep(Duration::from_millis(5));
                }

                std::thread::sleep(Duration::from_millis(50));
                println!("  Cycle {} complete", cycle + 1);
            }

            println!("\n=== Test Complete - No crash ===");
            let _ = robot.exit();
        })
        .run(|| {
            app::combined_app();
        });
}
