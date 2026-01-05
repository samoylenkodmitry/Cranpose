//! Interactive robot demonstration using semantic tree
//!
//! This example shows how to use robot testing with semantic queries
//! to automate interactions. Watch the robot find and click elements by text!
//!
//! Run with:
//! ```bash
//! cargo run --package desktop-app --example robot_interactive --features robot-app
//! ```

use compose_app::{AppLauncher, Robot};
use desktop_app::app;
use std::time::Duration;

fn wait_for_content(robot: &Robot, expected: &str, attempts: usize, delay: Duration) -> bool {
    for _ in 0..attempts {
        if robot.validate_content(expected).is_ok() {
            return true;
        }
        std::thread::sleep(delay);
    }
    false
}

fn main() {
    println!("=== Robot Interactive Demo (Semantic API) ===");
    println!("This demo uses semantic tree queries instead of hardcoded coordinates.\n");

    AppLauncher::new()
        .with_title("Robot Interactive Demo - Semantic API")
        .with_size(800, 600)
        .with_test_driver(|robot| {
            std::thread::sleep(Duration::from_millis(500));
            println!("✓ App launched\n");

            // Print semantic tree for debugging
            println!("--- Semantic Tree Structure ---");
            match robot.get_semantics() {
                Ok(sem) => Robot::print_semantics(&sem, 0),
                Err(e) => println!("Failed to get semantics: {}", e),
            }
            println!();

            // Workflow 1: Counter interactions using semantic queries
            println!("--- Workflow 1: Counter App (Semantic Clicking) ---");
            std::thread::sleep(Duration::from_secs(1));

            println!("Finding and clicking 'Increment' button 3 times...");
            for i in 1..=3 {
                println!("Click {}:", i);
                match robot.click_by_text("Increment") {
                    Ok(_) => println!("  ✓ Clicked successfully"),
                    Err(e) => println!("  Error: {}", e),
                }
                std::thread::sleep(Duration::from_millis(400));
            }

            std::thread::sleep(Duration::from_secs(1));
            println!("✓ Counter workflow complete\n");

            // Workflow 2: Tab navigation with semantic queries
            println!("--- Workflow 2: Tab Navigation (Semantic Queries) ---");
            std::thread::sleep(Duration::from_secs(1));

            let tabs = vec![
                ("Async Runtime", "Async Runtime Demo"),
                ("Modifiers Showcase", "Modifiers Showcase"),
                ("Counter App", "Increment"),
            ];

            for (tab_name, expected_content) in tabs {
                println!("Switching to '{}' tab...", tab_name);
                match robot.click_by_text(tab_name) {
                    Ok(_) => {}
                    Err(e) => {
                        println!("  Error clicking tab: {}", e);
                        continue;
                    }
                }

                if wait_for_content(&robot, expected_content, 10, Duration::from_millis(200)) {
                    println!("  ✓ Validated: found '{}'", expected_content);
                } else {
                    println!("  Warning: '{}' not found", expected_content);
                }

                std::thread::sleep(Duration::from_millis(500));
            }

            println!("✓ Tab navigation complete\n");

            // Keep window open
            println!("--- Demo Complete ---");
            println!("Window will stay open for 1 seconds...\n");

            for remaining in (1..=1).rev() {
                println!("Closing in {} seconds...", remaining);
                std::thread::sleep(Duration::from_secs(1));
            }

            println!("\nShutting down...");
            robot.exit().expect("Failed to shutdown");
            println!("Done!");
        })
        .run(|| {
            app::combined_app();
        });
}
