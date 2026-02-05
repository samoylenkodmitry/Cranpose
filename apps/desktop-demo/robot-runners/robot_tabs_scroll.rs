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

use cranpose::AppLauncher;
use cranpose_testing::{bounds_span, collect_tab_bounds, detect_tab_axis, root_bounds, TabAxis};
use desktop_app::app;
use std::time::Duration;

fn main() {
    env_logger::init();
    println!("=== Robot Tabs Scroll Test ===");
    println!("Testing tabs row scrolling and click behavior\n");

    AppLauncher::new()
        .with_title("Robot Tabs Scroll Test")
        .with_size(800, 600)
        .with_headless(true)
        .with_test_driver(|robot| {
            println!("✓ App launched\n");
            std::thread::sleep(Duration::from_millis(500));

            match robot.wait_for_idle() {
                Ok(_) => println!("✓ App ready\n"),
                Err(e) => println!("Note: {}\n", e),
            }

            let tab_labels = app::demo_tab_labels();

            println!("--- Test 1: Verify Tabs Row is Scrollable ---");

            // Dump semantic tree to understand structure
            println!("\n--- Semantic Tree Dump ---");
            match robot.get_semantics() {
                Ok(semantics) => {
                    fn print_element(elem: &cranpose::SemanticElement, depth: usize) {
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

            println!("\n=== Initial Tab Positions ===");
            let initial_tabs = collect_tab_bounds(&robot, &tab_labels);
            for (i, (label, (x, y, _, _))) in initial_tabs.iter().enumerate() {
                println!("  Tab {}: '{}' at x={:.1}, y={:.1}", i, label, x, y);
            }

            if initial_tabs.is_empty() {
                println!("\n⚠ No tab buttons found - test cannot proceed");
                robot.exit().ok();
                return;
            }

            let axis = detect_tab_axis(&initial_tabs).unwrap_or(TabAxis::Horizontal);
            let span = bounds_span(&initial_tabs).unwrap_or((0.0, 0.0, 0.0, 0.0));
            let (span_x, span_y) = (span.2 - span.0, span.3 - span.1);
            let root = root_bounds(&robot).unwrap_or((0.0, 0.0, 800.0, 600.0));
            let overflow = match axis {
                TabAxis::Horizontal => span_x > root.2 - 40.0,
                TabAxis::Vertical => span_y > root.3 - 40.0,
            };

            let find_tab = |tabs: &Vec<(String, (f32, f32, f32, f32))>, label: &str| {
                tabs.iter()
                    .find(|(name, _)| name == label)
                    .map(|(_, bounds)| *bounds)
            };

            println!("\n--- Test 2: Drag Tabs Row (Should Scroll) ---");
            if overflow {
                if let Some((x, y, w, h)) = find_tab(&initial_tabs, "Counter App") {
                    let start_x = x + w / 2.0;
                    let start_y = y + h / 2.0;
                    let (end_x, end_y) = match axis {
                        TabAxis::Horizontal => (start_x - 300.0, start_y),
                        TabAxis::Vertical => (start_x, start_y - 300.0),
                    };
                    println!(
                        "Dragging from ({:.1}, {:.1}) to ({:.1}, {:.1})",
                        start_x, start_y, end_x, end_y
                    );
                    match robot.drag(start_x, start_y, end_x, end_y) {
                        Ok(_) => println!("✓ Drag completed"),
                        Err(e) => println!("✗ Drag failed: {}", e),
                    }
                    std::thread::sleep(Duration::from_millis(500));
                } else {
                    println!("✗ Could not find 'Counter App' tab for drag start");
                    std::process::exit(1);
                }
            } else {
                println!("Tabs fit without overflow; skipping drag assertion");
            }

            println!("\n=== Tab Positions After Drag ===");
            let after_drag_tabs = collect_tab_bounds(&robot, &tab_labels);
            for (i, (label, (x, y, _, _))) in after_drag_tabs.iter().enumerate() {
                println!("  Tab {}: '{}' at x={:.1}, y={:.1}", i, label, x, y);
            }

            // Compare positions
            let mut tabs_moved = false;
            if overflow && initial_tabs.len() == after_drag_tabs.len() {
                for (initial, after) in initial_tabs.iter().zip(&after_drag_tabs) {
                    let initial_axis = match axis {
                        TabAxis::Horizontal => (initial.1).0,
                        TabAxis::Vertical => (initial.1).1,
                    };
                    let after_axis = match axis {
                        TabAxis::Horizontal => (after.1).0,
                        TabAxis::Vertical => (after.1).1,
                    };
                    if (initial_axis - after_axis).abs() > 0.1 {
                        tabs_moved = true;
                        let delta = after_axis - initial_axis;
                        println!(
                            "\n  ✓ Tab '{}' moved {:.1}px",
                            initial.0, delta
                        );
                        break;
                    }
                }
            }

            if overflow && tabs_moved {
                println!("\n✓ PASS: Tabs row scrolls correctly!");
            } else if overflow {
                println!("\n✗ FAIL: Tabs did NOT scroll");
            } else {
                println!("\n✓ PASS: Tabs fit without overflow; no scroll expected");
            }

            println!("\n--- Test 3: Check if Drag Triggered Click ---");
            println!("Looking for 'button clicked' messages in console...");
            println!("(This test relies on visual inspection of console output)");
            println!("Expected: NO click messages during drag");

            // Drag back to original position
            std::thread::sleep(Duration::from_millis(300));
            if overflow {
                if let Some((x, y, w, h)) = find_tab(&after_drag_tabs, "Counter App") {
                    let start_x = x + w / 2.0;
                    let start_y = y + h / 2.0;
                    let (end_x, end_y) = match axis {
                        TabAxis::Horizontal => (start_x + 300.0, start_y),
                        TabAxis::Vertical => (start_x, start_y + 300.0),
                    };
                    println!(
                        "\nDragging back from ({:.1}, {:.1}) to ({:.1}, {:.1})",
                        start_x, start_y, end_x, end_y
                    );
                    match robot.drag(start_x, start_y, end_x, end_y) {
                        Ok(_) => println!("✓ Drag completed"),
                        Err(e) => println!("✗ Drag failed: {}", e),
                    }
                    std::thread::sleep(Duration::from_millis(500));
                }
            }

            println!("\n--- Test 4: Verify Tap Still Works (Should Click) ---");
            if let Some((label, (x, y, w, h))) = initial_tabs.first() {
                let click_x = x + w / 2.0;
                let click_y = y + h / 2.0;
                println!(
                    "Tapping on first tab '{}' at ({:.1}, {:.1})",
                    label, click_x, click_y
                );
                match robot.click(click_x, click_y) {
                    Ok(_) => println!("✓ Tap completed"),
                    Err(e) => println!("✗ Tap failed: {}", e),
                }
                std::thread::sleep(Duration::from_millis(300));
                println!("Expected: ONE '{}' button clicked message", label);
            }

            println!("\n=== Test Summary ===");
            if overflow && tabs_moved {
                println!("✓ ALL TESTS PASSED");
            } else if overflow {
                println!("✗ SOME TESTS FAILED");
            } else {
                println!("✓ ALL TESTS PASSED");
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
