use std::time::Duration;

use cranpose::AppLauncher;
use cranpose_testing::{exit_with_timeout, find_button_in_semantics, find_text_in_semantics};
use desktop_app::app;

fn main() {
    println!("=== Robot Fling Interrupt Test ===\n");

    AppLauncher::new()
        .with_title("Robot Fling Interrupt Test")
        .with_size(800, 600)
        .with_headless(true)
        .with_test_driver(|robot| {
            std::thread::sleep(Duration::from_millis(500));
            let _ = robot.wait_for_idle();

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

            for cycle in 0..8 {
                println!("--- Cycle {} ---", cycle + 1);

                if let Err(err) = robot.reset_last_fling_velocity() {
                    eprintln!("  Failed to reset fling velocity: {err}");
                    let _ = robot.exit();
                    return;
                }

                let fling_x = 400.0;
                let _ = robot.mouse_move(fling_x, 450.0);
                let _ = robot.mouse_down();
                let steps = 8;
                for i in 1..=steps {
                    let t = i as f32 / steps as f32;
                    let y = 450.0 + (200.0 - 450.0) * t;
                    let _ = robot.mouse_move(fling_x, y);
                    std::thread::sleep(Duration::from_millis(8));
                }
                let _ = robot.mouse_up();
                println!("  Fling released");

                std::thread::sleep(Duration::from_millis(20));
                for _ in 0..2 {
                    if let Err(err) = robot.click(click_x, click_y) {
                        eprintln!("  Click failed: {}", err);
                        break;
                    }
                    std::thread::sleep(Duration::from_millis(20));
                }

                std::thread::sleep(Duration::from_millis(120));
                println!("  Cycle {} complete", cycle + 1);
            }

            println!("\n=== Test Complete - No crash ===");
            exit_with_timeout(&robot, Duration::from_secs(2));
        })
        .run(|| {
            app::combined_app();
        });
}
