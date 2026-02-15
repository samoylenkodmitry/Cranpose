//! Robot runner for Shaders performance profiling.
//!
//! Steps:
//! 1. Open app
//! 2. Go to Shaders tab
//! 3. Drag "Blur" and "Glass" rects around
//! 4. Scroll the page up and down
//! 5. Repeat for profiling
//!
//! Run with:
//! ```bash
//! cargo run --package desktop-app --example robot_measure_shaders --features robot-app
//! ```

use cranpose::AppLauncher;
use cranpose_testing::{
    find_button_in_semantics, find_in_semantics, find_text_exact, print_semantics_with_bounds,
};
use cranpose_ui::Point;
use desktop_app::app;
use std::time::{Duration, Instant};

fn env_u64(key: &str, default: u64) -> u64 {
    std::env::var(key)
        .ok()
        .and_then(|v| v.parse::<u64>().ok())
        .unwrap_or(default)
}

fn env_bool(key: &str, default: bool) -> bool {
    std::env::var(key)
        .ok()
        .map(|v| matches!(v.to_lowercase().as_str(), "1" | "true" | "yes" | "on"))
        .unwrap_or(default)
}

fn main() {
    env_logger::init();
    println!("=== Shaders Performance Profiling Robot ===");

    // Configuration — driven by env vars so perf scripts can control behavior
    let headless = env_bool("CRANPOSE_HEADLESS", false);
    let duration_secs = env_u64("CRANPOSE_PERF_DURATION_SECS", 20);
    let duration = Duration::from_secs(duration_secs);
    println!("  headless={}, duration={}s", headless, duration_secs);

    let scroll_steps = 10;

    AppLauncher::new()
        .with_title("Shaders Profiling")
        .with_size(1200, 800)
        .with_headless(headless)
        .with_test_driver(move |robot| {
            std::thread::sleep(Duration::from_millis(1000));
            let _ = robot.wait_for_idle();

            let click_button = |name: &str| -> bool {
                if let Some((x, y, w, h)) = find_button_in_semantics(&robot, name) {
                    robot.click(x + w / 2.0, y + h / 2.0).ok();
                    std::thread::sleep(Duration::from_millis(500));
                    return true;
                }
                println!("  ✗ Button '{}' not found!", name);
                false
            };

            let find_center = |text: &str| -> Option<Point> {
                find_in_semantics(&robot, |elem| find_text_exact(elem, text)).map(|(x, y, w, h)| {
                    Point {
                        x: x + w / 2.0,
                        y: y + h / 2.0,
                    }
                })
            };

            // 1. Navigate to Shaders tab
            if !click_button("Shaders") {
                println!("FATAL: Could not find 'Shaders' tab button");
                // Print top-level semantics for debugging
                if let Ok(elements) = robot.get_semantics() {
                    println!("Top-level semantics:");
                    print_semantics_with_bounds(&elements, 0);
                }
                robot.exit().ok();
                std::process::exit(1);
            }
            println!("  ✓ Entered Shaders tab");

            // Wait for content
            let start_time = Instant::now();
            let mut loops = 0;

            while start_time.elapsed() < duration {
                loops += 1;
                println!(
                    "  Loop #{} (elapsed: {:.1}s)",
                    loops,
                    start_time.elapsed().as_secs_f32()
                );

                // 2. Drag Blurred Rect
                if let Some(center) = find_center("Blur") {
                    let p1 = center;
                    let p2 = Point {
                        x: p1.x + 100.0,
                        y: p1.y,
                    };

                    robot.drag(p1.x, p1.y, p2.x, p2.y).ok();
                    let _ = robot.wait_for_idle();
                } else {
                    println!("  ⚠ 'Blur' rect not found");
                }

                // 3. Drag Glass Rect
                if let Some(center) = find_center("Glass") {
                    let p1 = center;
                    let p2 = Point {
                        x: p1.x - 50.0,
                        y: p1.y + 50.0,
                    };
                    robot.drag(p1.x, p1.y, p2.x, p2.y).ok();
                    let _ = robot.wait_for_idle();
                } else {
                    println!("  ⚠ 'Glass' rect not found");
                }

                // 4. Scroll Page
                let window_w = 1200.0;
                let window_h = 800.0;
                let scroll_x = window_w / 2.0;

                // Scroll Down (drag up)
                for _ in 0..scroll_steps {
                    robot
                        .drag(scroll_x, window_h * 0.8, scroll_x, window_h * 0.2)
                        .ok();
                    std::thread::sleep(Duration::from_millis(100));
                }
                let _ = robot.wait_for_idle();

                // Scroll Up (drag down)
                for _ in 0..scroll_steps {
                    robot
                        .drag(scroll_x, window_h * 0.2, scroll_x, window_h * 0.8)
                        .ok();
                    std::thread::sleep(Duration::from_millis(100));
                }
                let _ = robot.wait_for_idle();
            }

            println!("=== Profiling Run Complete ===");
            robot.exit().expect("Failed to exit");
        })
        .run(|| {
            app::combined_app();
        });
}
