mod robot_launch;

use std::time::Duration;

use cranpose_testing::{find_button_in_semantics, find_text_by_prefix_in_semantics};
use desktop_app::app;

fn main() {
    env_logger::init();
    println!("=== Lazy Stats Validation Test ===");

    robot_launch::launch("LazyStats Test", 1200, 800)
        .with_test_driver(|robot| {
            println!("✓ App launched");
            std::thread::sleep(Duration::from_millis(500));
            let _ = robot.wait_for_idle();

            println!("\n--- Step 1: Navigate to 'Lazy List' tab ---");
            if let Some((x, y, w, h)) = find_button_in_semantics(&robot, "Lazy List") {
                println!("  Found 'Lazy List' tab at ({:.1}, {:.1})", x, y);
                robot.click(x + w / 2.0, y + h / 2.0).ok();
                std::thread::sleep(Duration::from_millis(500));
            } else {
                println!("FATAL: 'Lazy List' tab not found");
                robot.exit().ok();
                std::process::exit(1);
            }
            let _ = robot.wait_for_idle();

            println!("\n--- Step 2: Dump all text nodes ---");
            if let Ok(elements) = robot.get_semantics() {
                cranpose_testing::print_semantics_with_bounds(&elements, 0);
            }

            println!("\n--- Step 3: Check 'Visible:' stats ---");

            let visible_text = find_text_by_prefix_in_semantics(&robot, "Visible:");
            if let Some((x, y, _w, _h, text)) = visible_text {
                println!("  Found: '{}' at ({:.1}, {:.1})", text, x, y);

                if let Some(num_str) = text.strip_prefix("Visible:").map(|s| s.trim()) {
                    if let Ok(num) = num_str.parse::<usize>() {
                        if num > 0 {
                            println!("  ✓ PASS: Visible count is {} (non-zero)", num);
                        } else {
                            println!("  ✗ FAIL: Visible count is 0 - reactive stats not working!");
                            robot.exit().ok();
                            std::process::exit(1);
                        }
                    } else {
                        println!("  ⚠️ Could not parse number from '{}'", num_str);
                    }
                }
            } else {
                println!("  ✗ 'Visible:' text not found!");
                robot.exit().ok();
                std::process::exit(1);
            }

            println!("\n--- Step 5: Check 'Cached:' stats ---");
            if let Some((_, _, _, _, text)) = find_text_by_prefix_in_semantics(&robot, "Cached:") {
                println!("  Found: '{}'", text);
            } else {
                println!("  'Cached:' text not found");
            }

            println!("\n=== Test Complete ===");
            robot.exit().ok();
        })
        .run(app::combined_app);
}
