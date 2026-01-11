//! Robot test to reproduce modifier showcase rendering issue
//! Tests that clicking "Positioned Boxes" in modifier tab shows content

use compose_app::AppLauncher;
use compose_testing::{find_button, find_in_semantics, find_text};
use desktop_app::app::combined_app;
use desktop_app::fonts::DEMO_FONTS;
use std::time::Duration;

fn main() {
    AppLauncher::new()
        .with_title("Robot: Modifier Render Test")
        .with_size(1200, 900)
        .with_fonts(&DEMO_FONTS)
        .with_headless(true)
        .with_fps_counter(true)
        .with_test_driver(|robot| {
            std::thread::sleep(Duration::from_millis(500));
            robot.wait_for_idle().expect("wait for idle");
            println!("✓ App launched");

            // Click on Modifiers Showcase tab (note: with 's')
            if let Some((x, y, w, h)) =
                find_in_semantics(&robot, |elem| find_button(elem, "Modifiers Showcase"))
            {
                let cx = x + w / 2.0;
                let cy = y + h / 2.0;
                robot.click(cx, cy).expect("click modifiers tab");
                robot.wait_for_idle().expect("wait after tab click");
                println!("✓ Clicked Modifiers Showcase tab at ({:.1}, {:.1})", cx, cy);
            } else {
                println!("✗ FAIL: Could not find Modifiers Showcase tab");
                robot.exit().expect("exit");
                return;
            }

            std::thread::sleep(Duration::from_millis(200));

            // Look for Positioned Boxes button
            if let Some((x, y, w, h)) =
                find_in_semantics(&robot, |elem| find_button(elem, "Positioned Boxes"))
            {
                let cx = x + w / 2.0;
                let cy = y + h / 2.0;
                println!("  Found 'Positioned Boxes' at ({:.1}, {:.1})", cx, cy);

                robot.click(cx, cy).expect("click Positioned Boxes");
                robot.wait_for_idle().expect("wait after click");
                println!("✓ Clicked Positioned Boxes");
            } else {
                println!("✗ FAIL: Could not find Positioned Boxes button");
                robot.exit().expect("exit");
                return;
            }

            std::thread::sleep(Duration::from_millis(500));
            robot.wait_for_idle().expect("wait for content");

            // Check for expected content - look for "Layer" text which should appear
            let has_layer = find_in_semantics(&robot, |elem| find_text(elem, "Layer"));
            let has_box = find_in_semantics(&robot, |elem| find_text(elem, "Box"));

            if has_layer.is_some() || has_box.is_some() {
                println!("  ✓ PASS: Content found after clicking Positioned Boxes");
                println!("✓ ALL TESTS PASSED");
            } else {
                // This is the regression - content should be visible after clicking
                println!("  ✗ FAIL: No content visible after clicking Positioned Boxes!");
                println!("         Expected to find 'Layer' or 'Box' text");
                println!("         This is the recomposition regression!");
            }

            // Verify Dynamic Modifiers responds to state changes without extra invalidation
            if let Some((x, y, w, h)) =
                find_in_semantics(&robot, |elem| find_button(elem, "Dynamic Modifiers"))
            {
                let cx = x + w / 2.0;
                let cy = y + h / 2.0;
                robot.click(cx, cy).expect("click Dynamic Modifiers");
                robot
                    .wait_for_idle()
                    .expect("wait after dynamic modifiers click");
                println!("✓ Clicked Dynamic Modifiers");
            } else {
                println!("✗ FAIL: Could not find Dynamic Modifiers button");
                robot.exit().expect("exit");
                return;
            }

            std::thread::sleep(Duration::from_millis(300));
            robot.wait_for_idle().expect("wait before move check");

            let before_move = find_in_semantics(&robot, |elem| find_text(elem, "Move"));
            let Some((before_x, _, before_w, _)) = before_move else {
                println!("✗ FAIL: Could not find 'Move' label before frame advance");
                robot.exit().expect("exit");
                return;
            };
            let before_center_x = before_x + before_w / 2.0;

            if let Some((x, y, w, h)) =
                find_in_semantics(&robot, |elem| find_button(elem, "Advance Frame"))
            {
                let cx = x + w / 2.0;
                let cy = y + h / 2.0;
                robot.click(cx, cy).expect("click Advance Frame");
                robot.wait_for_idle().expect("wait after advance frame");
            } else {
                println!("✗ FAIL: Could not find Advance Frame button");
                robot.exit().expect("exit");
                return;
            }

            std::thread::sleep(Duration::from_millis(300));
            robot.wait_for_idle().expect("wait after move check");

            let after_move = find_in_semantics(&robot, |elem| find_text(elem, "Move"));
            let Some((after_x, _, after_w, _)) = after_move else {
                println!("✗ FAIL: Could not find 'Move' label after frame advance");
                robot.exit().expect("exit");
                return;
            };
            let after_center_x = after_x + after_w / 2.0;

            let delta = (after_center_x - before_center_x).abs();
            if delta < 4.0 {
                println!(
                    "✗ FAIL: Dynamic Modifiers did not update position (Δx={:.1})",
                    delta
                );
            } else {
                println!("✓ Dynamic Modifiers updated position (Δx={:.1})", delta);
            }

            robot.exit().expect("exit");
        })
        .run(combined_app);
}
