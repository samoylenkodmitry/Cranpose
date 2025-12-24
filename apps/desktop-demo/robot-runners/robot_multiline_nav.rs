//! Robot test for multiline text navigation (Up/Down arrows)
//!
//! Tests that cursor preserves column when moving between lines.
//! Run with:
//! ```bash
//! cargo run --package desktop-app --example robot_multiline_nav --features robot-app
//! ```

use compose_app::AppLauncher;
use compose_testing::{find_button, find_in_semantics, find_text_exact};
use desktop_app::app;
use std::time::Duration;

fn main() {
    env_logger::init();
    println!("=== Multiline Navigation Test ===\n");

    AppLauncher::new()
        .with_title("Multiline Nav Test")
        .with_size(600, 600)
        .with_test_driver(|robot| {
            // Timeout after 20 seconds
            std::thread::spawn(|| {
                std::thread::sleep(Duration::from_secs(20));
                println!("\n✗ Test timed out");
                std::process::exit(1);
            });

            std::thread::sleep(Duration::from_millis(300));
            println!("✓ App ready\n");

            // Step 1: Switch to Text Input tab
            println!("--- Step 1: Switch to Text Input Tab ---");
            if let Some((x, y, w, h)) =
                find_in_semantics(&robot, |elem| find_button(elem, "Text Input"))
            {
                let cx = x + w / 2.0;
                let cy = y + h / 2.0;
                let _ = robot.mouse_move(cx, cy);
                std::thread::sleep(Duration::from_millis(20));
                let _ = robot.mouse_down();
                std::thread::sleep(Duration::from_millis(20));
                let _ = robot.mouse_up();
                std::thread::sleep(Duration::from_millis(500));
                println!("✓ Clicked Text Input tab\n");
            } else {
                println!("✗ FAIL: Could not find Text Input tab");
                let _ = robot.exit();
            }

            // Step 2: Find text field
            println!("--- Step 2: Find text field ---");
            let text_field = find_in_semantics(&robot, |elem| find_text_exact(elem, ""));
            if text_field.is_none() {
                println!("✗ FAIL: Could not find text field");
                let _ = robot.exit();
            }
            let (fx, fy, fw, fh) = text_field.unwrap();
            let field_cx = fx + fw / 2.0;
            let field_cy = fy + fh / 2.0;
            println!("✓ Found text field at ({}, {})\n", fx as i32, fy as i32);

            // Step 3: Click to focus the text field
            println!("--- Step 3: Focus text field ---");
            let _ = robot.mouse_move(field_cx, field_cy);
            std::thread::sleep(Duration::from_millis(50));
            let _ = robot.mouse_down();
            std::thread::sleep(Duration::from_millis(20));
            let _ = robot.mouse_up();
            std::thread::sleep(Duration::from_millis(200));
            println!("✓ Clicked text field\n");

            // Step 4: Type multiline text using send_key
            println!("--- Step 4: Type multiline text ---");
            // Type "aaaa" then Enter, then "bb" then Enter, then "cccc"
            let _ = robot.send_key("a");
            let _ = robot.send_key("a");
            let _ = robot.send_key("a");
            let _ = robot.send_key("a");
            std::thread::sleep(Duration::from_millis(50));
            let _ = robot.send_key("Return"); // Enter key
            std::thread::sleep(Duration::from_millis(50));
            let _ = robot.send_key("b");
            let _ = robot.send_key("b");
            std::thread::sleep(Duration::from_millis(50));
            let _ = robot.send_key("Return"); // Enter key
            std::thread::sleep(Duration::from_millis(50));
            let _ = robot.send_key("c");
            let _ = robot.send_key("c");
            let _ = robot.send_key("c");
            let _ = robot.send_key("c");
            std::thread::sleep(Duration::from_millis(200));
            println!("✓ Typed multiline text: aaaa\\nbb\\ncccc\n");

            // Step 5: Navigate to specific column on line 3 and test Up arrow
            println!("--- Step 5: Test Up arrow column preservation ---");
            // Go to Home (start of current line = line 3)
            let _ = robot.send_key("Home");
            std::thread::sleep(Duration::from_millis(50));
            println!("  • Moved to Home (start of line 3)");

            // Move right twice to column 2 (after "cc")
            let _ = robot.send_key("Right");
            let _ = robot.send_key("Right");
            std::thread::sleep(Duration::from_millis(100));
            println!("  • Moved Right twice to column 2 (after 'cc')");

            // Press Up - should go to column 2 on line 2 (after "bb")
            let _ = robot.send_key("Up");
            std::thread::sleep(Duration::from_millis(100));
            println!("  • Pressed Up - should be at column 2 on line 2");

            // Press Up again - should go to column 2 on line 1 (after "aa")
            let _ = robot.send_key("Up");
            std::thread::sleep(Duration::from_millis(100));
            println!("  • Pressed Up - should be at column 2 on line 1");

            // Step 6: Test Down arrow
            println!("--- Step 6: Test Down arrow column preservation ---");
            let _ = robot.send_key("Down");
            std::thread::sleep(Duration::from_millis(100));
            println!("  • Pressed Down - should return to column 2 on line 2");

            let _ = robot.send_key("Down");
            std::thread::sleep(Duration::from_millis(100));
            println!("  • Pressed Down - should return to column 2 on line 3");

            // Step 7: Insert marker to verify position
            println!("--- Step 7: Insert marker to verify position ---");
            let _ = robot.send_key("x");
            std::thread::sleep(Duration::from_millis(200));

            // Print all text found in semantics for debugging
            println!("  Scanning semantics for text content...");
            let found_text: std::cell::RefCell<Option<String>> = std::cell::RefCell::new(None);
            find_in_semantics(&robot, |elem| {
                fn search_text(elem: &compose_app::SemanticElement, texts: &mut Vec<String>) {
                    if let Some(ref t) = elem.text {
                        texts.push(t.clone());
                    }
                    for child in &elem.children {
                        search_text(child, texts);
                    }
                }
                let mut texts = Vec::new();
                search_text(elem, &mut texts);
                for t in &texts {
                    if t.contains("aaaa") {
                        println!("  Found text: '{}'", t.replace('\n', "\\n"));
                        *found_text.borrow_mut() = Some(t.clone());
                    }
                }
                None::<(f32, f32, f32, f32)>
            });

            // Extract and verify
            let found = found_text.borrow().clone();
            if let Some(text) = found {
                // Expected: "aaaa\nbb\nccxcc" (x inserted at column 2 of line 3)
                if text == "aaaa\nbb\nccxcc" {
                    println!("✓ PASS: Column preserved correctly!\n");
                    println!("=== ✓ ALL TESTS PASSED ===");
                    let _ = robot.exit();
                } else {
                    println!(
                        "✗ FAIL: Expected 'aaaa\\nbb\\nccxcc' but got '{}'",
                        text.replace('\n', "\\n")
                    );
                    let _ = robot.exit();
                }
            } else {
                println!("✗ FAIL: Could not find text content containing 'aaaa'");
                let _ = robot.exit();
            }
        })
        .run(|| {
            app::combined_app();
        });
}
