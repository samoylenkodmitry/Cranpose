//! Robot test that replicates scroll bug on Lazy List tab
//!
//! User's repro steps on LAZY LIST (infinite content):
//! 1. Go to Lazy List tab
//! 2. Scroll down (fast enough to trigger fling)
//! 3. Wait for fling to finish  
//! 4. Do a second scroll - content jumps back?
//!
//! Run with:
//! ```bash
//! cargo run --package desktop-app --example robot_scroll_jump --features robot-app
//! ```

use compose_app::{AppLauncher, Robot};
use compose_testing::{find_button, find_in_semantics, find_text};
use desktop_app::app;
use std::time::Duration;

fn main() {
    env_logger::init();
    println!("=== Lazy List Scroll Bug Test ===\n");

    AppLauncher::new()
        .with_title("Lazy List Scroll Bug")
        .with_size(800, 600)
        .with_headless(true)
        .with_test_driver(|robot| {
            std::thread::spawn(|| {
                std::thread::sleep(Duration::from_secs(120));
                eprintln!("✗ Test timed out");
                std::process::exit(1);
            });

            std::thread::sleep(Duration::from_millis(800));
            let _ = robot.wait_for_idle();
            println!("✓ App ready\n");

            // ============================================
            // STEP 1: Click Lazy List tab
            // ============================================
            println!("=== STEP 1: Click Lazy List tab ===");
            if let Some((x, y, w, h)) =
                find_in_semantics(&robot, |elem| find_button(elem, "Lazy List"))
            {
                let cx = x + w / 2.0;
                let cy = y + h / 2.0;
                println!("  Clicking at ({:.0}, {:.0})", cx, cy);
                let _ = robot.mouse_move(cx, cy);
                std::thread::sleep(Duration::from_millis(50));
                let _ = robot.mouse_down();
                std::thread::sleep(Duration::from_millis(50));
                let _ = robot.mouse_up();
                std::thread::sleep(Duration::from_millis(500));
                println!("  ✓ Tab clicked\n");
            } else {
                println!("  ✗ Lazy List tab not found!");
                let _ = robot.exit();
                return;
            }

            fn find_item_y(robot: &Robot, item_num: i32) -> Option<f32> {
                let text = format!("Item {}", item_num);
                find_in_semantics(robot, |elem| find_text(elem, &text))
                    .map(|(_, y, _, h)| y + h / 2.0)
            }

            // Find first visible item
            fn find_first_visible(robot: &Robot) -> Option<(i32, f32)> {
                for i in 0..50 {
                    if let Some(y) = find_item_y(robot, i) {
                        return Some((i, y));
                    }
                }
                None
            }

            // Record initial
            let (initial_item, initial_y) = find_first_visible(&robot).unwrap_or((0, 500.0));
            println!("Initial: Item {} at Y={:.1}\n", initial_item, initial_y);

            // ============================================
            // STEP 2: Fast scroll down with fling
            // ============================================
            println!("=== STEP 2: Fast scroll (will trigger fling) ===");

            let start_x = 400.0;
            let start_y = 450.0;

            let _ = robot.mouse_move(start_x, start_y);
            std::thread::sleep(Duration::from_millis(100));

            let _ = robot.mouse_down();
            std::thread::sleep(Duration::from_millis(20));

            // FAST drag - 200px in 50ms = 4000 px/sec
            for i in 1..=5 {
                let new_y = start_y - (40.0 * i as f32);
                let _ = robot.mouse_move(start_x, new_y);
                std::thread::sleep(Duration::from_millis(10));
            }

            let _ = robot.mouse_up();
            println!("  Fling triggered");

            // ============================================
            // STEP 3: Wait for fling to complete
            // ============================================
            println!("\n=== STEP 3: Wait for fling (1 second) ===");
            std::thread::sleep(Duration::from_millis(1000));

            let (after_fling_item, after_fling_y) = find_first_visible(&robot).unwrap_or((0, 0.0));
            println!(
                "  After fling: Item {} at Y={:.1}",
                after_fling_item, after_fling_y
            );
            println!("  Scrolled {} items", after_fling_item - initial_item);

            // ============================================
            // STEP 4: Second scroll - CHECK FOR JUMP
            // ============================================
            println!("\n=== STEP 4: Second scroll (CHECK FOR JUMP!) ===");

            let start_y_2 = 400.0;

            // Record before
            let (before_item, before_y) = find_first_visible(&robot).unwrap_or((0, 0.0));
            println!(
                "  BEFORE mouse down: Item {} at Y={:.1}",
                before_item, before_y
            );

            let _ = robot.mouse_move(start_x, start_y_2);
            std::thread::sleep(Duration::from_millis(50));

            let _ = robot.mouse_down();
            std::thread::sleep(Duration::from_millis(50));

            // Check after down
            let (after_down_item, after_down_y) = find_first_visible(&robot).unwrap_or((0, 0.0));
            println!(
                "  AFTER mouse down: Item {} at Y={:.1}",
                after_down_item, after_down_y
            );

            let item_jump = (after_down_item as i32 - before_item as i32).abs();
            if item_jump > 1 {
                println!(
                    "  ✗ JUMP DETECTED! Jumped {} items on mouse down!",
                    item_jump
                );
            } else {
                println!("  ✓ No significant jump");
            }

            // Drag
            let _ = robot.mouse_move(start_x, start_y_2 - 50.0);
            std::thread::sleep(Duration::from_millis(50));

            let (after_drag_item, _) = find_first_visible(&robot).unwrap_or((0, 0.0));
            println!("  AFTER drag: Item {}", after_drag_item);

            let _ = robot.mouse_up();

            // ============================================
            // VERDICT
            // ============================================
            println!("\n=== VERDICT ===");
            println!("Initial:       Item {}", initial_item);
            println!(
                "After fling:   Item {} (scrolled {} items)",
                after_fling_item,
                after_fling_item - initial_item
            );
            println!("Before 2nd:    Item {}", before_item);
            println!(
                "After 2nd down:Item {} (jumped {} items)",
                after_down_item, item_jump
            );

            if item_jump > 1 {
                println!(
                    "\n✗ TEST FAILED: Content jumped {} items on second scroll!",
                    item_jump
                );
                std::process::exit(1);
            } else if after_fling_item == initial_item {
                println!("\n⚠ WARNING: Fling didn't scroll - velocity tracking issue?");
            } else {
                println!("\n✓ TEST PASSED: No jump detected");
            }

            std::thread::sleep(Duration::from_secs(1));
            let _ = robot.exit();
        })
        .run(|| {
            app::combined_app();
        });
}
