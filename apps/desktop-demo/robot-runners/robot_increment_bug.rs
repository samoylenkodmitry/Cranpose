//! Robot test for button click after tab switch with cursor movement
//!
//! This test reproduces a bug where:
//! 1. Open app (starts on Counter App)
//! 2. Click on "CompositionLocal Test" tab
//! 3. Click back on "Counter App" tab
//! 4. Move cursor from "Counter App" button to "Increment" button (over gradient area)
//! 5. Click "Increment" - button doesn't work
//!
//! Run with:
//! ```bash
//! cargo run --package desktop-app --example robot_increment_bug --features robot-app
//! ```

use std::time::Duration;

use cranpose::AppLauncher;
use cranpose_testing::find_text_by_prefix_in_semantics;
use desktop_app::app;

fn fail(robot: &cranpose::Robot, message: &str) -> ! {
    let _ = robot;
    panic!("{message}");
}

fn counter_value(robot: &cranpose::Robot) -> Option<i32> {
    find_text_by_prefix_in_semantics(robot, "Counter:")
        .and_then(|(_, _, _, _, text)| text.split(':').nth(1)?.trim().parse().ok())
}

fn counter_values(robot: &cranpose::Robot) -> Vec<i32> {
    fn collect(elem: &cranpose::SemanticElement, out: &mut Vec<i32>) {
        if let Some(text) = &elem.text {
            if let Some(value) = text
                .strip_prefix("Counter:")
                .and_then(|value| value.trim().parse().ok())
            {
                out.push(value);
            }
        }
        for child in &elem.children {
            collect(child, out);
        }
    }

    let mut values = Vec::new();
    if let Ok(semantics) = robot.get_semantics() {
        for elem in &semantics {
            collect(elem, &mut values);
        }
    }
    values
}

fn wait_for_counter_value(
    robot: &cranpose::Robot,
    expected: i32,
    attempts: usize,
    delay: Duration,
) -> Option<i32> {
    for _ in 0..attempts {
        if let Some(value) = counter_value(robot) {
            if value == expected {
                return Some(value);
            }
        }
        let _ = robot.wait_for_idle();
        std::thread::sleep(delay);
    }

    let _ = robot.wait_for_idle();
    counter_value(robot)
}

