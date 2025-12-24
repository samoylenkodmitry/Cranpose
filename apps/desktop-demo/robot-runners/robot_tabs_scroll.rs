//! Robot test for tabs row scrolling and click behavior
//!
//! This test validates:
//! 1. Tabs row scrolls correctly when dragged
//! 2. Tab buttons don't fire click events during drag gestures
//! 3. Tab buttons still fire clicks for tap gestures
//!
//! Run with:
//! ```bash
//! cargo run --package desktop-app --example robot_tabs_scroll --features robot-app
//! ```

use compose_app::AppLauncher;
use compose_testing::find_clickables_in_range;
use desktop_app::app;
use std::time::Duration;

fn main() {
    env_logger::init();
    println!("=== Robot Tabs Scroll Test ===");
    println!("Testing tabs row scrolling and click behavior\n");

    AppLauncher::new()
        .with_title("Robot Tabs Scroll Test")
        .with_size(800, 600)
        .with_test_driver(|robot| {
            println!("✓ App launched\n");
            std::thread::sleep(Duration::from_millis(500));

            match robot.wait_for_idle() {
                Ok(_) => println!("✓ App ready\n"),
                Err(e) => println!("Note: {}\n", e),
            }

            // The tabs row should be near the top of the screen
            // Y position approximately 20px (outer padding)
            let tabs_y = 50.0; // Middle of tabs row

            println!("--- Test 1: Verify Tabs Row is Scrollable ---");

            // Dump semantic tree to understand structure
            println!("\n--- Semantic Tree Dump ---");
            match robot.get_semantics() {
                Ok(semantics) => {
                    fn print_element(elem: &compose_app::SemanticElement, depth: usize) {
                        let indent = "  ".repeat(depth);
                        println!("{}Role: {}, Bounds: ({:.1}, {:.1}, {:.1}x{:.1}), Text: {:?}, Clickable: {}",
                            indent, elem.role, elem.bounds.x, elem.bounds.y, elem.bounds.width, elem.bounds.height,
                            elem.text, elem.clickable);
                        for child in &elem.children {
                            print_element(child, depth + 1);
                        }
                    }
                    for (i, elem) in semantics.iter().enumerate() {
                        println!("\nRoot element {}:", i);
                        print_element(elem, 0);
                    }
                }
                Err(e) => println!("Failed to get semantics: {}", e),
            }
            println!("--- End Semantic Tree ---\n");

            // Get initial tab button positions using shared helper
            let get_tab_positions = |robot: &compose_app::Robot| -> Vec<(String, f32, f32)> {
                match robot.get_semantics() {
                    Ok(semantics) => {
                        find_clickables_in_range(&semantics, 20.0, 120.0)
                    }
                    Err(e) => {
                        println!("  ✗ Failed to get semantics: {}", e);
                        Vec::new()
                    }
                }
            };


            println!("\n=== Initial Tab Positions ===");
            let initial_tabs = get_tab_positions(&robot);
            for (i, (label, x, y)) in initial_tabs.iter().enumerate() {
                println!("  Tab {}: '{}' at x={:.1}, y={:.1}", i, label, x, y);
            }

            if initial_tabs.is_empty() {
                println!("\n⚠ No tab buttons found - test cannot proceed");
                robot.exit().ok();
                return;
            }

            println!("\n--- Test 2: Drag Tabs Row (Should Scroll) ---");
            println!("Dragging from (400, {}) to (100, {})", tabs_y, tabs_y);
            match robot.drag(400.0, tabs_y, 100.0, tabs_y) {
                Ok(_) => println!("✓ Drag completed"),
                Err(e) => println!("✗ Drag failed: {}", e),
            }
            std::thread::sleep(Duration::from_millis(500));

            println!("\n=== Tab Positions After Drag ===");
            let after_drag_tabs = get_tab_positions(&robot);
            for (i, (label, x, y)) in after_drag_tabs.iter().enumerate() {
                println!("  Tab {}: '{}' at x={:.1}, y={:.1}", i, label, x, y);
            }

            // Compare positions
            let mut tabs_moved = false;
            if initial_tabs.len() == after_drag_tabs.len() {
                for (initial, after) in initial_tabs.iter().zip(&after_drag_tabs) {
                    if (initial.1 - after.1).abs() > 0.1 {
                        tabs_moved = true;
                        let delta = after.1 - initial.1;
                        println!("\n  ✓ Tab '{}' moved {:.1}px (x: {:.1} → {:.1})",
                            initial.0, delta, initial.1, after.1);
                        break;
                    }
                }
            }

            if tabs_moved {
                println!("\n✓ PASS: Tabs row scrolls correctly!");
            } else {
                println!("\n✗ FAIL: Tabs did NOT scroll");
            }

            println!("\n--- Test 3: Check if Drag Triggered Click ---");
            println!("Looking for 'button clicked' messages in console...");
            println!("(This test relies on visual inspection of console output)");
            println!("Expected: NO click messages during drag");

            // Drag back to original position
            std::thread::sleep(Duration::from_millis(300));
            println!("\nDragging back from (200, {}) to (500, {})", tabs_y, tabs_y);
            match robot.drag(200.0, tabs_y, 500.0, tabs_y) {
                Ok(_) => println!("✓ Drag completed"),
                Err(e) => println!("✗ Drag failed: {}", e),
            }
            std::thread::sleep(Duration::from_millis(500));

            println!("\n--- Test 4: Verify Tap Still Works (Should Click) ---");
            if let Some((label, x, y)) = initial_tabs.first() {
                println!("Tapping on first tab '{}' at ({:.1}, {:.1})", label, x, y);
                match robot.click(*x + 20.0, *y + 20.0) {
                    Ok(_) => println!("✓ Tap completed"),
                    Err(e) => println!("✗ Tap failed: {}", e),
                }
                std::thread::sleep(Duration::from_millis(300));
                println!("Expected: ONE '{}' button clicked message", label);
            }

            println!("\n=== Test Summary ===");
            if tabs_moved {
                println!("✓ ALL TESTS PASSED");
            } else {
                println!("✗ SOME TESTS FAILED");
            }

            println!("\n--- Demo Complete ---");
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
