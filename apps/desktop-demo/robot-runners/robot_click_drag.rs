mod robot_launch;

mod robot_exit;

use std::time::Duration;

use cranpose_testing::{find_button, find_button_in_semantics, find_in_semantics, find_text};
use desktop_app::app;

fn main() {
    env_logger::init();
    println!("=== Robot Click Drag Test ===");
    println!("Testing button click behavior with drag cancellation and tab switching\n");

    const TEST_TIMEOUT_SECS: u64 = 60;

    robot_launch::launch("Robot Click Drag Test", 800, 600).with_test_driver(|robot| {
            robot_exit::arm_timeout(TEST_TIMEOUT_SECS);

            println!("✓ App launched\n");
            std::thread::sleep(Duration::from_millis(500));

            match robot.wait_for_idle() {
                Ok(_) => println!("✓ App ready\n"),
                Err(e) => println!("Note: {}\n", e),
            }

            let mut all_passed = true;

            println!("--- Test 1: Baseline Click Test ---");
            println!("Verifying that clicking Increment button works initially...\n");

            let initial_counter = find_in_semantics(&robot, |elem| find_text(elem, "Counter:"));
            if let Some((x, y, _w, _h)) = initial_counter {
                println!("  Initial counter found at ({:.1}, {:.1})", x, y);
            }

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

                if find_in_semantics(&robot, |elem| find_text(elem, "Counter: 1")).is_some() {
                    println!("  ✓ PASS: Counter incremented to 1\n");
                } else {
                    println!("  ✗ FAIL: Counter did not increment to 1\n");
                    all_passed = false;
                }
            } else {
                println!("  ✗ FAIL: Could not find 'Increment' button\n");
                all_passed = false;
            }

            println!("--- Test 2: Drag Should Cancel Click ---");
            println!("Dragging 20px on 'Increment' button should NOT increment counter...\n");

            if let Some((x, y, w, h)) = find_in_semantics(&robot, |elem| find_button(elem, "Increment")) {
                let cx = x + w / 2.0;
                let cy = y + h / 2.0;
                println!("  Found 'Increment' button at center ({:.1}, {:.1})", cx, cy);

                let _ = robot.mouse_move(cx, cy);
                std::thread::sleep(Duration::from_millis(50));
                let _ = robot.mouse_down();
                std::thread::sleep(Duration::from_millis(50));

                for i in 1..=10 {
                    let _ = robot.mouse_move(cx + (i as f32 * 2.0), cy);
                    std::thread::sleep(Duration::from_millis(10));
                }

                let _ = robot.mouse_up();
                std::thread::sleep(Duration::from_millis(200));

                if find_in_semantics(&robot, |elem| find_text(elem, "Counter: 1")).is_some() {
                    println!("  ✓ PASS: Counter still at 1 (drag cancelled click)\n");
                } else if find_in_semantics(&robot, |elem| find_text(elem, "Counter: 2")).is_some() {
                    println!("  ✗ FAIL: Counter incremented to 2 (drag DID NOT cancel click)\n");
                    all_passed = false;
                } else {
                    println!("  ? Could not verify counter state\n");
                }
            }

            println!("--- Test 3: Switch to Async Runtime Tab ---");

            if let Some((x, y, w, h)) = find_button_in_semantics(&robot, "Async Runtime") {
                let cx = x + w / 2.0;
                let cy = y + h / 2.0;
                println!("  Found 'Async Runtime' tab at ({:.1}, {:.1})", cx, cy);

                let _ = robot.mouse_move(cx, cy);
                std::thread::sleep(Duration::from_millis(50));
                let _ = robot.mouse_down();
                std::thread::sleep(Duration::from_millis(50));
                let _ = robot.mouse_up();
                std::thread::sleep(Duration::from_millis(500));

                if find_in_semantics(&robot, |elem| find_text(elem, "Async Runtime Demo")).is_some() {
                    println!("  ✓ Switched to Async Runtime tab\n");
                } else {
                    println!("  ? Could not verify Async Runtime tab content\n");
                }
            } else {
                println!("  ✗ Could not find 'Async Runtime' tab\n");
            }

            println!("--- Test 4: Switch Back to Counter App Tab ---");

            if let Some((x, y, w, h)) = find_button_in_semantics(&robot, "Counter App") {
                let cx = x + w / 2.0;
                let cy = y + h / 2.0;
                println!("  Found 'Counter App' tab at ({:.1}, {:.1})", cx, cy);

                let _ = robot.mouse_move(cx, cy);
                std::thread::sleep(Duration::from_millis(50));
                let _ = robot.mouse_down();
                std::thread::sleep(Duration::from_millis(50));
                let _ = robot.mouse_up();
                std::thread::sleep(Duration::from_millis(500));

                if find_in_semantics(&robot, |elem| find_text(elem, "Counter: 0")).is_some() {
                    println!("  ✓ Switched back to Counter App, counter reset to 0\n");
                } else {
                    println!("  ? Could not verify Counter App tab content\n");
                }
            } else {
                println!("  ✗ Could not find 'Counter App' tab\n");
            }

            println!("--- Test 5: Click Should Still Work After Tab Switching ---");
            println!("Clicking 'Increment' button after visiting Async Runtime tab...\n");

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

                if find_in_semantics(&robot, |elem| find_text(elem, "Counter: 1")).is_some() {
                    println!("  ✓ PASS: Counter incremented to 1 (buttons still work after tab switch)\n");
                } else {
                    if find_in_semantics(&robot, |elem| find_text(elem, "Counter: 0")).is_some() {
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
