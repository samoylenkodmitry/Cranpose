//! Robot test for text selection, copy, and paste functionality
//!
//! This test validates:
//! 1. Navigate to "Text Input" tab
//! 2. Focus the first input field
//! 3. Select the last 3 characters using Shift+Left
//! 4. Copy selected text with Ctrl+C
//! 5. Paste the copied text twice with Ctrl+V
//! 6. Validate the resulting text
//! 7. Press "Add !" button
//! 8. Validate final text
//!
//! Run with:
//! ```bash
//! cargo run --package desktop-app --example robot_copy_paste --features robot-app
//! ```

use compose_app::AppLauncher;
use compose_testing::{find_button, find_in_semantics, find_text};
use desktop_app::app;
use std::time::Duration;

fn main() {
    env_logger::init();
    println!("=== Robot Copy-Paste Test ===");
    println!("Testing text selection, copy, and paste functionality\n");

    AppLauncher::new()
        .with_title("Robot Copy-Paste Test")
        .with_size(900, 700)
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
            // TEST 1: Switch to Text Input tab
            // =========================================================
            println!("--- Test 1: Switch to Text Input Tab ---");

            if let Some((x, y, w, h)) =
                find_in_semantics(&robot, |elem| find_button(elem, "Text Input"))
            {
                let cx = x + w / 2.0;
                let cy = y + h / 2.0;
                println!("  Found 'Text Input' tab at ({:.1}, {:.1})", cx, cy);

                let _ = robot.mouse_move(cx, cy);
                std::thread::sleep(Duration::from_millis(50));
                let _ = robot.mouse_down();
                std::thread::sleep(Duration::from_millis(50));
                let _ = robot.mouse_up();
                std::thread::sleep(Duration::from_millis(500));

                // Verify we switched
                if find_in_semantics(&robot, |elem| find_text(elem, "Text Input Demo")).is_some() {
                    println!("  ✓ PASS: Switched to Text Input tab\n");
                } else {
                    println!("  ✗ FAIL: Could not verify Text Input tab content\n");
                    all_passed = false;
                }
            } else {
                println!("  ✗ FAIL: Could not find 'Text Input' tab\n");
                all_passed = false;
            }

            // =========================================================
            // TEST 2: Focus the first input field
            // =========================================================
            println!("--- Test 2: Focus First Input Field ---");

            // Find the text field by looking for initial "Type here..." text
            // then clear it and type our test content
            let text_field_focused = if let Some((x, y, w, h)) =
                find_in_semantics(&robot, |elem| find_text(elem, "Type here..."))
            {
                let cx = x + w / 2.0;
                let cy = y + h / 2.0;
                println!(
                    "  Found text field with 'Type here...' at ({:.1}, {:.1})",
                    cx, cy
                );

                // Click to focus
                let _ = robot.mouse_move(cx, cy);
                std::thread::sleep(Duration::from_millis(30));
                let _ = robot.mouse_down();
                std::thread::sleep(Duration::from_millis(30));
                let _ = robot.mouse_up();
                std::thread::sleep(Duration::from_millis(200));

                // Select all text (Ctrl+A) and replace with our test content
                let _ = robot.send_key_with_modifiers("a", false, true, false, false);
                std::thread::sleep(Duration::from_millis(100));

                // Type initial text "abcdef" (replaces selection)
                match robot.type_text("abcdef") {
                    Ok(_) => {
                        println!("  Typed 'abcdef' (replacing initial text)");
                        let _ = robot.wait_for_idle();
                        std::thread::sleep(Duration::from_millis(300));

                        // Verify text field has "abcdef"
                        if find_in_semantics(&robot, |elem| find_text(elem, "abcdef")).is_some() {
                            println!("  ✓ PASS: Text field focused and contains 'abcdef'\n");
                            true
                        } else {
                            println!("  ✗ FAIL: Text 'abcdef' not found after typing\n");
                            all_passed = false;
                            false
                        }
                    }
                    Err(e) => {
                        println!("  ✗ FAIL: Could not type text: {}\n", e);
                        all_passed = false;
                        false
                    }
                }
            } else {
                println!("  ✗ FAIL: Could not find text field with 'Type here...'\n");
                all_passed = false;
                false
            };

            if !text_field_focused {
                // Can't continue without focused text field
                println!("\n=== Test Summary ===");
                println!("✗ SOME TESTS FAILED (could not focus text field)");
                std::thread::sleep(Duration::from_secs(1));
                let _ = robot.exit();
                return;
            }

            // =========================================================
            // TEST 3: Select last 3 characters using Shift+Left
            // =========================================================
            println!("--- Test 3: Select Last 3 Characters (Shift+Left) ---");

            // Cursor should be at end after typing, so Shift+Left 3 times selects "def"
            for i in 1..=3 {
                match robot.send_key_with_modifiers("Left", true, false, false, false) {
                    Ok(_) => println!("  Shift+Left ({}/3)", i),
                    Err(e) => {
                        println!("  ✗ FAIL: Could not send Shift+Left: {}", e);
                        all_passed = false;
                    }
                }
                std::thread::sleep(Duration::from_millis(100));
            }
            let _ = robot.wait_for_idle();
            println!("  ✓ Selected last 3 characters ('def')\n");

            // =========================================================
            // TEST 4: Copy selected text with Ctrl+C
            // =========================================================
            println!("--- Test 4: Copy Selected Text (Ctrl+C) ---");

            match robot.send_key_with_modifiers("c", false, true, false, false) {
                Ok(_) => {
                    let _ = robot.wait_for_idle();
                    std::thread::sleep(Duration::from_millis(200));
                    println!("  ✓ Sent Ctrl+C (copy)\n");
                }
                Err(e) => {
                    println!("  ✗ FAIL: Could not send Ctrl+C: {}\n", e);
                    all_passed = false;
                }
            }

            // =========================================================
            // TEST 5: Paste copied text twice with Ctrl+V
            // =========================================================
            println!("--- Test 5: Paste Text Twice (Ctrl+V x2) ---");

            // First, move cursor to end (press End key, or Right 3 times to deselect and go to end)
            let _ = robot.send_key("End");
            std::thread::sleep(Duration::from_millis(100));

            // Paste first time
            match robot.send_key_with_modifiers("v", false, true, false, false) {
                Ok(_) => {
                    let _ = robot.wait_for_idle();
                    std::thread::sleep(Duration::from_millis(200));
                    println!("  Pasted first time (Ctrl+V)");
                }
                Err(e) => {
                    println!("  ✗ FAIL: Could not send first Ctrl+V: {}", e);
                    all_passed = false;
                }
            }

            // Paste second time
            match robot.send_key_with_modifiers("v", false, true, false, false) {
                Ok(_) => {
                    let _ = robot.wait_for_idle();
                    std::thread::sleep(Duration::from_millis(200));
                    println!("  Pasted second time (Ctrl+V)");
                    println!("  ✓ Pasted text twice\n");
                }
                Err(e) => {
                    println!("  ✗ FAIL: Could not send second Ctrl+V: {}\n", e);
                    all_passed = false;
                }
            }

            // =========================================================
            // TEST 6: Validate resulting text
            // =========================================================
            println!("--- Test 6: Validate Resulting Text ---");

            // After operations:
            // - Started with "abcdef" (cursor at end)
            // - Selected last 3 chars: "def"
            // - Copied "def"
            // - Moved cursor to end (after "def", so at position 6)
            // - Pasted "def" twice -> "abcdefdefdef"
            let expected_text = "abcdefdefdef";

            std::thread::sleep(Duration::from_millis(300));
            if find_in_semantics(&robot, |elem| find_text(elem, expected_text)).is_some() {
                println!("  ✓ PASS: Text field contains '{}'\n", expected_text);
            } else if find_in_semantics(&robot, |elem| {
                find_text(elem, &format!("Current value: \"{}\"", expected_text))
            })
            .is_some()
            {
                println!("  ✓ PASS: Current value shows '{}'\n", expected_text);
            } else {
                // Check what text we actually have
                println!(
                    "  ✗ FAIL: Expected '{}' but got different text",
                    expected_text
                );
                println!("  Looking for actual text in semantics...");

                // Try to find any text containing "abc"
                if let Some((_, _, _, _)) = find_in_semantics(&robot, |elem| {
                    if let Some(ref text) = elem.text {
                        if text.contains("abc") {
                            println!("    Found text: '{}'", text);
                            return Some((
                                elem.bounds.x,
                                elem.bounds.y,
                                elem.bounds.width,
                                elem.bounds.height,
                            ));
                        }
                    }
                    None
                }) {
                    // Found something
                }
                all_passed = false;
                println!();
            }

            // =========================================================
            // TEST 7: Press "Add !" button
            // =========================================================
            println!("--- Test 7: Press 'Add !' Button ---");

            if let Some((x, y, w, h)) = find_in_semantics(&robot, |elem| find_button(elem, "Add !"))
            {
                let cx = x + w / 2.0;
                let cy = y + h / 2.0;
                println!("  Found 'Add !' button at ({:.1}, {:.1})", cx, cy);

                let _ = robot.mouse_move(cx, cy);
                std::thread::sleep(Duration::from_millis(50));
                let _ = robot.mouse_down();
                std::thread::sleep(Duration::from_millis(50));
                let _ = robot.mouse_up();
                std::thread::sleep(Duration::from_millis(300));
                let _ = robot.wait_for_idle();
                println!("  ✓ Pressed 'Add !' button\n");
            } else {
                println!("  ✗ FAIL: Could not find 'Add !' button\n");
                all_passed = false;
            }

            // =========================================================
            // TEST 8: Validate final text
            // =========================================================
            println!("--- Test 8: Validate Final Text ---");

            // After "Add !" button: "abcdefdefdef!"
            let final_text = format!("{}!", expected_text);

            std::thread::sleep(Duration::from_millis(200));
            if find_in_semantics(&robot, |elem| find_text(elem, &final_text)).is_some() {
                println!("  ✓ PASS: Text field contains '{}'\n", final_text);
            } else if find_in_semantics(&robot, |elem| {
                find_text(elem, &format!("Current value: \"{}\"", final_text))
            })
            .is_some()
            {
                println!("  ✓ PASS: Current value shows '{}'\n", final_text);
            } else {
                println!("  ✗ FAIL: Expected final text '{}'", final_text);
                // Debug: show what text we actually have
                println!("  Looking for actual text in semantics...");
                let _ = find_in_semantics(&robot, |elem| {
                    if let Some(ref text) = elem.text {
                        if text.contains("abc")
                            || text.contains("!")
                            || text.contains("Current value")
                        {
                            println!("    Found: '{}'", text);
                        }
                    }
                    None::<(f32, f32, f32, f32)>
                });
                all_passed = false;
                println!();
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
