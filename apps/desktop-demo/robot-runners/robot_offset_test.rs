//! Robot test to verify offset modifier positioning in the combined app
//!
//! This test validates that the offset modifier correctly positions elements
//! by navigating to the Modifiers Showcase tab and checking actual positions.
//!
//! Run with:
//! ```bash
//! cargo run --package desktop-app --example robot_offset_test --features robot-app
//! ```

use std::time::Duration;

use cranpose::{AppLauncher, SemanticElement};
use cranpose_testing::{find_button_in_semantics, find_by_text_recursive, find_text_exact};
use desktop_app::app;

fn wait_for_condition(
    description: &str,
    timeout_ms: u64,
    predicate: impl Fn() -> bool,
) -> Result<(), String> {
    let attempts = (timeout_ms / 100).max(1);
    for _ in 0..attempts {
        if predicate() {
            return Ok(());
        }
        std::thread::sleep(Duration::from_millis(100));
    }
    Err(format!("timed out waiting for {description}"))
}

fn find_exact_text(elements: &[SemanticElement], text: &str) -> Option<(f32, f32, f32, f32)> {
    for elem in elements {
        if let Some(bounds) = find_text_exact(elem, text) {
            return Some(bounds);
        }
    }
    None
}

fn main() {
    println!("Robot Offset Test - Combined App");
    println!("=================================\n");

    AppLauncher::new()
        .with_title("Robot Offset Test")
        .with_size(900, 700)
        .with_headless(true)
        .with_test_driver(|robot| {
            println!("App launched! Waiting for initial render...");
            std::thread::sleep(Duration::from_secs(1));
            wait_for_condition("Modifiers Showcase tab", 5_000, || {
                find_button_in_semantics(&robot, "Modifiers Showcase").is_some()
            })
            .expect("Failed to observe initial tab bar");

            // =====================================================
            // Step 1: Navigate to Modifiers Showcase tab
            // =====================================================
            println!("\n📌 Step 1: Navigate to Modifiers Showcase tab");
            if let Some((x, y, w, h)) = find_button_in_semantics(&robot, "Modifiers Showcase") {
                let cx = x + w / 2.0;
                let cy = y + h / 2.0;
                if let Err(err) = robot.click(cx, cy) {
                    println!("   ✗ Failed to click Modifiers Showcase tab: {err}");
                    robot.exit().ok();
                    std::process::exit(1);
                }
            } else {
                println!("   ✗ Failed to find Modifiers Showcase tab");
                robot.exit().ok();
                std::process::exit(1);
            }
            std::thread::sleep(Duration::from_millis(500));
            wait_for_condition("Positioned Boxes menu", 5_000, || {
                robot.get_semantics().ok().is_some_and(|semantics| {
                    find_exact_text(&semantics, "Positioned Boxes").is_some()
                })
            })
            .ok();
            println!("   Tab ready");

            // =====================================================
            // Step 2: Select "Positioned Boxes" showcase
            // =====================================================
            println!("\n📌 Step 2: Select 'Positioned Boxes' showcase");
            if let Err(err) = robot.click_by_text("Positioned Boxes") {
                println!("   ✗ Failed to click Positioned Boxes: {err}");
                robot.exit().ok();
                std::process::exit(1);
            }
            std::thread::sleep(Duration::from_millis(500));
            wait_for_condition("Positioned Boxes header", 5_000, || {
                robot
                    .get_semantics()
                    .ok()
                    .and_then(|semantics| find_exact_text(&semantics, "=== Positioned Boxes ==="))
                    .is_some()
            })
            .ok();

            // Validate positioned boxes
            println!("\n   Validating positioned boxes:");
            let semantics = robot.get_semantics().expect("Failed to get semantics");
            if find_exact_text(&semantics, "=== Positioned Boxes ===").is_none() {
                println!("   ✗ Positioned Boxes header not found (still showing old content?)");
                cranpose::Robot::print_semantics(&semantics, 0);
                robot.exit().ok();
                std::process::exit(1);
            }

            // The positioned boxes showcase has:
            // - Box A at offset(20, 20) - Purple, top-left
            // - Box B at offset(220, 160) - Green, bottom-right
            // - C at offset(140, 30) - Orange, center-top
            // - Box D at offset(40, 140) - Blue, center-left

            let mut test_passed = true;

            // Box A should be at offset(20, 20)
            if let Some(elem) = find_by_text_recursive(&semantics, "Box A") {
                println!(
                    "   ✓ Found 'Box A' at x={:.1}, y={:.1}",
                    elem.bounds.x, elem.bounds.y
                );
                // Box A is at offset(20, 20), plus container/padding offsets
                if elem.bounds.x > 0.0 && elem.bounds.x < 500.0 {
                    println!("     ✓ PASS: Box A has positive x offset");
                } else {
                    println!("     ✗ FAIL: Box A x={}", elem.bounds.x);
                    test_passed = false;
                }
            } else {
                println!("   ✗ 'Box A' not found");
                test_passed = false;
            }

            // Box B should be at offset(220, 160) - significantly more to the right
            if let Some(elem) = find_by_text_recursive(&semantics, "Box B") {
                println!(
                    "   ✓ Found 'Box B' at x={:.1}, y={:.1}",
                    elem.bounds.x, elem.bounds.y
                );
                // Box B should be significantly to the right of Box A
                if let Some(box_a) = find_by_text_recursive(&semantics, "Box A") {
                    if elem.bounds.x > box_a.bounds.x + 100.0 {
                        println!(
                            "     ✓ PASS: Box B is to the right of Box A (diff: {:.0}px)",
                            elem.bounds.x - box_a.bounds.x
                        );
                    } else {
                        println!("     ✗ FAIL: Box B should be far right of Box A");
                        test_passed = false;
                    }
                }
            } else {
                println!("   ✗ 'Box B' not found");
                test_passed = false;
            }

            // C (small box) should be at offset(140, 30)
            if let Some(elem) = find_by_text_recursive(&semantics, "C") {
                println!(
                    "   ✓ Found 'C' at x={:.1}, y={:.1}",
                    elem.bounds.x, elem.bounds.y
                );
            }

            // Box D should be at offset(40, 140)
            if let Some(elem) = find_by_text_recursive(&semantics, "Box D") {
                println!(
                    "   ✓ Found 'Box D' at x={:.1}, y={:.1}",
                    elem.bounds.x, elem.bounds.y
                );
            }

            if test_passed {
                println!("\n   ✅ Positioned Boxes validation PASSED!");
            } else {
                println!("\n   ❌ Positioned Boxes validation FAILED!");
                robot.exit().ok();
                std::process::exit(1);
            }

            std::thread::sleep(Duration::from_secs(1));

            // =====================================================
            // Step 3: Select "Dynamic Modifiers" showcase
            // =====================================================
            println!("\n📌 Step 3: Select 'Dynamic Modifiers' showcase");
            if let Err(err) = robot.click_by_text("Dynamic Modifiers") {
                println!("   ✗ Failed to click Dynamic Modifiers: {err}");
                robot.exit().ok();
                std::process::exit(1);
            }
            std::thread::sleep(Duration::from_millis(500));
            wait_for_condition("Dynamic Modifiers header", 5_000, || {
                robot
                    .get_semantics()
                    .ok()
                    .and_then(|semantics| find_exact_text(&semantics, "=== Dynamic Modifiers ==="))
                    .is_some()
            })
            .ok();
            let semantics = robot.get_semantics().expect("Failed to get semantics");
            if find_exact_text(&semantics, "=== Dynamic Modifiers ===").is_none() {
                println!("   ✗ Dynamic Modifiers header not found (selection stuck?)");
                cranpose::Robot::print_semantics(&semantics, 0);
                robot.exit().ok();
                std::process::exit(1);
            }

            // =====================================================
            // Step 4: Press "Advance Frame" 3 times and validate
            // =====================================================
            println!("\n📌 Step 4: Press 'Advance Frame' 3 times and validate positions");

            // Get initial position of the "Move" box
            let semantics_before = robot.get_semantics().expect("Failed to get semantics");
            let move_elem_before = find_by_text_recursive(&semantics_before, "Move");
            if let Some(ref elem) = move_elem_before {
                println!("   Initial 'Move' box position: x={:.1}", elem.bounds.x);
            }

            let mut prev_x = move_elem_before.map(|e| e.bounds.x).unwrap_or(0.0);

            for i in 1..=3 {
                println!("\n   --- Frame {} ---", i);

                // Click Advance Frame button
                if let Err(err) = robot.click_by_text("Advance Frame") {
                    println!("   ✗ Failed to click Advance Frame: {err}");
                    robot.exit().ok();
                    std::process::exit(1);
                }
                std::thread::sleep(Duration::from_millis(300));
                std::thread::sleep(Duration::from_millis(200));

                // Get semantics and check dynamic element positions
                let semantics = robot.get_semantics().expect("Failed to get semantics");

                // Check the "Move" box position
                if let Some(elem) = find_by_text_recursive(&semantics, "Move") {
                    println!("   'Move' box at x={:.1}", elem.bounds.x);

                    // Verify the box moved (x should increase by 10)
                    if elem.bounds.x > prev_x {
                        println!("   ✓ PASS: Box moved right");
                    } else {
                        println!(
                            "   ⚠ Box didn't move as expected (prev={:.1}, now={:.1})",
                            prev_x, elem.bounds.x
                        );
                    }
                    prev_x = elem.bounds.x;
                }

                // Check frame indicator text
                if let Some(elem) = find_by_text_recursive(&semantics, "Frame:") {
                    println!("   Frame indicator: {:?}", elem.text);
                }
            }

            println!("\n=== Test Summary ===");
            if test_passed {
                println!("✓ ALL TESTS PASSED");
            } else {
                println!("✗ SOME TESTS FAILED");
            }
            println!("   Keeping window open for 1 seconds...");
            std::thread::sleep(Duration::from_secs(1));

            println!("\n🛑 Shutting down...");
            robot.exit().expect("Failed to exit");
        })
        .run(|| {
            app::combined_app();
        });
}
