//! Robot test for Positioned Boxes after visiting LazyList.
//!
//! Run with:
//! ```bash
//! cargo run --package desktop-app --example robot_positioned_boxes_after_lazy_list --features robot-app
//! ```

use compose_app::{AppLauncher, SemanticElement};
use compose_testing::find_button_in_semantics;
use desktop_app::app;
use std::time::Duration;

fn count_text_occurrences(elements: &[SemanticElement], text: &str) -> usize {
    let mut count = 0;
    for elem in elements {
        if elem.text.as_deref() == Some(text) {
            count += 1;
        }
        count += count_text_occurrences(&elem.children, text);
    }
    count
}

fn print_semantics_with_bounds(elements: &[SemanticElement], indent: usize) {
    for elem in elements {
        let prefix = "  ".repeat(indent);
        let text = elem.text.as_deref().unwrap_or("");
        println!(
            "{}role={} text=\"{}\" bounds=({:.1},{:.1},{:.1},{:.1}){}",
            prefix,
            elem.role,
            text,
            elem.bounds.x,
            elem.bounds.y,
            elem.bounds.width,
            elem.bounds.height,
            if elem.clickable { " [CLICKABLE]" } else { "" }
        );
        print_semantics_with_bounds(&elem.children, indent + 1);
    }
}

fn main() {
    env_logger::init();
    println!("=== Positioned Boxes After LazyList (dup check) ===");

    AppLauncher::new()
        .with_title("Positioned Boxes After LazyList")
        .with_size(1200, 800)
        .with_test_driver(|robot| {
            println!("✓ App launched");
            std::thread::sleep(Duration::from_millis(500));

            let click_button = |name: &str| -> bool {
                if let Some((x, y, w, h)) = find_button_in_semantics(&robot, name) {
                    println!("  Found button '{}' at ({:.1}, {:.1})", name, x, y);
                    robot.click(x + w / 2.0, y + h / 2.0).ok();
                    std::thread::sleep(Duration::from_millis(150));
                    true
                } else {
                    println!("  ✗ Button '{}' not found!", name);
                    false
                }
            };

            println!("\n--- Step 1: Navigate to 'Lazy List' tab ---");
            if !click_button("Lazy List") {
                println!("FATAL: Could not find 'Lazy List' tab button");
                robot.exit().ok();
                std::process::exit(1);
            }
            std::thread::sleep(Duration::from_millis(400));

            println!("\n--- Step 2: Navigate to 'Modifiers Showcase' tab ---");
            if !click_button("Modifiers Showcase") {
                println!("FATAL: Could not find 'Modifiers Showcase' tab button");
                robot.exit().ok();
                std::process::exit(1);
            }
            std::thread::sleep(Duration::from_millis(300));

            println!("\n--- Step 3: Select 'Positioned Boxes' showcase ---");
            if !click_button("Positioned Boxes") {
                println!("FATAL: Could not find 'Positioned Boxes' button");
                robot.exit().ok();
                std::process::exit(1);
            }
            std::thread::sleep(Duration::from_millis(500));
            let _ = robot.wait_for_idle();

            let semantics = robot.get_semantics().ok();
            let Some(elements) = semantics.as_deref() else {
                println!("  ✗ Failed to read semantics tree");
                robot.exit().ok();
                std::process::exit(1);
            };

            let header_text = "=== Positioned Boxes ===";
            let count = count_text_occurrences(elements, header_text);
            println!("  Found '{}' occurrences: {}", header_text, count);

            if count != 1 {
                println!("  ✗ Expected exactly 1 Positioned Boxes header");
                print_semantics_with_bounds(elements, 0);
                let _ = robot.send_key("d");
                robot.exit().ok();
                std::process::exit(1);
            }

            println!("✓ Positioned Boxes header count OK");
            robot.exit().ok();
        })
        .run(app::combined_app);
}
