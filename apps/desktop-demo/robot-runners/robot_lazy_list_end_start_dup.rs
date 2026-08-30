mod robot_launch;

use std::time::Duration;

use cranpose_testing::{
    count_text_in_tree, find_button_in_semantics, find_element_by_text_exact,
    print_semantics_with_bounds,
};
use desktop_app::app;

fn main() {
    env_logger::init();
    println!("=== LazyList End/Start Duplication Test ===");

    robot_launch::launch("LazyList End/Start Dup Test", 1200, 800)
        .with_test_driver(|robot| {
            println!("✓ App launched");
            std::thread::sleep(Duration::from_millis(500));

            let click_button = |name: &str| -> bool {
                if let Some((x, y, w, h)) = find_button_in_semantics(&robot, name) {
                    println!("  Found button '{}' at ({:.1}, {:.1})", name, x, y);
                    robot.click(x + w / 2.0, y + h / 2.0).ok();
                    std::thread::sleep(Duration::from_millis(200));
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
            std::thread::sleep(Duration::from_millis(500));

            println!("\n--- Step 2: Jump to end then start ---");
            if !click_button("End") {
                println!("FATAL: Could not find 'End' button");
                robot.exit().ok();
                std::process::exit(1);
            }
            std::thread::sleep(Duration::from_millis(500));
            if !click_button("Start") {
                println!("FATAL: Could not find 'Start' button");
                robot.exit().ok();
                std::process::exit(1);
            }
            std::thread::sleep(Duration::from_millis(500));

            println!("\n--- Step 3: Validate Item #0 height label duplication ---");
            let semantics = robot.get_semantics().ok();
            let Some(elements) = semantics.as_deref() else {
                println!("  ✗ Failed to fetch semantics");
                robot.exit().ok();
                std::process::exit(1);
            };

            let Some(item_row) = find_element_by_text_exact(elements, "ItemRow #0") else {
                println!("  ✗ ItemRow #0 semantics not found");
                print_semantics_with_bounds(elements, 0);
                let _ = robot.send_key("d");
                robot.exit().ok();
                std::process::exit(1);
            };

            print_semantics_with_bounds(std::slice::from_ref(item_row), 1);
            let height_label = "h: 48px";
            let height_count = count_text_in_tree(std::slice::from_ref(item_row), height_label);
            println!(
                "  Found '{}' occurrences under ItemRow #0: {}",
                height_label, height_count
            );

            if height_count != 1 {
                println!("  ✗ Height label duplicated (expected 1)");
                print_semantics_with_bounds(elements, 0);
                let _ = robot.send_key("d");
                robot.exit().ok();
                std::process::exit(1);
            }

            println!("✓ ItemRow #0 height label is not duplicated");
            println!("\n=== LazyList End/Start Duplication Test Complete ===");
            robot.exit().ok();
        })
        .run(app::combined_app);
}
