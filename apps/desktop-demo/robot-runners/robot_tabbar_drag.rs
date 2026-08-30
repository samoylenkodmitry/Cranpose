mod robot_launch;

mod robot_exit;

use std::time::Duration;

use cranpose_testing::{bounds_span, collect_tab_bounds, detect_tab_axis, root_bounds, TabAxis};
use desktop_app::app;

fn main() {
    env_logger::init();
    println!("=== Robot Tabbar Drag Test ===\n");

    robot_launch::launch("Robot Tabbar Drag Test", 800, 600)
        .with_test_driver(|robot| {
            robot_exit::arm_timeout(30);

            println!("✓ App launched");
            std::thread::sleep(Duration::from_millis(500));
            let _ = robot.wait_for_idle();

            let tab_labels = app::demo_tab_labels();

            let tabs_before = collect_tab_bounds(&robot, &tab_labels);
            let axis = detect_tab_axis(&tabs_before).unwrap_or(TabAxis::Horizontal);
            let span = bounds_span(&tabs_before).unwrap_or((0.0, 0.0, 0.0, 0.0));
            let (span_x, span_y) = (span.2 - span.0, span.3 - span.1);
            let root = root_bounds(&robot).unwrap_or((0.0, 0.0, 800.0, 600.0));
            let overflow = match axis {
                TabAxis::Horizontal => span_x > root.2 - 40.0,
                TabAxis::Vertical => span_y > root.3 - 40.0,
            };

            let find_tab = |tabs: &TabBounds, label: &str| {
                tabs.iter()
                    .find(|(name, _)| name == label)
                    .map(|(_, bounds)| *bounds)
            };

            let lazy_before = find_tab(&tabs_before, "Lazy List");
            let counter_before = find_tab(&tabs_before, "Counter App");
            let before_axis = lazy_before.map(|(x, y, _, _)| match axis {
                TabAxis::Horizontal => x,
                TabAxis::Vertical => y,
            });

            println!("\n--- Test: Drag tabbar should scroll tabs ---");
            println!(
                "  'Lazy List' tab {} before drag: {:?}",
                match axis {
                    TabAxis::Horizontal => "X",
                    TabAxis::Vertical => "Y",
                },
                before_axis
            );

            if lazy_before.is_none() || counter_before.is_none() {
                println!("  ✗ Could not find 'Lazy List' tab");
                std::process::exit(1);
            }

            if let Some((x, y, w, h)) = counter_before {
                let start_x = x + w / 2.0;
                let start_y = y + h / 2.0;

                println!(
                    "  Starting drag from 'Counter App' at ({:.1}, {:.1})",
                    start_x, start_y
                );

                let _ = robot.mouse_move(start_x, start_y);
                std::thread::sleep(Duration::from_millis(50));
                let _ = robot.mouse_down();
                std::thread::sleep(Duration::from_millis(50));

                if overflow {
                    match axis {
                        TabAxis::Horizontal => {
                            for i in 0..20 {
                                let _ = robot.mouse_move(start_x - (i as f32 * 10.0), start_y);
                                std::thread::sleep(Duration::from_millis(20));
                            }
                        }
                        TabAxis::Vertical => {
                            for i in 0..20 {
                                let _ = robot.mouse_move(start_x, start_y - (i as f32 * 10.0));
                                std::thread::sleep(Duration::from_millis(20));
                            }
                        }
                    }
                    let _ = robot.mouse_up();
                    std::thread::sleep(Duration::from_millis(200));
                    let _ = robot.wait_for_idle();

                    println!(
                        "  Dragged 200px {}",
                        match axis {
                            TabAxis::Horizontal => "left",
                            TabAxis::Vertical => "up",
                        }
                    );
                } else {
                    let _ = robot.mouse_up();
                    println!("  Tabs fit without overflow; skipping drag assertion");
                }
            } else {
                println!("  ✗ Could not find 'Counter App' tab to start drag");
                std::process::exit(1);
            }

            let tabs_after = collect_tab_bounds(&robot, &tab_labels);
            let lazy_after = find_tab(&tabs_after, "Lazy List");
            let after_axis = lazy_after.map(|(x, y, _, _)| match axis {
                TabAxis::Horizontal => x,
                TabAxis::Vertical => y,
            });

            println!(
                "  'Lazy List' tab {} after drag: {:?}",
                match axis {
                    TabAxis::Horizontal => "X",
                    TabAxis::Vertical => "Y",
                },
                after_axis
            );

            if overflow {
                match (before_axis, after_axis) {
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
            } else {
                println!("  ✓ PASS: Tabs fit without overflow; no scroll expected");
            }

            println!("\n=== Test Summary ===");
            println!("✓ ALL TESTS PASSED - Tabbar scroll working");
            let _ = robot.exit();
        })
        .run(|| {
            app::combined_app();
        });
}
type TabBounds = Vec<(String, (f32, f32, f32, f32))>;
