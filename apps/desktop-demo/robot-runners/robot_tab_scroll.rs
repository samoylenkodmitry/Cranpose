use std::time::Duration;

use cranpose::AppLauncher;
use cranpose_testing::find_button_in_semantics;
use desktop_app::app;

fn main() {
    env_logger::init();
    println!("=== Robot Tab Scroll Test ===");
    println!("Testing that clicking tabs doesn't cause scroll following cursor\n");

    AppLauncher::new()
        .with_title("Robot Tab Scroll Test")
        .with_size(800, 600)
        .with_headless(true)
        .with_test_driver(|robot| {
            println!("✓ App launched\n");
            std::thread::sleep(Duration::from_millis(500));

            match robot.wait_for_idle() {
                Ok(_) => println!("✓ App ready\n"),
                Err(e) => println!("Note: {}\n", e),
            }

            let mut all_passed = true;

            println!("--- Test: Click 'Web Fetch' Tab Then Move Cursor ---");

            let web_fetch_tab = find_button_in_semantics(&robot, "Web Fetch");

            let ref_tab_before = find_button_in_semantics(&robot, "Modifiers Showcase");
            let ref_x_before = ref_tab_before.map(|(x, _, _, _)| x).unwrap_or(0.0);
            println!(
                "  Reference tab ('Modifiers Showcase') initial x={:.1}",
                ref_x_before
            );
            if let Some((x, y, w, h)) = web_fetch_tab {
                let cx = x + w / 2.0;
                let cy = y + h / 2.0;
                println!("  Found 'Web Fetch' tab at center ({:.1}, {:.1})", cx, cy);

                let _ = robot.mouse_move(cx, cy);
                std::thread::sleep(Duration::from_millis(50));
                let _ = robot.mouse_down();
                std::thread::sleep(Duration::from_millis(50));
                let _ = robot.mouse_up();
                std::thread::sleep(Duration::from_millis(100));
                let _ = robot.wait_for_idle();

                println!("  Clicked 'Web Fetch' tab");

                println!("  Moving cursor 150px right (no button pressed)...");
                let _ = robot.mouse_move(cx + 150.0, cy);
                let _ = robot.wait_for_idle();
                std::thread::sleep(Duration::from_millis(200));

                let ref_tab_after = find_button_in_semantics(&robot, "Modifiers Showcase");
                let ref_x_after = ref_tab_after.map(|(x, _, _, _)| x).unwrap_or(0.0);

                let scroll_delta = (ref_x_after - ref_x_before).abs();
                println!(
                    "  Reference tab after: x={:.1}, delta={:.1}px",
                    ref_x_after, scroll_delta
                );

                if scroll_delta > 5.0 {
                    println!(
                        "  ✗ FAIL: Tab row scrolled by {:.1}px after click + cursor move!",
                        scroll_delta
                    );
                    println!("         BUG: Scroll following cursor after mouse up");
                    all_passed = false;
                } else {
                    println!("  ✓ PASS: Tab row did NOT scroll after click + cursor move");
                }
            } else {
                println!("  Could not find 'Web Fetch' tab, trying 'Counter App'");

                let counter_tab = find_button_in_semantics(&robot, "Counter App");
                if let Some((x, y, w, h)) = counter_tab {
                    let cx = x + w / 2.0;
                    let cy = y + h / 2.0;
                    println!("  Found 'Counter App' tab at center ({:.1}, {:.1})", cx, cy);

                    let _ = robot.mouse_move(cx, cy);
                    std::thread::sleep(Duration::from_millis(50));
                    let _ = robot.mouse_down();
                    std::thread::sleep(Duration::from_millis(50));
                    let _ = robot.mouse_up();
                    std::thread::sleep(Duration::from_millis(100));
                    let _ = robot.wait_for_idle();

                    println!("  Clicked 'Counter App' tab");

                    println!("  Moving cursor 150px right (no button pressed)...");
                    let _ = robot.mouse_move(cx + 150.0, cy);
                    let _ = robot.wait_for_idle();
                    std::thread::sleep(Duration::from_millis(200));

                    let ref_tab_after = find_button_in_semantics(&robot, "Modifiers Showcase");
                    let ref_x_after = ref_tab_after.map(|(x, _, _, _)| x).unwrap_or(0.0);

                    let scroll_delta = (ref_x_after - ref_x_before).abs();
                    println!(
                        "  Reference tab after: x={:.1}, delta={:.1}px",
                        ref_x_after, scroll_delta
                    );

                    if scroll_delta > 5.0 {
                        println!("  ✗ FAIL: Tab row scrolled by {:.1}px!", scroll_delta);
                        all_passed = false;
                    } else {
                        println!("  ✓ PASS: Tab row did NOT scroll");
                    }
                } else {
                    println!("  ✗ Could not find any tab buttons");
                    all_passed = false;
                }
            }

            println!("\n\n=== Test Summary ===");
            if all_passed {
                println!("✓ ALL TESTS PASSED");
                let _ = robot.exit();
            } else {
                println!("✗ SOME TESTS FAILED");
                let _ = robot.exit();
            }
        })
        .run(|| {
            app::combined_app();
        });
}
