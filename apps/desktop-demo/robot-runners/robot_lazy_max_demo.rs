use std::time::Duration;

use cranpose::AppLauncher;
use cranpose_testing::{
    find_button_in_semantics, find_text_by_prefix_in_semantics, find_text_in_semantics,
};
use desktop_app::app::lazy_list::lazy_list_example;

fn main() {
    println!("=== LazyColumn usize::MAX Demo Test (ACTUAL APP) ===\n");

    AppLauncher::new()
        .with_title("LazyMax Test")
        .with_size(800, 600)
        .with_headless(true)
        .with_test_driver(|robot| {
            use std::time::Instant;

            std::thread::sleep(Duration::from_millis(500));
            println!("✓ App launched");

            let find_text = |text: &str| find_text_in_semantics(&robot, text);

            println!("\n=== PHASE 1: Initial State ===");

            if find_text("Lazy List Demo").is_some() {
                println!("  ✓ Header found: 'Lazy List Demo'");
            } else {
                println!("  ✗ Header NOT found!");
            }

            if find_text_by_prefix_in_semantics(&robot, "Virtualized list with").is_some() {
                println!("  ✓ Initial item count text present");
            }

            println!("\n=== PHASE 2: Click 'Set usize::MAX' ===");

            if let Some((x, y, w, h)) = find_button_in_semantics(&robot, "Set usize::MAX") {
                println!("  Found button at ({:.0}, {:.0})", x + w / 2.0, y + h / 2.0);
                robot.click(x + w / 2.0, y + h / 2.0).ok();
                std::thread::sleep(Duration::from_millis(400));
                println!("  ✓ Clicked 'Set usize::MAX'");

                if find_text("Lazy List Demo").is_some() {
                    println!("  ✓ App still responsive after setting usize::MAX");
                } else {
                    println!("  ✗ App may have crashed!");
                }
            } else if let Some((x, y, w, h)) = find_text("Set usize::MAX") {
                println!(
                    "  Found button text at ({:.0}, {:.0})",
                    x + w / 2.0,
                    y + h / 2.0
                );
                robot.click(x + w / 2.0, y + h / 2.0).ok();
                std::thread::sleep(Duration::from_millis(400));
                println!("  ✓ Clicked 'Set usize::MAX'");
            } else {
                println!("  ⚠️ 'Set usize::MAX' button not found");
            }

            println!("\n=== PHASE 3: Click 'Jump to Middle' ===");

            if let Some((x, y, w, h)) = find_button_in_semantics(&robot, "Jump to Middle") {
                println!("  Found button at ({:.0}, {:.0})", x + w / 2.0, y + h / 2.0);
                let jump_start = Instant::now();
                robot.click(x + w / 2.0, y + h / 2.0).ok();
                std::thread::sleep(Duration::from_millis(400));
                let jump_time = jump_start.elapsed();
                println!("  ✓ Clicked 'Jump to Middle' ({}ms)", jump_time.as_millis());

                if find_text("Lazy List Demo").is_some() {
                    println!("  ✓ App still responsive after jumping to middle");
                } else {
                    println!("  ✗ App may have crashed!");
                }
            } else if let Some((x, y, w, h)) = find_text("Jump to Middle") {
                println!(
                    "  Found button text at ({:.0}, {:.0})",
                    x + w / 2.0,
                    y + h / 2.0
                );
                let jump_start = Instant::now();
                robot.click(x + w / 2.0, y + h / 2.0).ok();
                std::thread::sleep(Duration::from_millis(400));
                let jump_time = jump_start.elapsed();
                println!("  ✓ Clicked 'Jump to Middle' ({}ms)", jump_time.as_millis());
            } else {
                println!("  ⚠️ 'Jump to Middle' button not found");
            }

            println!("\n=== SUMMARY ===");
            let success = find_text("Lazy List Demo").is_some();

            if success {
                println!("✓ usize::MAX demo test PASSED - no crashes");
            } else {
                println!("✗ Test FAILED - app crashed");
            }

            println!("\n=== Test Complete ===");
            robot.exit().ok();
        })
        .run(lazy_list_example);
}
