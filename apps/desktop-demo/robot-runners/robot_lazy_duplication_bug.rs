use compose_app::{AppLauncher, SemanticElement};
use compose_testing::find_button_in_semantics;
use desktop_app::app;
use std::time::Duration;

fn main() {
    env_logger::init();
    println!("=== Lazy Duplication Bug Reproduction Test ===");

    AppLauncher::new()
        .with_title("Lazy Duplication Test")
        .with_size(1200, 800)
        .with_test_driver(|robot| {
            println!("✓ App launched");
            std::thread::sleep(Duration::from_millis(500));
            let _ = robot.wait_for_idle();

            // Step 1: Navigate to LazyList tab
            println!("\n--- Step 1: Navigate to 'Lazy List' tab ---");
            if let Some((x, y, w, h)) = find_button_in_semantics(&robot, "Lazy List") {
                robot.click(x + w / 2.0, y + h / 2.0).ok();
                std::thread::sleep(Duration::from_millis(500));
            } else {
                println!("FATAL: 'Lazy List' tab not found");
                robot.exit().ok();
                std::process::exit(1);
            }
            let _ = robot.wait_for_idle();

            // Step 2: Click "Set usize::MAX" button
            println!("\n--- Step 2: Click 'Set usize::MAX' ---");
            if let Some((x, y, w, h)) = find_button_in_semantics(&robot, "Set usize::MAX") {
                robot.click(x + w / 2.0, y + h / 2.0).ok();
                std::thread::sleep(Duration::from_millis(500));
            } else {
                println!("FATAL: 'Set usize::MAX' button not found");
                robot.exit().ok();
                std::process::exit(1);
            }

            // Wait for update
            let _ = robot.wait_for_idle();

            // Step 3: Verify no duplication of "Visible:" and "Cached:"
            println!("\n--- Step 3: Verify UI indicators ---");

            if let Ok(elements) = robot.get_semantics() {
                // Debug: dump the semantics tree
                fn dump_tree(tree: &[SemanticElement], indent: usize) {
                    for node in tree {
                        let prefix = "  ".repeat(indent);
                        if let Some(text) = &node.text {
                            println!("{prefix}Text: {}", text);
                        } else {
                            println!("{prefix}Node (children={})", node.children.len());
                        }
                        dump_tree(&node.children, indent + 1);
                    }
                }
                println!("=== SEMANTICS TREE ===");
                dump_tree(&elements, 0);
                println!("=== END TREE ===\n");

                // Count occurrences
                let visible_count = count_text_starting_with(&elements, "Visible:");
                let cached_count = count_text_starting_with(&elements, "Cached:");

                println!("Found 'Visible:' count: {}", visible_count);
                println!("Found 'Cached:' count: {}", cached_count);

                if visible_count != 1 {
                    println!(
                        "❌ FAIL: Expected 1 'Visible:' indicator, found {}",
                        visible_count
                    );
                    robot.exit().ok();
                    std::process::exit(1);
                }

                if cached_count != 1 {
                    println!(
                        "❌ FAIL: Expected 1 'Cached:' indicator, found {}",
                        cached_count
                    );
                    robot.exit().ok();
                    std::process::exit(1);
                }

                println!("✅ PASS: No duplication found.");
            } else {
                println!("FATAL: Could not get semantics tree");
                robot.exit().ok();
                std::process::exit(1);
            }

            println!("\n=== Test Complete ===");
            robot.exit().ok();
        })
        .run(app::combined_app);
}

fn count_text_starting_with(tree: &[SemanticElement], prefix: &str) -> usize {
    let mut count = 0;
    for node in tree {
        if let Some(text) = &node.text {
            if text.starts_with(prefix) {
                count += 1;
            }
        }
        count += count_text_starting_with(&node.children, prefix);
    }
    count
}
