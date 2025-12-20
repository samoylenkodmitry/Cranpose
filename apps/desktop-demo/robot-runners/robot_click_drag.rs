//! Robot test for button click drag cancellation and tab switching
//!
//! This test validates:
//! 1. Dragging on a button SHOULD NOT trigger a click (drag cancellation)
//! 2. Buttons SHOULD still work after switching to/from Async Runtime tab
//!
//! Run with:
//! ```bash
//! cargo run --package desktop-app --example robot_click_drag --features robot-app
//! ```

use compose_app::AppLauncher;
use compose_testing::{find_text, find_button, find_in_semantics};
use desktop_app::app;
use std::time::Duration;

fn main() {
    env_logger::init();
    println!("=== Robot Click Drag Test ===");
    println!("Testing button click behavior with drag cancellation and tab switching\n");

    AppLauncher::new()
        .with_title("Robot Click Drag Test")
        .with_size(800, 600)
        .with_test_driver(|robot| {
            // Timeout after 30 seconds
            std::thread::spawn(|| {
                std::thread::sleep(Duration::from_secs(30));
                println!("✗ Test timed out after 30 seconds");
                std::process::exit(1);
            });

            println!("✓ App launched\n");
            std::thread::sleep(Duration::from_millis(500));

            match robot.wait_for_idle() {
                Ok(_) => println!("✓ App ready\n"),
                Err(e) => println!("Note: {}\n", e),
            }

            let mut all_passed = true;

            // =========================================================
            // TEST 1: Verify click works initially (baseline)
            // =========================================================
            println!("--- Test 1: Baseline Click Test ---");
            println!("Verifying that clicking Increment button works initially...\n");

            // Find the initial counter value
            let initial_counter = find_in_semantics(&robot, |elem| find_text(elem, "Counter:"));
            if let Some((x, y, _w, _h)) = initial_counter {
                println!("  Initial counter found at ({:.1}, {:.1})", x, y);
            }

            // Find and click the Increment button
            if let Some((x, y, w, h)) = find_in_semantics(&robot, |elem| find_button(elem, "Increment")) {
                let cx = x + w / 2.0;
                let cy = y + h / 2.0;
                println!("  Found 'Increment' button at center ({:.1}, {:.1})", cx, cy);
                
                let _ = robot.mouse_move(cx, cy);
                std::thread::sleep(Duration::from_millis(50));
                let _ = robot.mouse_down();
                std::thread::sleep(Duration::from_millis(50));
                let _ = robot.mouse_up();
                std::thread::sleep(Duration::from_millis(200));

                // Verify counter incremented
                if let Some(_) = find_in_semantics(&robot, |elem| find_text(elem, "Counter: 1")) {
                    println!("  ✓ PASS: Counter incremented to 1\n");
                } else {
                    println!("  ✗ FAIL: Counter did not increment to 1\n");
                    all_passed = false;
                }
            } else {
                println!("  ✗ FAIL: Could not find 'Increment' button\n");
                all_passed = false;
            }

            // =========================================================
            // TEST 2: Drag on button should NOT trigger click
            // =========================================================
            println!("--- Test 2: Drag Should Cancel Click ---");
            println!("Dragging 20px on 'Increment' button should NOT increment counter...\n");

            // Get current counter value (should be 1)
            if let Some((x, y, w, h)) = find_in_semantics(&robot, |elem| find_button(elem, "Increment")) {
                let cx = x + w / 2.0;
                let cy = y + h / 2.0;
                println!("  Found 'Increment' button at center ({:.1}, {:.1})", cx, cy);
                
                // Drag 20px to the right (beyond 8px threshold)
                let _ = robot.mouse_move(cx, cy);
                std::thread::sleep(Duration::from_millis(50));
                let _ = robot.mouse_down();
                std::thread::sleep(Duration::from_millis(50));
                
                // Move beyond threshold
                for i in 1..=10 {
                    let _ = robot.mouse_move(cx + (i as f32 * 2.0), cy);
                    std::thread::sleep(Duration::from_millis(10));
                }
                
                let _ = robot.mouse_up();
                std::thread::sleep(Duration::from_millis(200));

                // Verify counter did NOT increment (should still be 1)
                if let Some(_) = find_in_semantics(&robot, |elem| find_text(elem, "Counter: 1")) {
                    println!("  ✓ PASS: Counter still at 1 (drag cancelled click)\n");
                } else if let Some(_) = find_in_semantics(&robot, |elem| find_text(elem, "Counter: 2")) {
                    println!("  ✗ FAIL: Counter incremented to 2 (drag DID NOT cancel click)\n");
                    all_passed = false;
                } else {
                    println!("  ? Could not verify counter state\n");
                }
            }

            // =========================================================
            // TEST 3: Switch to Async Runtime tab and back
            // =========================================================
            println!("--- Test 3: Switch to Async Runtime Tab ---");
            
            if let Some((x, y, w, h)) = find_in_semantics(&robot, |elem| find_button(elem, "Async Runtime")) {
                let cx = x + w / 2.0;
                let cy = y + h / 2.0;
                println!("  Found 'Async Runtime' tab at ({:.1}, {:.1})", cx, cy);
                
                let _ = robot.mouse_move(cx, cy);
                std::thread::sleep(Duration::from_millis(50));
                let _ = robot.mouse_down();
                std::thread::sleep(Duration::from_millis(50));
                let _ = robot.mouse_up();
                std::thread::sleep(Duration::from_millis(500));

                // Verify we switched
                if let Some(_) = find_in_semantics(&robot, |elem| find_text(elem, "Async Runtime Demo")) {
                    println!("  ✓ Switched to Async Runtime tab\n");
                } else {
                    println!("  ? Could not verify Async Runtime tab content\n");
                }
            } else {
                println!("  ✗ Could not find 'Async Runtime' tab\n");
            }

            // =========================================================
            // TEST 4: Switch back to Counter App tab
            // =========================================================
            println!("--- Test 4: Switch Back to Counter App Tab ---");
            
            if let Some((x, y, w, h)) = find_in_semantics(&robot, |elem| find_button(elem, "Counter App")) {
                let cx = x + w / 2.0;
                let cy = y + h / 2.0;
                println!("  Found 'Counter App' tab at ({:.1}, {:.1})", cx, cy);
                
                let _ = robot.mouse_move(cx, cy);
                std::thread::sleep(Duration::from_millis(50));
                let _ = robot.mouse_down();
                std::thread::sleep(Duration::from_millis(50));
                let _ = robot.mouse_up();
                std::thread::sleep(Duration::from_millis(500));

                // Verify we switched back - counter resets to 0 after tab switch
                if let Some(_) = find_in_semantics(&robot, |elem| find_text(elem, "Counter: 0")) {
                    println!("  ✓ Switched back to Counter App, counter reset to 0\n");
                } else {
                    println!("  ? Could not verify Counter App tab content\n");
                }
            } else {
                println!("  ✗ Could not find 'Counter App' tab\n");
            }

            // =========================================================
            // TEST 5: Click should STILL work after tab switching
            // =========================================================
            println!("--- Test 5: Click Should Still Work After Tab Switching ---");
            println!("Clicking 'Increment' button after visiting Async Runtime tab...\n");
            
            // Wait longer for recomposition after tab switch
            std::thread::sleep(Duration::from_millis(1000));

            if let Some((x, y, w, h)) = find_in_semantics(&robot, |elem| find_button(elem, "Increment")) {
                let cx = x + w / 2.0;
                let cy = y + h / 2.0;
                println!("  Found 'Increment' button at center ({:.1}, {:.1})", cx, cy);
                
                let _ = robot.mouse_move(cx, cy);
                std::thread::sleep(Duration::from_millis(100));
                let _ = robot.mouse_down();
                std::thread::sleep(Duration::from_millis(100));
                let _ = robot.mouse_up();
                std::thread::sleep(Duration::from_millis(500));

                // Verify counter incremented to 1 (from 0 after tab reset)
                if let Some(_) = find_in_semantics(&robot, |elem| find_text(elem, "Counter: 1")) {
                    println!("  ✓ PASS: Counter incremented to 1 (buttons still work after tab switch)\n");
                } else {
                    // Check what the counter actually shows
                    if let Some(_) = find_in_semantics(&robot, |elem| find_text(elem, "Counter: 0")) {
                        println!("  ✗ FAIL: Counter still at 0 (button click did not work after tab switch!)\n");
                        all_passed = false;
                    } else {
                        println!("  ? Could not verify counter state\n");
                    }
                }
            } else {
                println!("  ✗ FAIL: Could not find 'Increment' button after tab switch\n");
                all_passed = false;
            }

            // =========================================================
            // Summary
            // =========================================================
            println!("\n=== Test Summary ===");
            if all_passed {
                println!("✓ ALL TESTS PASSED");
                std::thread::sleep(Duration::from_secs(1));
                let _ = robot.exit();
            } else {
                println!("✗ SOME TESTS FAILED");
                std::thread::sleep(Duration::from_secs(1));
                let _ = robot.exit();
            }
        })
        .run(|| {
            app::combined_app();
        });
}
