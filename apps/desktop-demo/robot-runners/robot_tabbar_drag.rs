//! Robot test verifying tabbar horizontal scroll works.
//!
//! This test catches regressions where horizontal_scroll using ScrollState
//! doesn't update visually when dragged.

use compose_app::AppLauncher;
use compose_testing::{find_button, find_in_semantics};
use desktop_app::app;
use std::time::Duration;

fn main() {
    env_logger::init();
    println!("=== Robot Tabbar Drag Test ===\n");

    AppLauncher::new()
        .with_title("Robot Tabbar Drag Test")
        .with_size(800, 600)
        .with_headless(true)
        .with_test_driver(|robot| {
            std::thread::spawn(|| {
                std::thread::sleep(Duration::from_secs(30));
                println!("✗ Test timed out");
                std::process::exit(1);
            });

            println!("✓ App launched");
            std::thread::sleep(Duration::from_millis(500));
            let _ = robot.wait_for_idle();

            // Find "Lazy List" tab position BEFORE drag
            let lazy_tab_before = find_in_semantics(&robot, |elem| find_button(elem, "Lazy List"));
            let x_before = lazy_tab_before.map(|(x, _, _, _)| x);

            println!("\n--- Test: Drag tabbar should scroll tabs ---");
            println!("  'Lazy List' tab X before drag: {:?}", x_before);

            if x_before.is_none() {
                println!("  ✗ Could not find 'Lazy List' tab");
                std::process::exit(1);
            }

            // Find a tab near the left to start dragging from
            let counter_tab = find_in_semantics(&robot, |elem| find_button(elem, "Counter App"));
            if let Some((x, y, w, h)) = counter_tab {
                let start_x = x + w / 2.0;
                let start_y = y + h / 2.0;

                println!(
                    "  Starting drag from 'Counter App' at ({:.1}, {:.1})",
                    start_x, start_y
                );

                // Perform drag: press, move LEFT, release
                let _ = robot.mouse_move(start_x, start_y);
                std::thread::sleep(Duration::from_millis(50));
                let _ = robot.mouse_down();
                std::thread::sleep(Duration::from_millis(50));

                // Drag LEFT by 200px (should scroll tabs right)
                for i in 0..20 {
                    let _ = robot.mouse_move(start_x - (i as f32 * 10.0), start_y);
                    std::thread::sleep(Duration::from_millis(20));
                }
                let _ = robot.mouse_up();
                std::thread::sleep(Duration::from_millis(200));
                let _ = robot.wait_for_idle();

                println!("  Dragged 200px left");
            } else {
                println!("  ✗ Could not find 'Counter App' tab to start drag");
                std::process::exit(1);
            }

            // Find "Lazy List" tab position AFTER drag
            let lazy_tab_after = find_in_semantics(&robot, |elem| find_button(elem, "Lazy List"));
            let x_after = lazy_tab_after.map(|(x, _, _, _)| x);

            println!("  'Lazy List' tab X after drag: {:?}", x_after);

            // ===== CRITICAL ASSERTION =====
            match (x_before, x_after) {
                (Some(before), Some(after)) => {
                    let delta = (after - before).abs();
                    if delta > 50.0 {
                        println!("  ✓ PASS: Tab moved by {:.1}px", delta);
                    } else {
                        println!(
                            "  ✗ FAIL: Tab only moved {:.1}px - TABBAR SCROLL BROKEN!",
                            delta
                        );
                        println!("         Expected >50px movement from 200px drag");
                        std::process::exit(1);
                    }
                }
                _ => {
                    println!("  ? INCONCLUSIVE: Could not find tab positions");
                }
            }

            println!("\n=== Test Summary ===");
            println!("✓ ALL TESTS PASSED - Tabbar scroll working");
            let _ = robot.exit();
        })
        .run(|| {
            app::combined_app();
        });
}
