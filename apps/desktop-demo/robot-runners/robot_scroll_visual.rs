use std::time::Duration;

use cranpose::AppLauncher;
use cranpose_testing::{
    exit_with_timeout, find_button_in_semantics, find_text_by_prefix_in_semantics,
};
use desktop_app::app;

fn parse_first_index(text: &str) -> Option<usize> {
    let (_, value) = text.split_once(':')?;
    value.trim().parse::<usize>().ok()
}

fn read_first_index(robot: &cranpose::Robot) -> Option<usize> {
    let (_, _, _, _, text) = find_text_by_prefix_in_semantics(robot, "FirstIndex:")?;
    parse_first_index(&text)
}

fn main() {
    env_logger::init();
    println!("=== Robot Scroll Visual Test ===");
    println!("Verifying that scroll actually moves content visually\n");

    AppLauncher::new()
        .with_title("Robot Scroll Visual Test")
        .with_size(800, 600)
        .with_headless(true)
        .with_test_driver(|robot| {
            println!("✓ App launched\n");
            std::thread::sleep(Duration::from_millis(500));
            let _ = robot.wait_for_idle();

            let lazy_tab = find_button_in_semantics(&robot, "Lazy List");

            if let Some((x, y, w, h)) = lazy_tab {
                let cx = x + w / 2.0;
                let cy = y + h / 2.0;
                let _ = robot.mouse_move(cx, cy);
                std::thread::sleep(Duration::from_millis(50));
                let _ = robot.mouse_down();
                std::thread::sleep(Duration::from_millis(50));
                let _ = robot.mouse_up();
                std::thread::sleep(Duration::from_millis(200));
                let _ = robot.wait_for_idle();
                println!("  Switched to Lazy List tab");
            } else {
                println!("  ✗ Could not find Lazy List tab");
                std::process::exit(1);
            }

            std::thread::sleep(Duration::from_millis(300));
            let _ = robot.wait_for_idle();

            let mut before_index = None;
            for _ in 0..20 {
                if let Some(idx) = read_first_index(&robot) {
                    before_index = Some(idx);
                    break;
                }
                std::thread::sleep(Duration::from_millis(50));
            }

            let Some(before_index) = before_index else {
                println!("  ✗ Failed to read FirstIndex before scroll");
                std::process::exit(1);
            };

            println!("\n--- Test: Scroll should move content visually ---");
            println!("  First visible index before scroll: {}", before_index);

            println!("  Clicking 'Jump to Middle' to change scroll position...");
            let jump_button = find_button_in_semantics(&robot, "Jump to Middle");
            if let Some((x, y, w, h)) = jump_button {
                let _ = robot.mouse_move(x + w / 2.0, y + h / 2.0);
                std::thread::sleep(Duration::from_millis(50));
                let _ = robot.mouse_down();
                std::thread::sleep(Duration::from_millis(50));
                let _ = robot.mouse_up();
                std::thread::sleep(Duration::from_millis(250));
            } else {
                println!("  ✗ Could not find 'Jump to Middle' button");
                std::process::exit(1);
            }
            println!("  Scroll action complete, checking list index...");

            let mut after_index = None;
            for _ in 0..20 {
                if let Some(idx) = read_first_index(&robot) {
                    if idx != before_index {
                        after_index = Some(idx);
                        break;
                    }
                }
                std::thread::sleep(Duration::from_millis(50));
            }

            match after_index {
                Some(idx) if idx >= 40 => {
                    println!("  ✓ PASS: First visible index moved to {}", idx);
                }
                Some(idx) => {
                    println!(
                        "  ✗ FAIL: First visible index did not move far enough ({})",
                        idx
                    );
                    std::process::exit(1);
                }
                None => {
                    println!("  ✗ FAIL: First visible index did not change after scroll");
                    std::process::exit(1);
                }
            }

            println!("\n=== Test Summary ===");
            println!("✓ ALL TESTS PASSED - Scroll is working visually");
            exit_with_timeout(&robot, Duration::from_secs(5));
        })
        .run(|| {
            app::combined_app();
        });
}
