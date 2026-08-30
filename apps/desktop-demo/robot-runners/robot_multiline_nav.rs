mod robot_launch;

mod robot_exit;

use std::time::Duration;

use cranpose_testing::{find_in_semantics, find_text, find_text_exact};
use desktop_app::app;

mod text_input_robot_helpers;

fn main() {
    env_logger::init();
    println!("=== Multiline Navigation Test ===\n");

    robot_launch::launch("Multiline Nav Test", 600, 600)
        .with_test_driver(|robot| {
            robot_exit::arm_timeout(60);

            std::thread::sleep(Duration::from_millis(300));
            println!("✓ App ready\n");

            println!("--- Step 1: Switch to Text Input Tab ---");
            if text_input_robot_helpers::open_text_input_tab(&robot) {
                println!("✓ Clicked Text Input tab\n");
            } else {
                println!("✗ FAIL: Could not find Text Input tab");
                let _ = robot.exit();
                return;
            }

            println!("--- Step 2: Find text field ---");
            let text_field = text_input_robot_helpers::wait_for_in_semantics(&robot, |robot| {
                find_in_semantics(robot, |elem| find_text(elem, "Empty Text Field:"))
            })
            .and_then(|_| {
                text_input_robot_helpers::wait_for_in_semantics(&robot, |robot| {
                    find_in_semantics(robot, |elem| find_text_exact(elem, ""))
                })
            });
            if text_field.is_none() {
                println!("✗ FAIL: Could not find text field");
                let _ = robot.exit();
                return;
            }
            let (fx, fy, fw, fh) = text_field.expect("checked above");
            let field_cx = fx + fw / 2.0;
            let field_cy = fy + fh / 2.0;
            println!("✓ Found text field at ({}, {})\n", fx as i32, fy as i32);

            println!("--- Step 3: Focus text field ---");
            let _ = robot.mouse_move(field_cx, field_cy);
            std::thread::sleep(Duration::from_millis(50));
            let _ = robot.mouse_down();
            std::thread::sleep(Duration::from_millis(20));
            let _ = robot.mouse_up();
            std::thread::sleep(Duration::from_millis(200));
            println!("✓ Clicked text field\n");

            println!("--- Step 4: Type multiline text ---");
            let _ = robot.send_key("a");
            let _ = robot.send_key("a");
            let _ = robot.send_key("a");
            let _ = robot.send_key("a");
            std::thread::sleep(Duration::from_millis(50));
            let _ = robot.send_key("Return");
            std::thread::sleep(Duration::from_millis(50));
            let _ = robot.send_key("b");
            let _ = robot.send_key("b");
            std::thread::sleep(Duration::from_millis(50));
            let _ = robot.send_key("Return");
            std::thread::sleep(Duration::from_millis(50));
            let _ = robot.send_key("c");
            let _ = robot.send_key("c");
            let _ = robot.send_key("c");
            let _ = robot.send_key("c");
            std::thread::sleep(Duration::from_millis(200));
            println!("✓ Typed multiline text: aaaa\\nbb\\ncccc\n");

            println!("--- Step 5: Test Up arrow column preservation ---");
            let _ = robot.send_key("Home");
            std::thread::sleep(Duration::from_millis(50));
            println!("  • Moved to Home (start of line 3)");

            let _ = robot.send_key("Right");
            let _ = robot.send_key("Right");
            std::thread::sleep(Duration::from_millis(100));
            println!("  • Moved Right twice to column 2 (after 'cc')");

            let _ = robot.send_key("Up");
            std::thread::sleep(Duration::from_millis(100));
            println!("  • Pressed Up - should be at column 2 on line 2");

            let _ = robot.send_key("Up");
            std::thread::sleep(Duration::from_millis(100));
            println!("  • Pressed Up - should be at column 2 on line 1");

            println!("--- Step 6: Test Down arrow column preservation ---");
            let _ = robot.send_key("Down");
            std::thread::sleep(Duration::from_millis(100));
            println!("  • Pressed Down - should return to column 2 on line 2");

            let _ = robot.send_key("Down");
            std::thread::sleep(Duration::from_millis(100));
            println!("  • Pressed Down - should return to column 2 on line 3");

            println!("--- Step 7: Insert marker to verify position ---");
            let _ = robot.send_key("x");
            std::thread::sleep(Duration::from_millis(200));

            println!("  Scanning semantics for text content...");
            let found_text: std::cell::RefCell<Option<String>> = std::cell::RefCell::new(None);
            find_in_semantics(&robot, |elem| {
                fn search_text(elem: &cranpose::SemanticElement, texts: &mut Vec<String>) {
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

            let found = found_text.borrow().clone();
            if let Some(text) = found {
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
