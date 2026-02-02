use cranpose::{AppLauncher, Robot};
use cranpose_testing::{find_in_semantics, find_text_exact};
use desktop_app::app;
use std::time::Duration;

fn wait_for_content(robot: &Robot, expected: &str, attempts: usize, delay: Duration) -> bool {
    for _ in 0..attempts {
        if robot.validate_content(expected).is_ok() {
            return true;
        }
        std::thread::sleep(delay);
    }
    false
}

fn main() {
    println!("Launching app with robot control for Scroll Bug Reproduction...");

    AppLauncher::new()
        .with_title("Scroll Bug Reproduction")
        .with_size(1024, 768)
        .with_headless(true)
        .with_test_driver(|robot| {
            println!("App launched! Starting test...");
            std::thread::sleep(Duration::from_secs(1));

            // 1. Reveal "Scroll Repro" tab (it's near the end)
            println!("Dragging tab bar to find Scroll Repro tab...");
            // Drag left (scroll right)
            robot.mouse_move(800.0, 50.0).unwrap();
            robot.mouse_down().unwrap();
            robot.mouse_move(200.0, 50.0).unwrap();
            robot.mouse_up().unwrap();
            std::thread::sleep(Duration::from_millis(500));

            // 2. Click "Scroll Repro" tab
            println!("Clicking Scroll Repro tab via text...");
            if let Err(e) = robot.click_by_text("Scroll Repro") {
                println!("Failed to click by text: {}, trying blind click...", e);
                robot.click(800.0, 50.0).expect("Failed to click tab");
            }
            std::thread::sleep(Duration::from_millis(1000));

            // 3. Verify we are on the tab
            // Check for unique content "Fake Title"
            println!("Waiting for Fake Title...");
            if wait_for_content(&robot, "Fake Title", 10, Duration::from_millis(200)) {
                println!("Scroll Repro tab active (Content found).");
            } else {
                panic!("Failed to switch to Scroll Repro tab (Fake Title not found)");
            }

            // 4. Scroll down
            // User: "scroll after item 4 jumps back to item 2"
            // Items are ~70px. 4 items ~280px.
            // Drag up 400px.
            println!("Scrolling down past item 4...");
            robot.mouse_move(512.0, 600.0).unwrap();
            robot.mouse_down().unwrap();
            // Drag slowly? Or fast? User didn't specify, but fling might affect it.
            // Let's drag consistently.
            // Drag to y=200
            let steps = 10;
            for i in 1..=steps {
                let t = i as f32 / steps as f32;
                let y = 600.0 + (200.0 - 600.0) * t;
                robot.mouse_move(512.0, y).unwrap();
                std::thread::sleep(Duration::from_millis(16)); // 60fps simulation
            }
            robot.mouse_up().unwrap();

            // Wait for momentum/settling
            println!("Waiting for scroll to settle...");
            std::thread::sleep(Duration::from_secs(1));

            // 5. Check visible items
            println!("Checking visible items...");

            let viewport_bounds =
                find_in_semantics(&robot, |elem| find_text_exact(elem, "LazyListViewport"))
                    .map(|(x, y, w, h)| (x, y, w, h));

            let is_visible = |bounds: (f32, f32, f32, f32)| -> bool {
                if let Some((vx, vy, vw, vh)) = viewport_bounds {
                    let (x, y, w, h) = bounds;
                    let x_overlap = x < vx + vw && x + w > vx;
                    let y_overlap = y < vy + vh && y + h > vy;
                    x_overlap && y_overlap
                } else {
                    true
                }
            };

            // We want to find the top-most visible rank by exact text match.
            let mut visible_ranks = Vec::new();
            for i in 1..=20 {
                let label = format!("{}.", i);
                if let Some(bounds) =
                    find_in_semantics(&robot, |elem| find_text_exact(elem, &label))
                {
                    if is_visible(bounds) {
                        visible_ranks.push((i, bounds.1));
                    }
                }
            }

            visible_ranks
                .sort_by(|a, b| a.1.partial_cmp(&b.1).unwrap_or(std::cmp::Ordering::Equal));

            println!(
                "Visible ranks (top to bottom): {:?}",
                visible_ranks
                    .iter()
                    .map(|(rank, _)| *rank)
                    .collect::<Vec<_>>()
            );

            if let Some((first_rank, _)) = visible_ranks.first() {
                println!("Top-most visible item rank: {}", first_rank);

                // If we scrolled past item 4 (4 * ~70px = 280px), we expect item 5 or 6 at top.
                // If we see item 2 or 3, bug is reproduced.
                if *first_rank <= 3 {
                    println!(
                        "BUG REPRODUCED: Scrolled down 400px but item {} is at top!",
                        first_rank
                    );
                    panic!("BUG REPRODUCED: Scroll jumped back to item {}", first_rank);
                } else {
                    println!("Scroll appears correct. Top item: {}", first_rank);
                }
            } else {
                println!("No visible items found! Dumping semantic tree:");
                if let Ok(semantics) = robot.get_semantics() {
                    Robot::print_semantics(&semantics, 0);
                }
            }

            robot.exit().expect("Failed to exit");
        })
        .run(|| {
            app::combined_app();
        });
}
