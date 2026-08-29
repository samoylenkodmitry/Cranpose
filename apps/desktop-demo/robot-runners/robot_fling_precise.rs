use std::time::Duration;

use cranpose::{AppLauncher, Robot};
use cranpose_testing::{
    exit_with_timeout, find_bounds_by_text, find_button_in_semantics, find_in_semantics, find_text,
    visible_bounds_in_viewport,
};
use desktop_app::app;

fn main() {
    env_logger::init();
    println!("=== Precise Fling Test ===\n");

    const TEST_TIMEOUT_SECS: u64 = 120;

    AppLauncher::new()
        .with_title("Precise Fling Test")
        .with_size(800, 600)
        .with_headless(true)
        .with_test_driver(|robot| {
            std::thread::spawn(|| {
                std::thread::sleep(Duration::from_secs(TEST_TIMEOUT_SECS));
                eprintln!("✗ Test timed out after {} seconds", TEST_TIMEOUT_SECS);
                std::process::exit(1);
            });

            println!("✓ App launched");
            std::thread::sleep(Duration::from_millis(500));

            let _ = robot.wait_for_idle();
            println!("✓ App ready\n");

            let mut all_passed = true;
            let mut test_count = 0;
            let mut pass_count = 0;

            macro_rules! test {
                ($name:expr, $body:expr) => {{
                    test_count += 1;
                    print!("Test {}: {} ... ", test_count, $name);
                    let result: Result<(), String> = (|| $body)();
                    match result {
                        Ok(()) => {
                            pass_count += 1;
                            println!("PASS");
                        }
                        Err(e) => {
                            all_passed = false;
                            println!("FAIL: {}", e);
                        }
                    }
                }};
            }

            println!("--- Setup: Navigate to Lazy List Tab ---");
            if let Some((x, y, w, h)) = find_button_in_semantics(&robot, "Lazy List") {
                let cx = x + w / 2.0;
                let cy = y + h / 2.0;
                let _ = robot.mouse_move(cx, cy);
                std::thread::sleep(Duration::from_millis(50));
                let _ = robot.mouse_down();
                std::thread::sleep(Duration::from_millis(50));
                let _ = robot.mouse_up();
                std::thread::sleep(Duration::from_millis(500));
                println!("✓ Clicked Lazy List tab\n");
            } else {
                println!("✗ Could not find Lazy List tab - aborting");
                let _ = robot.exit();
                return;
            }

            let list_bounds = match find_bounds_by_text(&robot, "LazyListViewport") {
                Some(bounds) => bounds,
                None => {
                    println!("✗ Could not find LazyListViewport bounds - aborting");
                    let _ = robot.exit();
                    return;
                }
            };
            let visible_bounds = match visible_bounds_in_viewport(&robot, list_bounds, 12.0) {
                Some(bounds) => bounds,
                None => {
                    println!("✗ LazyListViewport is not visible in the viewport");
                    let _ = robot.exit();
                    return;
                }
            };

            let center_x = visible_bounds.0 + visible_bounds.2 * 0.5;
            let upper_y = visible_bounds.1 + visible_bounds.3 * 0.2;
            let lower_y = visible_bounds.1 + visible_bounds.3 * 0.8;
            let drag_distance = (lower_y - upper_y).max(80.0);

            fn find_item(robot: &Robot, item_text: &str) -> Option<(f32, f32)> {
                find_in_semantics(robot, |elem| find_text(elem, item_text))
                    .map(|(x, y, w, h)| (x + w / 2.0, y + h / 2.0))
            }

            fn find_any_item(robot: &Robot) -> Option<(f32, String)> {
                for i in 0..20 {
                    let item_text = format!("Item #{}", i);
                    if let Some((_, y)) = find_item(robot, &item_text) {
                        return Some((y, item_text));
                    }
                }
                None
            }

            test!("Initial state - Item #0 visible", {
                let item0 = find_item(&robot, "Item #0");
                if item0.is_none() {
                    return Err("Item #0 not found in initial state".to_string());
                }
                let (_, y) = item0.unwrap();

                let viewport =
                    find_in_semantics(&robot, |elem| find_text(elem, "LazyListViewport"));
                let Some((_vx, vy, _vw, vh)) = viewport else {
                    return Err("LazyListViewport not found in semantics".to_string());
                };
                if y < vy || y > (vy + vh) {
                    return Err(format!(
                        "Item 0 y={:.1} outside viewport bounds y=[{:.1}, {:.1}]",
                        y,
                        vy,
                        vy + vh
                    ));
                }
                Ok(())
            });

            test!("Simple drag scroll - position changes", {
                let before = find_item(&robot, "Item #0");
                if before.is_none() {
                    return Err("Item #0 not found before scroll".to_string());
                }
                let (_, before_y) = before.unwrap();

                let _ = robot.mouse_move(center_x, lower_y);
                std::thread::sleep(Duration::from_millis(50));
                let _ = robot.mouse_down();
                std::thread::sleep(Duration::from_millis(100));

                for i in 1..=10 {
                    let progress = i as f32 / 10.0;
                    let new_y = lower_y - (drag_distance * progress);
                    let _ = robot.mouse_move(center_x, new_y);
                    std::thread::sleep(Duration::from_millis(50));
                }

                let _ = robot.mouse_up();
                std::thread::sleep(Duration::from_millis(200));

                let after = find_item(&robot, "Item #0");
                match after {
                    Some((_, after_y)) => {
                        let delta = after_y - before_y;
                        if delta > -50.0 {
                            return Err(format!(
                                "Item 0 delta {} (expected < -50, before={}, after={})",
                                delta, before_y, after_y
                            ));
                        }
                        Ok(())
                    }
                    None => Ok(()),
                }
            });

            test!("Scroll back to top", {
                let _ = robot.mouse_move(center_x, upper_y);
                std::thread::sleep(Duration::from_millis(50));
                let _ = robot.mouse_down();
                std::thread::sleep(Duration::from_millis(50));

                for i in 1..=10 {
                    let progress = i as f32 / 10.0;
                    let new_y = upper_y + (drag_distance * progress);
                    let _ = robot.mouse_move(center_x, new_y);
                    std::thread::sleep(Duration::from_millis(30));
                }

                let _ = robot.mouse_up();
                std::thread::sleep(Duration::from_millis(500));

                let item0 = find_item(&robot, "Item #0");
                if item0.is_none() {
                    return Err("Item #0 not found after scroll back".to_string());
                }
                Ok(())
            });

            test!("Fast swipe triggers fling", {
                robot
                    .reset_last_fling_velocity()
                    .map_err(|err| format!("failed to reset fling velocity: {err}"))?;

                let before = find_item(&robot, "Item #0");
                let before_y = before.map(|(_, y)| y).unwrap_or(100.0);

                let _ = robot.mouse_move(center_x, lower_y);
                std::thread::sleep(Duration::from_millis(50));
                let _ = robot.mouse_down();
                std::thread::sleep(Duration::from_millis(20));

                for i in 1..=5 {
                    let progress = i as f32 / 5.0;
                    let new_y = lower_y - (drag_distance * progress);
                    let _ = robot.mouse_move(center_x, new_y);
                    std::thread::sleep(Duration::from_millis(10));
                }

                let _ = robot.mouse_up();

                std::thread::sleep(Duration::from_millis(300));
                let _ = robot.wait_for_idle();

                let after = find_item(&robot, "Item #0");
                match after {
                    Some((_, after_y)) => {
                        let total_movement = before_y - after_y;
                        let velocity = robot
                            .last_fling_velocity()
                            .map_err(|err| format!("failed to query fling velocity: {err}"))?;
                        if velocity.abs() < 50.0 {
                            return Err(format!(
                                "Fling velocity {:.1} < 50px/sec (expected fling momentum)",
                                velocity
                            ));
                        }
                        let min_expected = (drag_distance * 0.6).max(80.0);
                        if total_movement < min_expected {
                            return Err(format!(
                                "Total movement {} < {:.1}px (expected fling momentum)",
                                total_movement, min_expected
                            ));
                        }
                        eprintln!("  (Item 0 moved {} px total)", total_movement);
                        Ok(())
                    }
                    None => {
                        eprintln!("  (Item 0 scrolled off screen - good!)");
                        Ok(())
                    }
                }
            });

            test!("Repeated scrolls no jump-back", {
                let _ = robot.wait_for_idle();

                let scroll_end_y = (lower_y - 50.0).max(upper_y);

                let _ = robot.mouse_move(center_x, lower_y);
                std::thread::sleep(Duration::from_millis(30));
                let _ = robot.mouse_down();
                std::thread::sleep(Duration::from_millis(30));
                let _ = robot.mouse_move(center_x, scroll_end_y);
                std::thread::sleep(Duration::from_millis(30));
                let _ = robot.mouse_up();
                std::thread::sleep(Duration::from_millis(300));
                let _ = robot.wait_for_idle();

                let after_first = find_any_item(&robot);
                let after_first_y = after_first.as_ref().map(|(y, _)| *y).unwrap_or(300.0);

                let _ = robot.mouse_move(center_x, lower_y);
                std::thread::sleep(Duration::from_millis(30));
                let _ = robot.mouse_down();
                std::thread::sleep(Duration::from_millis(30));

                let during_second = find_any_item(&robot);
                let during_y = during_second.as_ref().map(|(y, _)| *y).unwrap_or(300.0);

                let _ = robot.mouse_up();
                std::thread::sleep(Duration::from_millis(100));

                let jump = (during_y - after_first_y).abs();
                if jump > 50.0 {
                    return Err(format!(
                        "Jump-back detected! After first scroll Y={}, on second down Y={}, jump={}",
                        after_first_y, during_y, jump
                    ));
                }
                eprintln!("  (No jump-back: delta={:.1}px)", jump);
                Ok(())
            });

            println!("\n=== Test Summary ===");
            println!("{} / {} tests passed", pass_count, test_count);

            if all_passed {
                println!("✓ ALL TESTS PASSED");
            } else {
                println!("✗ SOME TESTS FAILED");
            }

            std::thread::sleep(Duration::from_secs(1));
            exit_with_timeout(&robot, Duration::from_secs(2));
        })
        .run(|| {
            app::combined_app();
        });
}