fn main() {
    env_logger::init();
    println!("=== Robot Increment Button Bug Test ===");
    println!("Testing if Increment button works after tab switch + cursor movement\n");

    AppLauncher::new()
        .with_title("Robot Increment Bug Test")
        .with_size(800, 600)
        .with_headless(true)
        .with_test_driver(|robot| {
            println!("✓ App launched\n");
            std::thread::sleep(Duration::from_millis(500));

            match robot.wait_for_idle() {
                Ok(_) => println!("✓ App ready\n"),
                Err(e) => println!("Note: {}\n", e),
            }

            // Helper to find button center
            let find_button_center = |robot: &cranpose::Robot, name: &str| -> Option<(f32, f32)> {
                robot
                    .find_button_bounds(name)
                    .ok()
                    .flatten()
                    .map(|(x, y, w, h)| (x + w / 2.0, y + h / 2.0))
            };

            println!("--- Step 1: Verify Initial State ---");
            let initial_counter = counter_value(&robot)
                .unwrap_or_else(|| fail(&robot, "initial counter value not found"));
            println!("  Initial counter value: {}", initial_counter);

            println!("\n--- Step 2: Click CompositionLocal Test Tab ---");
            if let Some((x, y)) = find_button_center(&robot, "CompositionLocal Test") {
                println!(
                    "  Found 'CompositionLocal Test' tab at ({:.1}, {:.1})",
                    x, y
                );
                robot.click(x, y).unwrap_or_else(|err| {
                    fail(
                        &robot,
                        &format!("failed to click 'CompositionLocal Test': {err}"),
                    )
                });
                println!("  ✓ Clicked");
            } else {
                fail(&robot, "tab 'CompositionLocal Test' not found");
            }
            std::thread::sleep(Duration::from_millis(300));

            println!("\n--- Step 3: Click Counter App Tab ---");
            let counter_app_pos = find_button_center(&robot, "Counter App");
            if let Some((x, y)) = counter_app_pos {
                println!("  Found 'Counter App' tab at ({:.1}, {:.1})", x, y);
                robot.click(x, y).unwrap_or_else(|err| {
                    fail(&robot, &format!("failed to click 'Counter App': {err}"))
                });
                println!("  ✓ Clicked");
            } else {
                fail(&robot, "tab 'Counter App' not found");
            }
            std::thread::sleep(Duration::from_millis(300));

            println!("\n--- Step 4: Move Cursor Over Gradient Area ---");
            // Move from Counter App tab position through the gradient area (y ≈ 220+)
            // This triggers the gradient's pointer_input handler which updates state
            // and causes recomposition during the cursor movement.
            if let Some((tab_x, tab_y)) = counter_app_pos {
                println!(
                    "  Moving cursor from tab ({:.1}, {:.1}) through gradient area...",
                    tab_x, tab_y
                );

                // Move to a point in the gradient area (approximately y=220-250)
                // This is where the gradient's pointer_input handler tracks mouse position
                let gradient_x = 80.0;
                let gradient_y = 230.0;

                // Move in steps through the gradient area to trigger recomposition
                for step in 0..20 {
                    let progress = step as f32 / 19.0;
                    let x = tab_x + (gradient_x - tab_x) * progress;
                    let y = tab_y + (gradient_y - tab_y) * progress;
                    robot.mouse_move(x, y).unwrap_or_else(|err| {
                        fail(&robot, &format!("failed to move mouse through gradient area: {err}"))
                    });
                    std::thread::sleep(Duration::from_millis(25));
                }
                println!("  ✓ Cursor moved through gradient area (triggering recomposition)");
            } else {
                fail(&robot, "counter tab position missing before gradient walk");
            }
            std::thread::sleep(Duration::from_millis(200));

            println!("\n--- Step 5: Find and Click Increment Button ---");
            // The Increment button is INSIDE the gradient area, at approximately y=129
            // based on previous test runs finding it at (144.6, 129.4)
            let increment_pos = find_button_center(&robot, "Increment");
            if let Some((x, y)) = increment_pos {
                println!("  Found 'Increment' button at ({:.1}, {:.1})", x, y);

                // First move to the button position (may trigger additional recomposition)
                robot.mouse_move(x, y).unwrap_or_else(|err| {
                    fail(&robot, &format!("failed to move mouse to Increment button: {err}"))
                });
                std::thread::sleep(Duration::from_millis(100));

                // Then click - this tests that Up event reaches the button
                // even if Down event triggered recomposition
                robot.click(x, y).unwrap_or_else(|err| {
                    fail(&robot, &format!("failed to click Increment button: {err}"))
                });
                println!("  ✓ Clicked Increment");
            } else {
                fail(&robot, "Increment button not found after tab roundtrip");
            }
            std::thread::sleep(Duration::from_millis(300));

            println!("\n--- Step 6: Verify Counter Incremented ---");
            let _ = robot.wait_for_idle();
            let final_counter =
                wait_for_counter_value(&robot, initial_counter + 1, 40, Duration::from_millis(50))
                    .unwrap_or(-1);
            let all_counters = counter_values(&robot);
            println!("  Final counter value: {}", final_counter);
            println!("  All counter texts: {:?}", all_counters);

            println!("\n=== Test Summary ===");
            if final_counter == initial_counter + 1 {
                println!("✓ ALL TESTS PASSED");
                println!(
                    "  Counter incremented from {} to {}",
                    initial_counter, final_counter
                );
            } else {
                fail(
                    &robot,
                    &format!(
                        "counter value mismatch after robot click: expected {}, got {}, all counters {:?}",
                        initial_counter + 1,
                        final_counter,
                        all_counters,
                    ),
                );
            }

            println!("\nClosing in 1 second...");
            std::thread::sleep(Duration::from_secs(1));
            robot.exit().expect("Failed to shutdown");
            println!("Done!");
        })
        .run(|| {
            app::combined_app();
        });
}
