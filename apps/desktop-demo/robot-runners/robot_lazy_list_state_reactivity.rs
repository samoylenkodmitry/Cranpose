mod robot_launch;

mod robot_exit;

use std::time::Duration;

use cranpose_testing::{find_button_in_semantics, find_text_by_prefix_in_semantics};
use desktop_app::app;

fn main() {
    env_logger::init();
    println!("=== LazyListState Reactivity Test ===");
    println!("Testing that first_visible_item_index() triggers recomposition\n");

    robot_launch::launch("LazyListState Reactivity Test", 1200, 800)
        .with_test_driver(|robot| {
            robot_exit::arm_timeout(30);

            std::thread::sleep(Duration::from_millis(500));
            let _ = robot.wait_for_idle();
            println!("App launched and ready\n");

            let mut all_passed = true;

            println!("--- Step 1: Navigate to 'Lazy List' tab ---");
            if let Some((x, y, w, h)) = find_button_in_semantics(&robot, "Lazy List") {
                println!("  Found 'Lazy List' tab at ({:.1}, {:.1})", x, y);
                robot.click(x + w / 2.0, y + h / 2.0).ok();
                std::thread::sleep(Duration::from_millis(500));
                let _ = robot.wait_for_idle();
                println!("  Clicked 'Lazy List' tab\n");
            } else {
                println!("  FATAL: 'Lazy List' tab not found");
                robot.exit().ok();
                std::process::exit(1);
            }

            println!("--- Step 2: Verify initial 'FirstIndex: 0' ---");
            std::thread::sleep(Duration::from_millis(300));
            let _ = robot.wait_for_idle();

            let initial_index = find_text_by_prefix_in_semantics(&robot, "FirstIndex:");
            if let Some((_, _, _, _, text)) = initial_index {
                println!("  Found: '{}'", text);
                if text.contains("0") {
                    println!("  PASS: Initial FirstIndex is 0\n");
                } else {
                    println!("  WARN: Initial FirstIndex is not 0 (may be from previous scroll)\n");
                }
            } else {
                println!("  FAIL: 'FirstIndex:' text not found!");
                println!("  This indicates the UI element was not added correctly.\n");
                all_passed = false;
            }

            println!("--- Step 3: Click 'Jump to Middle' button ---");
            if let Some((x, y, w, h)) = find_button_in_semantics(&robot, "Jump to Middle") {
                println!("  Found 'Jump to Middle' button at ({:.1}, {:.1})", x, y);
                robot.click(x + w / 2.0, y + h / 2.0).ok();
                std::thread::sleep(Duration::from_millis(500));
                let _ = robot.wait_for_idle();
                println!("  Clicked 'Jump to Middle' button\n");
            } else {
                println!("  FAIL: 'Jump to Middle' button not found");
                all_passed = false;
            }

            println!("--- Step 4: Verify 'FirstIndex: 50' after jump ---");
            std::thread::sleep(Duration::from_millis(500));
            let _ = robot.wait_for_idle();

            println!("  Dumping semantics tree:");
            if let Ok(elements) = robot.get_semantics() {
                cranpose_testing::print_semantics_with_bounds(&elements, 2);
            }

            let after_jump_index = find_text_by_prefix_in_semantics(&robot, "FirstIndex:");
            if let Some((_, _, _, _, text)) = after_jump_index {
                println!("\n  Found: '{}'", text);
                if let Some(num_str) = text.strip_prefix("FirstIndex:").map(|s| s.trim()) {
                    if let Ok(num) = num_str.parse::<usize>() {
                        if num == 50 {
                            println!("  PASS: FirstIndex reactively updated to 50\n");
                        } else if num == 0 {
                            println!("  FAIL: FirstIndex is still 0 - REACTIVITY BUG CONFIRMED!");
                            println!("        The LazyListState.first_visible_item_index() read");
                            println!("        did not create a proper snapshot dependency.");
                            println!("        UI shows stale data after scrolling.\n");
                            all_passed = false;
                        } else {
                            if (45..=55).contains(&num) {
                                println!("  PASS: FirstIndex is {} (close to 50)\n", num);
                            } else {
                                println!("  WARN: FirstIndex is {} (expected ~50)\n", num);
                            }
                        }
                    } else {
                        println!("  WARN: Could not parse number from '{}'\n", num_str);
                    }
                }
            } else {
                println!("  FAIL: 'FirstIndex:' text not found after jump!\n");
                all_passed = false;
            }

            println!("--- Step 5: Click 'Start' button ---");
            let start_button = find_button_in_semantics(&robot, "Start")
                .or_else(|| find_button_in_semantics(&robot, "⏫ Start"));
            if let Some((x, y, w, h)) = start_button {
                println!("  Found 'Start' button at ({:.1}, {:.1})", x, y);
                robot.click(x + w / 2.0, y + h / 2.0).ok();
                std::thread::sleep(Duration::from_millis(500));
                let _ = robot.wait_for_idle();
                println!("  Clicked 'Start' button\n");
            } else {
                println!("  FAIL: 'Start' button not found");
                all_passed = false;
            }

            println!("--- Step 6: Verify 'FirstIndex: 0' after scrolling back ---");
            std::thread::sleep(Duration::from_millis(500));
            let _ = robot.wait_for_idle();

            let after_start_index = find_text_by_prefix_in_semantics(&robot, "FirstIndex:");
            if let Some((_, _, _, _, text)) = after_start_index {
                println!("  Found: '{}'", text);
                if let Some(num_str) = text.strip_prefix("FirstIndex:").map(|s| s.trim()) {
                    if let Ok(num) = num_str.parse::<usize>() {
                        if num == 0 {
                            println!("  PASS: FirstIndex reactively updated back to 0\n");
                        } else if num == 50 || num >= 45 {
                            println!(
                                "  FAIL: FirstIndex is still {} - REACTIVITY BUG CONFIRMED!",
                                num
                            );
                            println!("        The UI did not update after scroll_to_item(0).\n");
                            all_passed = false;
                        } else {
                            println!("  WARN: FirstIndex is {} (expected 0)\n", num);
                        }
                    }
                }
            } else {
                println!("  FAIL: 'FirstIndex:' text not found after scrolling back!\n");
                all_passed = false;
            }

            println!("=== Test Summary ===");
            if all_passed {
                println!(
                    "ALL TESTS PASSED - LazyListState.first_visible_item_index() is reactive!"
                );
            } else {
                println!("SOME TESTS FAILED - LazyListState reactivity bug detected!");
                println!("\nThe fix requires refactoring LazyListState to use separate");
                println!("MutableState<T> fields for each observable property instead of");
                println!("MutableState<Rc<RefCell<Inner>>>.");
            }

            std::thread::sleep(Duration::from_secs(1));
            robot.exit().ok();

            if !all_passed {
                std::process::exit(1);
            }
        })
        .run(app::combined_app);
}
