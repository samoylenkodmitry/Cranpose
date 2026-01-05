//! Robot test for TextField reactive state integration.
//!
//! This test verifies that TextFieldState properly integrates with the composition
//! snapshot state system, triggering recomposition when text changes.
//!
//! Test case:
//! 1. Navigate to Text Input tab
//! 2. Find initial text field with "Type here..."
//! 3. Type "abc" via keyboard (appends to end)
//! 4. Verify "Current value: ..." label shows "Type here...abc" (reactive update)
//! 5. Press "Add !" button
//! 6. Verify "Current value: ..." label shows "Type here...abc!" (button update)
//!
//! Run with:
//! ```bash
//! cargo run --package desktop-app --example robot_reactive_state --features robot-app
//! ```

use compose_app::AppLauncher;
use compose_testing::{find_button, find_in_semantics, find_text};
use desktop_app::app;
use std::time::Duration;

fn main() {
    env_logger::init();
    println!("=== Robot Reactive State Test ===");
    println!("Verifying TextFieldState snapshot integration\n");

    AppLauncher::new()
        .with_title("Robot Reactive State Test")
        .with_size(900, 700)
        .with_test_driver(|robot| {
            // Timeout after 20 seconds
            std::thread::spawn(|| {
                std::thread::sleep(Duration::from_secs(20));
                eprintln!("TIMEOUT: Test exceeded 20 seconds");
                std::process::exit(1);
            });

            std::thread::sleep(Duration::from_millis(500));
            println!("✓ App launched and ready\n");

            let mut all_passed = true;

            // =========================================================
            // Step 1: Navigate to Text Input Tab
            // =========================================================
            println!("--- Step 1: Navigate to Text Input Tab ---");
            if let Some((x, y, w, h)) =
                find_in_semantics(&robot, |elem| find_text(elem, "Text Input"))
            {
                let _ = robot.mouse_move(x + w / 2.0, y + h / 2.0);
                std::thread::sleep(Duration::from_millis(30));
                let _ = robot.mouse_down();
                std::thread::sleep(Duration::from_millis(30));
                let _ = robot.mouse_up();
                std::thread::sleep(Duration::from_millis(500));
                let _ = robot.wait_for_idle();
                println!("  ✓ Clicked 'Text Input' tab\n");
            } else {
                println!("  ✗ Could not find 'Text Input' tab\n");
                all_passed = false;
            }

            // =========================================================
            // Step 2: Find and click text field with "Type here..."
            // =========================================================
            println!("--- Step 2: Click text field and type 'abc' ---");
            if let Some((x, y, w, h)) =
                find_in_semantics(&robot, |elem| find_text(elem, "Type here..."))
            {
                // Click near right edge to position cursor at end
                let _ = robot.mouse_move(x + w - 5.0, y + h / 2.0);
                std::thread::sleep(Duration::from_millis(30));
                let _ = robot.mouse_down();
                std::thread::sleep(Duration::from_millis(30));
                let _ = robot.mouse_up();
                std::thread::sleep(Duration::from_millis(200));
                println!(
                    "  Clicked text field at ({:.1}, {:.1})",
                    x + w - 5.0,
                    y + h / 2.0
                );

                // Type "abc"
                let _ = robot.type_text("abc");
                let _ = robot.wait_for_idle();
                std::thread::sleep(Duration::from_millis(300));
                println!("  Typed 'abc' via keyboard (NO button press!)\n");

                // =========================================================
                // Step 3: Verify "Current value" shows "abc" (reactive)
                // =========================================================
                println!("--- Step 3: Verify 'Current value' shows 'abc' (reactive update) ---");

                // Wait for semantics tree to rebuild after typing
                let _ = robot.wait_for_idle();
                std::thread::sleep(Duration::from_millis(300));

                // Helper to find element with BOTH patterns (recursive search)
                fn find_dual_text(
                    elem: &compose_app::SemanticElement,
                    pat1: &str,
                    pat2: &str,
                ) -> Option<(f32, f32, f32, f32)> {
                    if let Some(ref text) = elem.text {
                        if text.contains(pat1) && text.contains(pat2) {
                            return Some((
                                elem.bounds.x,
                                elem.bounds.y,
                                elem.bounds.width,
                                elem.bounds.height,
                            ));
                        }
                    }
                    for child in &elem.children {
                        if let Some(pos) = find_dual_text(child, pat1, pat2) {
                            return Some(pos);
                        }
                    }
                    None
                }

                let found_abc =
                    find_in_semantics(&robot, |elem| find_dual_text(elem, "Current value:", "abc"));

                if found_abc.is_some() {
                    println!("  ✓ PASS: 'Current value' label shows 'abc' after typing\n");
                } else {
                    println!("  ✗ FAIL: 'Current value' label did NOT update reactively!");
                    println!("         Expected label to contain 'abc' after keyboard typing.\n");
                    all_passed = false;
                }

                // =========================================================
                // Step 4: Press "Add !" button
                // =========================================================
                println!("--- Step 4: Press 'Add !' button ---");
                if let Some((bx, by, bw, bh)) =
                    find_in_semantics(&robot, |elem| find_button(elem, "Add !"))
                {
                    let _ = robot.mouse_move(bx + bw / 2.0, by + bh / 2.0);
                    std::thread::sleep(Duration::from_millis(30));
                    let _ = robot.mouse_down();
                    std::thread::sleep(Duration::from_millis(30));
                    let _ = robot.mouse_up();
                    std::thread::sleep(Duration::from_millis(300));
                    let _ = robot.wait_for_idle();
                    println!("  ✓ Clicked 'Add !' button\n");

                    // =========================================================
                    // Step 5: Verify "Current value" shows "abc!" (after Add !)
                    // =========================================================
                    println!("--- Step 5: Verify 'Current value' shows 'abc!' (after Add !) ---");

                    // Wait for recomposition after button click
                    let _ = robot.wait_for_idle();
                    std::thread::sleep(Duration::from_millis(300));

                    // Use same recursive finder as Step 3
                    let found_abc_exclaim = find_in_semantics(&robot, |elem| {
                        find_dual_text(elem, "Current value:", "abc!")
                    });

                    if found_abc_exclaim.is_some() {
                        println!("  ✓ PASS: 'Current value' label shows 'abc!' after Add !\n");
                    } else {
                        println!("  ✗ FAIL: 'Current value' label did NOT show 'abc!'!\n");
                        all_passed = false;
                    }
                } else {
                    println!("  ✗ Could not find 'Add !' button\n");
                    all_passed = false;
                }
            } else {
                println!("  ✗ Could not find text field with 'Type here...'\n");
                all_passed = false;
            }

            // =========================================================
            // Summary
            // =========================================================
            println!("=== Test Summary ===");
            if all_passed {
                println!("✓ ALL TESTS PASSED - TextFieldState snapshot integration works!");
            } else {
                println!("✗ SOME TESTS FAILED");
            }

            std::thread::sleep(Duration::from_secs(1));
            let _ = robot.exit();
        })
        .run(|| {
            app::combined_app();
        });
}
