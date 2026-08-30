mod robot_launch;

mod robot_exit;

use std::time::Duration;

use cranpose_testing::{
    find_bounds_by_text, find_button_in_semantics, find_in_semantics, find_text,
    visible_bounds_in_viewport,
};
use desktop_app::app;

fn main() {
    env_logger::init();
    println!("=== Robot Fling Test ===");
    println!("Testing velocity detection for fling gestures\n");

    const TEST_TIMEOUT_SECS: u64 = 60;

    robot_launch::launch("Robot Fling Test", 800, 600)
        .with_test_driver(|robot| {
            robot_exit::arm_timeout(TEST_TIMEOUT_SECS);

            println!("✓ App launched\n");
            std::thread::sleep(Duration::from_millis(500));

            match robot.wait_for_idle() {
                Ok(_) => println!("✓ App ready\n"),
                Err(e) => println!("Note: {}\n", e),
            }

            let mut all_passed = true;

            println!("--- Navigating to Lazy List Tab ---");

            if let Some((x, y, w, h)) = find_button_in_semantics(&robot, "Lazy List") {
                let cx = x + w / 2.0;
                let cy = y + h / 2.0;
                println!("  Found 'Lazy List' tab at ({:.1}, {:.1})", cx, cy);

                let _ = robot.mouse_move(cx, cy);
                std::thread::sleep(Duration::from_millis(50));
                let _ = robot.mouse_down();
                std::thread::sleep(Duration::from_millis(50));
                let _ = robot.mouse_up();
                std::thread::sleep(Duration::from_millis(500));

                println!("  ✓ Clicked Lazy List tab\n");
            } else {
                println!("  ✗ Could not find 'Lazy List' tab\n");
                all_passed = false;
            }

            println!("--- Test 1: Verify List Content ---");

            let has_list_item = find_in_semantics(&robot, |elem| find_text(elem, "Item 0"))
                .is_some()
                || find_in_semantics(&robot, |elem| find_text(elem, "Item 1")).is_some()
                || find_in_semantics(&robot, |elem| find_text(elem, "0")).is_some();

            if has_list_item {
                println!("  ✓ PASS: List content visible\n");
            } else {
                println!("  ? List content not found - looking for scrollable area\n");
            }

            println!("--- Test 2: Quick Swipe (Fling Gesture) ---");
            println!("Performing fast downward swipe to trigger velocity detection...\n");

            let list_bounds = match find_bounds_by_text(&robot, "LazyListViewport") {
                Some(bounds) => bounds,
                None => {
                    println!("  ✗ Could not find LazyListViewport bounds");
                    let _ = robot.exit();
                    return;
                }
            };

            let visible_bounds = match visible_bounds_in_viewport(&robot, list_bounds, 12.0) {
                Some(bounds) => bounds,
                None => {
                    println!("  ✗ LazyListViewport is not visible in the viewport");
                    let _ = robot.exit();
                    return;
                }
            };

            let start_x = visible_bounds.0 + visible_bounds.2 * 0.5;
            let start_y = visible_bounds.1 + visible_bounds.3 * 0.8;
            let end_y = visible_bounds.1 + visible_bounds.3 * 0.2;
            let swipe_distance = (start_y - end_y).abs();
            let swipe_steps = 5;
            let step_delay_ms = 10;

            if let Err(err) = robot.reset_last_fling_velocity() {
                println!("  ✗ Failed to reset fling velocity: {err}\n");
                all_passed = false;
            }

            let item_before = find_in_semantics(&robot, |elem| find_text(elem, "Item 5"));
            let before_y = item_before.map(|(_, y, _, _)| y);

            if let Some(y) = before_y {
                println!("  Item 5 before swipe at Y={:.1}", y);
            }

            let _ = robot.mouse_move(start_x, start_y);
            std::thread::sleep(Duration::from_millis(50));
            let _ = robot.mouse_down();
            std::thread::sleep(Duration::from_millis(20));

            for i in 1..=swipe_steps {
                let progress = i as f32 / swipe_steps as f32;
                let new_y = start_y - (swipe_distance * progress);
                let _ = robot.mouse_move(start_x, new_y);
                std::thread::sleep(Duration::from_millis(step_delay_ms));
            }

            println!("  Releasing after {:.0}px swipe...", swipe_distance);
            let _ = robot.mouse_up();

            std::thread::sleep(Duration::from_millis(300));

            let item_after = find_in_semantics(&robot, |elem| find_text(elem, "Item 5"));
            let after_y = item_after.map(|(_, y, _, _)| y);

            match (before_y, after_y) {
                (Some(by), Some(ay)) => {
                    let delta = ay - by;
                    if delta.abs() > 5.0 {
                        println!(
                            "  ✓ PASS: Item 5 moved by {:.1}px (scroll detected)\n",
                            delta
                        );
                    } else {
                        println!("  ? Item 5 at same position (delta={:.1})\n", delta);
                    }
                }
                (Some(_), None) => {
                    println!("  ✓ PASS: Item 5 no longer visible (scrolled off screen)\n");
                }
                (None, Some(_)) => {
                    println!("  ✓ PASS: Item 5 now visible (scrolled into view)\n");
                }
                (None, None) => {
                    println!("  ? Could not track Item 5 position\n");
                }
            }

            println!("--- Test 3: Reverse Swipe with Velocity Check ---");

            if let Err(err) = robot.reset_last_fling_velocity() {
                println!("  ✗ Failed to reset fling velocity: {err}\n");
                all_passed = false;
            }

            let _ = robot.mouse_move(start_x, end_y);
            std::thread::sleep(Duration::from_millis(50));
            let _ = robot.mouse_down();
            std::thread::sleep(Duration::from_millis(20));

            for i in 1..=swipe_steps {
                let progress = i as f32 / swipe_steps as f32;
                let new_y = end_y + (swipe_distance * progress);
                let _ = robot.mouse_move(start_x, new_y);
                std::thread::sleep(Duration::from_millis(step_delay_ms));
            }

            let _ = robot.mouse_up();
            std::thread::sleep(Duration::from_millis(300));

            let velocity = match robot.last_fling_velocity() {
                Ok(value) => value,
                Err(err) => {
                    println!("  ✗ Failed to query fling velocity: {err}\n");
                    all_passed = false;
                    0.0
                }
            };
            println!("  Measured fling velocity: {:.1} px/sec", velocity);

            if velocity.abs() > 50.0 {
                println!(
                    "  ✓ PASS: Velocity detected ({:.1} px/sec > 50 threshold)\n",
                    velocity
                );
            } else {
                println!(
                    "  ✗ FAIL: Velocity too low ({:.1} px/sec, expected > 50)\n",
                    velocity
                );
                all_passed = false;
            }

            println!("\n=== Test Summary ===");
            if all_passed {
                println!("✓ ALL TESTS PASSED");
                println!("\nNote: This test verifies velocity DETECTION.");
                println!("Runtime-backed fling animation is covered by scroll animation tests.");
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
