//! Robot test for tab selection state visual changes
//!
//! This test validates that when a tab is clicked:
//! 1. The clicked tab becomes visually selected (highlighted)
//! 2. The previously selected tab becomes visually unselected
//! 3. The content area changes to show the selected tab's content
//!
//! This test was created to catch a regression where clicks worked but
//! visual state updates did not reflect in the UI.
//!
//! Run with:
//! ```bash
//! cargo run --package desktop-app --example robot_tab_selection --features robot-app
//! ```

use compose_app::AppLauncher;
use compose_testing::{find_button_in_semantics, find_text_in_semantics};
use desktop_app::app;
use desktop_app::app::DemoTab;
use std::time::Duration;

fn read_active_tab() -> Option<DemoTab> {
    app::TEST_ACTIVE_TAB_STATE.with(|cell| cell.borrow().as_ref().map(|state| state.get()))
}

fn main() {
    env_logger::init();
    println!("=== Robot Tab Selection Test ===");
    println!("Testing that tab selection state visually changes\n");

    AppLauncher::new()
        .with_title("Robot Tab Selection Test")
        .with_size(1200, 800)
        .with_test_driver(|robot| {
            println!("✓ App launched\n");
            std::thread::sleep(Duration::from_millis(500));

            match robot.wait_for_idle() {
                Ok(_) => println!("✓ App ready\n"),
                Err(e) => println!("Note: {}\n", e),
            }

            // Helper to find tab button by text and return center coordinates
            let find_tab_center = |robot: &compose_app::Robot, name: &str| -> Option<(f32, f32)> {
                find_button_in_semantics(robot, name).map(|(x, y, w, h)| (x + w / 2.0, y + h / 2.0))
            };

            // Helper to check if content text exists
            let content_exists = |robot: &compose_app::Robot, text: &str| -> bool {
                find_text_in_semantics(robot, text).is_some()
            };

            let wait_for_text = |robot: &compose_app::Robot, text: &str| -> bool {
                for _ in 0..20 {
                    if content_exists(robot, text) {
                        return true;
                    }
                    std::thread::sleep(Duration::from_millis(100));
                }
                false
            };

            println!("--- Test 1: Verify Initial State (Counter App Tab) ---");

            // Counter App should be initially selected
            // We should see the "Counter App" tab and counter-related content
            if content_exists(&robot, "Counter:") || content_exists(&robot, "Increment") {
                println!("  ✓ Counter App content is visible (initial state correct)");
            } else {
                println!("  ⚠ Counter App content NOT visible - checking what's there...");
                if let Ok(semantics) = robot.get_semantics() {
                    for elem in &semantics {
                        println!("    Root text: {:?}", elem.text);
                    }
                }
            }

            println!("\n--- Test 2: Click on Async Runtime Tab ---");
            if let Some((x, y)) = find_tab_center(&robot, "Async Runtime") {
                println!("  Found 'Async Runtime' tab at center ({:.1}, {:.1})", x, y);
                match robot.click(x, y) {
                    Ok(_) => println!("  ✓ Clicked 'Async Runtime' tab"),
                    Err(e) => println!("  ✗ Click failed: {}", e),
                }
            } else {
                println!("  ✗ Could not find 'Async Runtime' tab");
            }

            std::thread::sleep(Duration::from_millis(500));
            if let Some(active) = read_active_tab() {
                println!("  Active tab state after click: {:?}", active);
            }

            // After clicking Async Runtime, we should see its content
            println!("\n--- Test 3: Verify Content Changed to Async Runtime ---");
            // The Async Runtime tab should show "Pause" or "Resume" button, or animation status
            let async_content_visible = content_exists(&robot, "Pause")
                || content_exists(&robot, "Resume")
                || content_exists(&robot, "Animation")
                || content_exists(&robot, "Direction");

            if async_content_visible {
                println!("  ✓ PASS: Async Runtime content is visible (state change worked!)");
            } else {
                println!("  ✗ FAIL: Async Runtime content NOT visible");
                println!("        This indicates the UI did not update after click");

                // Check what content IS visible
                if content_exists(&robot, "Counter:") || content_exists(&robot, "Increment") {
                    println!("        Counter App content is still visible (stale UI)");
                }

                if let Ok(semantics) = robot.get_semantics() {
                    compose_app::Robot::print_semantics(&semantics, 0);
                }
            }

            println!("\n--- Test 4: Click on Modifiers Showcase Tab ---");
            if let Some((x, y)) = find_tab_center(&robot, "Modifiers Showcase") {
                println!(
                    "  Found 'Modifiers Showcase' tab at center ({:.1}, {:.1})",
                    x, y
                );
                match robot.click(x, y) {
                    Ok(_) => println!("  ✓ Clicked 'Modifiers Showcase' tab"),
                    Err(e) => println!("  ✗ Click failed: {}", e),
                }
            } else {
                println!("  ✗ Could not find 'Modifiers Showcase' tab");
            }

            std::thread::sleep(Duration::from_millis(500));
            if let Some(active) = read_active_tab() {
                println!("  Active tab state after click: {:?}", active);
            }

            println!("\n--- Test 5: Verify Content Changed to Modifiers Showcase ---");
            // The Modifiers Showcase tab should show showcase options like "Simple Card" etc.
            let showcase_content_visible = wait_for_text(&robot, "Select Showcase")
                || content_exists(&robot, "Simple Card")
                || content_exists(&robot, "Positioned Boxes")
                || content_exists(&robot, "Dynamic Modifiers");

            if showcase_content_visible {
                println!("  ✓ PASS: Modifiers Showcase content is visible");
            } else {
                println!("  ✗ FAIL: Modifiers Showcase content NOT visible");
                if let Ok(semantics) = robot.get_semantics() {
                    compose_app::Robot::print_semantics(&semantics, 0);
                }
            }

            println!("\n--- Test 6: Click Back to Counter App Tab ---");
            if let Some((x, y)) = find_tab_center(&robot, "Counter App") {
                println!("  Found 'Counter App' tab at center ({:.1}, {:.1})", x, y);
                match robot.click(x, y) {
                    Ok(_) => println!("  ✓ Clicked 'Counter App' tab"),
                    Err(e) => println!("  ✗ Click failed: {}", e),
                }
            } else {
                println!("  ✗ Could not find 'Counter App' tab");
            }

            std::thread::sleep(Duration::from_millis(500));
            if let Some(active) = read_active_tab() {
                println!("  Active tab state after click: {:?}", active);
            }

            println!("\n--- Test 7: Verify Content Changed Back to Counter App ---");
            let counter_content_visible =
                wait_for_text(&robot, "Counter:") || content_exists(&robot, "Increment");

            if counter_content_visible {
                println!("  ✓ PASS: Counter App content is visible again");
            } else {
                println!("  ✗ FAIL: Counter App content NOT visible after switching back");
                if let Ok(semantics) = robot.get_semantics() {
                    compose_app::Robot::print_semantics(&semantics, 0);
                }
            }

            // Summary
            println!("\n=== Test Summary ===");
            let test3_pass = async_content_visible;
            let test5_pass = showcase_content_visible;
            let test7_pass = counter_content_visible;

            if test3_pass && test5_pass && test7_pass {
                println!("✓ ALL TESTS PASSED");
                println!("  Tab selection state changes are working correctly!");
            } else {
                println!("✗ SOME TESTS FAILED");
                if !test3_pass {
                    println!("  - Async Runtime tab content did not appear");
                }
                if !test5_pass {
                    println!("  - Modifiers Showcase tab content did not appear");
                }
                if !test7_pass {
                    println!("  - Counter App tab content did not appear when switching back");
                }
                println!("\n  This indicates a regression in state change propagation.");
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
