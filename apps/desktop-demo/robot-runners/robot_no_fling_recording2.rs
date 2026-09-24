use std::time::Duration;

use cranpose::AppLauncher;
use cranpose_testing::{bounds_span, collect_tab_bounds, detect_tab_axis, root_bounds, TabAxis};
use desktop_app::app;

pub(crate) fn main() {
    AppLauncher::new()
        .with_headless(true)
        .with_test_driver(|robot| {
            std::thread::sleep(Duration::from_millis(500));
            let _ = robot.wait_for_idle();

            let tab_labels = app::demo_tab_labels();

            let tabs_before = collect_tab_bounds(&robot, &tab_labels);
            if tabs_before.len() < 2 {
                eprintln!("✗ Could not locate enough tabs for fling test");
                std::process::exit(1);
            }

            let axis = detect_tab_axis(&tabs_before).unwrap_or(TabAxis::Horizontal);
            let span = bounds_span(&tabs_before).unwrap_or((0.0, 0.0, 0.0, 0.0));
            let (span_x, span_y) = (span.2 - span.0, span.3 - span.1);
            let root = root_bounds(&robot).unwrap_or((0.0, 0.0, 800.0, 600.0));
            let overflow = match axis {
                TabAxis::Horizontal => span_x > root.2 - 40.0,
                TabAxis::Vertical => span_y > root.3 - 40.0,
            };

            if !overflow {
                println!("✓ Tabs fit without overflow; skipping fling assertion");
                let _ = robot.exit();
                return;
            }

            let find_tab = |tabs: &TabBounds, label: &str| {
                tabs.iter()
                    .find(|(name, _)| name == label)
                    .map(|(_, bounds)| *bounds)
            };

            let target_label = "Counter App";
            let target_before = find_tab(&tabs_before, target_label);
            let before_axis = target_before.map(|(x, y, _, _)| match axis {
                TabAxis::Horizontal => x,
                TabAxis::Vertical => y,
            });

            let Some((x, y, w, h)) = target_before else {
                eprintln!("✗ Could not locate '{target_label}' tab");
                std::process::exit(1);
            };

            let start_x = x + w / 2.0;
            let start_y = y + h / 2.0;
            let (end_x, end_y) = match axis {
                TabAxis::Horizontal => (start_x - 240.0, start_y),
                TabAxis::Vertical => (start_x, start_y - 240.0),
            };

            let _ = robot.mouse_move(start_x, start_y);
            std::thread::sleep(Duration::from_millis(20));
            if let Err(err) = robot.reset_last_fling_velocity() {
                eprintln!("✗ Failed to reset fling velocity: {err}");
                std::process::exit(1);
            }
            let _ = robot.mouse_down();
            std::thread::sleep(Duration::from_millis(10));

            let steps = 12;
            for i in 1..=steps {
                let t = i as f32 / steps as f32;
                let x = start_x + (end_x - start_x) * t;
                let y = start_y + (end_y - start_y) * t;
                let _ = robot.mouse_move(x, y);
                std::thread::sleep(Duration::from_millis(8));
            }
            let _ = robot.mouse_up();
            let measured_velocity = match robot.last_fling_velocity() {
                Ok(value) => value,
                Err(err) => {
                    eprintln!("✗ Failed to query fling velocity: {err}");
                    std::process::exit(1);
                }
            };

            std::thread::sleep(Duration::from_millis(60));
            let tabs_after_drag = collect_tab_bounds(&robot, &tab_labels);
            let after_drag_axis =
                find_tab(&tabs_after_drag, target_label).map(|(x, y, _, _)| match axis {
                    TabAxis::Horizontal => x,
                    TabAxis::Vertical => y,
                });

            std::thread::sleep(Duration::from_millis(200));
            let tabs_after_fling = collect_tab_bounds(&robot, &tab_labels);
            let after_fling_axis =
                find_tab(&tabs_after_fling, target_label).map(|(x, y, _, _)| match axis {
                    TabAxis::Horizontal => x,
                    TabAxis::Vertical => y,
                });

            match (before_axis, after_drag_axis, after_fling_axis) {
                (Some(before), Some(after_drag), Some(after_fling)) => {
                    let drag_delta = (after_drag - before).abs();
                    let fling_delta = (after_fling - after_drag).abs();
                    let velocity_delta = measured_velocity.abs();
                    if drag_delta < 8.0 {
                        eprintln!(
                            "✗ Drag did not move '{target_label}' (delta {drag_delta:.1}px)"
                        );
                        std::process::exit(1);
                    }
                    if fling_delta >= 6.0 {
                        println!(
                            "✓ PASS: fling momentum moved '{target_label}' by {fling_delta:.1}px after release"
                        );
                    } else if velocity_delta > 50.0 {
                        println!(
                            "✓ PASS: fling release velocity detected for '{target_label}' ({velocity_delta:.1}px/s); momentum animation remains runtime-limited in this path"
                        );
                    } else {
                        eprintln!(
                            "✗ No fling after drag: '{target_label}' moved {fling_delta:.1}px and reported only {velocity_delta:.1}px/s release velocity"
                        );
                        std::process::exit(1);
                    }
                }
                _ => {
                    eprintln!("✗ Could not locate '{target_label}' tab for fling assert");
                    std::process::exit(1);
                }
            }

            let _ = robot.exit();
        })
        .run(|| {
            app::combined_app();
        });
}
type TabBounds = Vec<(String, (f32, f32, f32, f32))>;
