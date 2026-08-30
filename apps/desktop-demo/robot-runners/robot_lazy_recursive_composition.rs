mod robot_launch;

use std::time::Duration;

use cranpose_testing::{
    find_button_in_semantics, find_element_by_text_exact, find_in_semantics, find_text_exact,
};
use desktop_app::app;

fn main() {
    env_logger::init();
    println!("=== LazyList Recursive Composition Bug Test ===");

    robot_launch::launch("LazyList Recursive Composition Test", 1200, 800).with_test_driver(|robot| {
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

            println!("\n--- Step 2: Validate item children are nested, not separate ---");
            let semantics = robot.get_semantics().ok();
            let Some(elements) = semantics.as_deref() else {
                println!("  ✗ Failed to fetch semantics");
                robot.exit().ok();
                std::process::exit(1);
            };

            let Some(list_elem) = find_element_by_text_exact(elements, "LazyListViewport") else {
                println!("  ✗ LazyListViewport not found");
                robot.exit().ok();
                std::process::exit(1);
            };

            let row_bounds = find_in_semantics(&robot, |elem| find_text_exact(elem, "ItemRow #0"));
            let Some((row_x, row_y, row_w, row_h)) = row_bounds else {
                println!("  ✗ ItemRow #0 not found");
                robot.exit().ok();
                std::process::exit(1);
            };
            println!("  ItemRow #0 bounds: ({:.1}, {:.1}, {:.1}, {:.1})", row_x, row_y, row_w, row_h);

            let text_bounds = find_in_semantics(&robot, |elem| find_text_exact(elem, "Item #0"));
            let Some((text_x, text_y, _text_w, _text_h)) = text_bounds else {
                println!("  ✗ 'Item #0' text not found");
                robot.exit().ok();
                std::process::exit(1);
            };
            println!("  'Item #0' text at: ({:.1}, {:.1})", text_x, text_y);

            let text_inside_row = text_y >= row_y && text_y < row_y + row_h;
            if !text_inside_row {
                println!("  ✗ BUG DETECTED: 'Item #0' text is NOT inside ItemRow #0!");
                println!("    Text Y={:.1} should be between Row Y={:.1} and Y+H={:.1}", 
                         text_y, row_y, row_y + row_h);
                println!("  This indicates children are being placed as separate lazy list items.");
                robot.exit().ok();
                std::process::exit(1);
            }
            println!("  ✓ 'Item #0' text is correctly inside ItemRow #0");


            let direct_children_count = list_elem.children.len();
            println!("  LazyListViewport has {} direct children", direct_children_count);

            if direct_children_count > 50 {
                println!("  ✗ BUG: Too many direct children ({})!", direct_children_count);
                println!("    Expected ~10 root items, got {} - suggests nested children are placed separately",
                         direct_children_count);
                robot.exit().ok();
                std::process::exit(1);
            }
            println!("  ✓ Direct children count ({}) is reasonable", direct_children_count);

            println!("\n✓ No recursive composition bug detected");
            println!("=== LazyList Recursive Composition Test PASSED ===");
            robot.exit().ok();
        })
        .run(app::combined_app);
}
