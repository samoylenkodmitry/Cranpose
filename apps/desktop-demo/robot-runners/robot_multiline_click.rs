//! Robot test for multiline click positioning
//!
//! Tests that clicking on different lines positions the cursor correctly.
//! Run with:
//! ```bash
//! cargo run --package desktop-app --example robot_multiline_click --features robot-app
//! ```

use compose_app::{AppLauncher, Robot};
use compose_testing::{find_button, find_in_semantics, find_text_exact};
use desktop_app::app;
use std::time::Duration;

fn main() {
    env_logger::init();
    println!("=== Multiline Click Positioning Test ===\n");

    AppLauncher::new()
        .with_title("Multiline Click Test")
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
                return;
            }

            // Step 2: Find the empty text field
            println!("--- Step 2: Find empty text field ---");
            // Look for text field with empty string content after switching to Text Input tab
            let text_field = find_in_semantics(&robot, |elem| find_text_exact(elem, ""));
            if text_field.is_none() {
                println!("✗ FAIL: Could not find empty text field");
                let _ = robot.exit();
                return;
            }
            let (fx, fy, fw, fh) = text_field.unwrap();
            println!("✓ Found text field at ({}, {}, {}x{})\n", fx as i32, fy as i32, fw as i32, fh as i32);

            // Step 3: Click to focus the text field
            println!("--- Step 3: Focus text field ---");
            let _ = robot.mouse_move(fx + 10.0, fy + 10.0);
            std::thread::sleep(Duration::from_millis(50));
            let _ = robot.mouse_down();
            std::thread::sleep(Duration::from_millis(20));
            let _ = robot.mouse_up();
            std::thread::sleep(Duration::from_millis(200));
            println!("✓ Clicked text field\n");

            // Step 4: Type 3 simple lines
            println!("--- Step 4: Type 3 lines of text ---");
            // Type aaa, newline, bbb, newline, ccc (lowercase for send_key compatibility)
            let _ = robot.send_key("a");
            let _ = robot.send_key("a");
            let _ = robot.send_key("a");
            std::thread::sleep(Duration::from_millis(50));
            let _ = robot.send_key("Return");
            std::thread::sleep(Duration::from_millis(50));
            let _ = robot.send_key("b");
            let _ = robot.send_key("b");
            let _ = robot.send_key("b");
            std::thread::sleep(Duration::from_millis(50));
            let _ = robot.send_key("Return");
            std::thread::sleep(Duration::from_millis(50));
            let _ = robot.send_key("c");
            let _ = robot.send_key("c");
            let _ = robot.send_key("c");
            std::thread::sleep(Duration::from_millis(200));
            println!("✓ Typed text: aaa\\nbbb\\nccc\n");

            // Print current text state
            println!("--- Step 5: Read current text state ---");
            print_all_texts(&robot);

            // Step 6: Click on line 2 using calculated Y
            println!("--- Step 6: Click on line 2 ---");
            // Line height is 20.0, so line 2 is at Y = fy + padding + 20*1 + 10
            // Let's try clicking in middle of line 2
            const LINE_HEIGHT: f32 = 20.0;
            const PADDING: f32 = 8.0;
            let line2_y = fy + PADDING + LINE_HEIGHT * 1.0 + LINE_HEIGHT / 2.0;
            let click_x = fx + 40.0; // Click in middle of line
            
            println!("  Field at ({}, {}), clicking at ({}, {}) for line 2", 
                     fx as i32, fy as i32, click_x as i32, line2_y as i32);
            let _ = robot.mouse_move(click_x, line2_y);
            std::thread::sleep(Duration::from_millis(50));
            let _ = robot.mouse_down();
            std::thread::sleep(Duration::from_millis(20));
            let _ = robot.mouse_up();
            std::thread::sleep(Duration::from_millis(200));
            println!("✓ Clicked on line 2\n");

            // Step 7: Type x to mark position
            println!("--- Step 7: Insert 'x' marker ---");
            let _ = robot.send_key("x");
            std::thread::sleep(Duration::from_millis(200));
            
            // Print text after click
            println!("--- Step 8: Verify marker position ---");
            print_all_texts(&robot);
            
            let text_result = find_multiline_text(&robot);
            
            if let Some(text) = &text_result {
                println!("  Found multiline text: '{}'", text.replace('\n', "\\n"));
                let lines: Vec<&str> = text.split('\n').collect();
                println!("  Lines: {:?}", lines);
                
                // Check if x is on line 2
                if lines.len() >= 2 && lines[1].contains('x') {
                    println!("✓ PASS: Marker 'x' correctly placed on line 2\n");
                } else if lines.len() >= 1 && lines[0].contains('x') {
                    println!("✗ FAIL: Marker 'x' on line 1 instead of line 2 (Y-coordinate ignored!)\n");
                    let _ = robot.exit();
                    return;
                } else if lines.len() >= 3 && lines[2].contains('x') {
                    println!("✗ FAIL: Marker 'x' on line 3 instead of line 2\n");
                    let _ = robot.exit();
                    return;
                } else {
                    println!("✗ FAIL: Marker 'x' not found where expected. Lines: {:?}", lines);
                    let _ = robot.exit();
                    return;
                }
            } else {
                println!("✗ FAIL: Could not find multiline text");
                let _ = robot.exit();
                return;
            }

            // Step 9: Click on line 3
            println!("--- Step 9: Click on line 3 ---");
            // First remove the X
            let _ = robot.send_key("BackSpace");
            std::thread::sleep(Duration::from_millis(100));
            
            let line3_y = fy + PADDING + LINE_HEIGHT * 2.0 + LINE_HEIGHT / 2.0;
            println!("  Clicking at ({}, {}) for line 3", click_x as i32, line3_y as i32);
            let _ = robot.mouse_move(click_x, line3_y);
            std::thread::sleep(Duration::from_millis(50));
            let _ = robot.mouse_down();
            std::thread::sleep(Duration::from_millis(20));
            let _ = robot.mouse_up();
            std::thread::sleep(Duration::from_millis(200));
            
            // Step 10: Type y marker
            let _ = robot.send_key("y");
            std::thread::sleep(Duration::from_millis(200));
            
            println!("--- Step 10: Verify final position ---");
            let text_result2 = find_multiline_text(&robot);
            
            if let Some(text) = &text_result2 {
                println!("  Final text: '{}'", text.replace('\n', "\\n"));
                let lines: Vec<&str> = text.split('\n').collect();
                
                if lines.len() >= 3 && lines[2].contains('y') {
                    println!("✓ PASS: Marker 'y' correctly placed on line 3\n");
                    println!("=== ✓ ALL TESTS PASSED ===");
                    let _ = robot.exit();
                } else {
                    println!("✗ FAIL: Expected 'y' on line 3, but got lines: {:?}", lines);
                    let _ = robot.exit();
                }
            } else {
                println!("✗ FAIL: Could not find text");
                let _ = robot.exit();
            }
        })
        .run(|| {
            app::combined_app();
        });
}

fn print_all_texts(robot: &Robot) {
    find_in_semantics(robot, |elem| {
        fn search(elem: &compose_app::SemanticElement) {
            if let Some(ref t) = elem.text {
                if !t.is_empty() {
                    println!("    Text: '{}'", t.replace('\n', "\\n"));
                }
            }
            for child in &elem.children {
                search(child);
            }
        }
        search(elem);
        None::<(f32, f32, f32, f32)>
    });
}

fn find_multiline_text(robot: &Robot) -> Option<String> {
    let result: std::cell::RefCell<Option<String>> = std::cell::RefCell::new(None);
    find_in_semantics(robot, |elem| {
        fn search(elem: &compose_app::SemanticElement, result: &std::cell::RefCell<Option<String>>) {
            if let Some(ref t) = elem.text {
                // Look for multiline text containing our test characters (lowercase)
                if t.contains('\n') && (t.contains('a') || t.contains('b') || t.contains('c')) {
                    *result.borrow_mut() = Some(t.clone());
                }
            }
            for child in &elem.children {
                search(child, result);
            }
        }
        search(elem, &result);
        None::<(f32, f32, f32, f32)>
    });
    result.into_inner()
}
