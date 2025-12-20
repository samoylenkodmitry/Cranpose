//! Robot test for tab scroll after click bug regression
//!
//! This test validates:
//! 1. Clicking a tab button SHOULD NOT cause the tab row to scroll
//! 2. Mouse move after click release SHOULD NOT scroll the tab row
//!
//! Run with:
//! ```bash
//! cargo run --package desktop-app --example robot_tab_scroll --features robot-app
//! ```

use compose_app::AppLauncher;
use compose_testing::{find_button, find_in_semantics};
use desktop_app::app;
use std::time::Duration;

fn main() {
    env_logger::init();
    println!("=== Robot Tab Scroll Test ===");
    println!("Testing that clicking tabs doesn't cause scroll following cursor\n");

    AppLauncher::new()
        .with_title("Robot Tab Scroll Test")
        .with_size(800, 600)
        .with_test_driver(|robot| {
            // Timeout after 30 seconds
            std::thread::spawn(|| {
                std::thread::sleep(Duration::from_secs(30));
                println!("✗ Test timed out after 30 seconds");
                std::process::exit(1);
            });

            println!("✓ App launched\n");
            std::thread::sleep(Duration::from_millis(500));

            match robot.wait_for_idle() {
                Ok(_) => println!("✓ App ready\n"),
                Err(e) => println!("Note: {}\n", e),
            }

            let mut all_passed = true;

            // =========================================================
            // Test: Click "Web Fetch" tab then move cursor
            // The bug: tab row scrolls following cursor after click
            // =========================================================
            println!("--- Test: Click 'Web Fetch' Tab Then Move Cursor ---");
            
            // Record reference tab position BEFORE any interaction
            let ref_tab_before = find_in_semantics(&robot, |elem| find_button(elem, "Modifiers Showcase"));
            let ref_x_before = ref_tab_before.map(|(x, _, _, _)| x).unwrap_or(0.0);
            println!("  Reference tab ('Modifiers Showcase') initial x={:.1}", ref_x_before);
            
            let web_fetch_tab = find_in_semantics(&robot, |elem| find_button(elem, "Web Fetch"));
            if let Some((x, y, w, h)) = web_fetch_tab {
                let cx = x + w/2.0;
                let cy = y + h/2.0;
                println!("  Found 'Web Fetch' tab at center ({:.1}, {:.1})", cx, cy);
                
                // Click the tab (down + up quickly)
                let _ = robot.mouse_move(cx, cy);
                std::thread::sleep(Duration::from_millis(50));
                let _ = robot.mouse_down();
                std::thread::sleep(Duration::from_millis(50));
                let _ = robot.mouse_up();
                std::thread::sleep(Duration::from_millis(100));
                let _ = robot.wait_for_idle();
                
                println!("  Clicked 'Web Fetch' tab");
                
                // Now move cursor to the RIGHT (without pressing any button)
                println!("  Moving cursor 150px right (no button pressed)...");
                for i in 0..15 {
                    let _ = robot.mouse_move(cx + (i as f32 * 10.0), cy);
                    std::thread::sleep(Duration::from_millis(30));
                }
                let _ = robot.wait_for_idle();
                std::thread::sleep(Duration::from_millis(200));
                
                // Check if reference tab moved (it should NOT)
                let ref_tab_after = find_in_semantics(&robot, |elem| find_button(elem, "Modifiers Showcase"));
                let ref_x_after = ref_tab_after.map(|(x, _, _, _)| x).unwrap_or(0.0);
                
                let scroll_delta = (ref_x_after - ref_x_before).abs();
                println!("  Reference tab after: x={:.1}, delta={:.1}px", ref_x_after, scroll_delta);
                
                if scroll_delta > 5.0 {
                    println!("  ✗ FAIL: Tab row scrolled by {:.1}px after click + cursor move!", scroll_delta);
                    println!("         BUG: Scroll following cursor after mouse up");
                    all_passed = false;
                } else {
                    println!("  ✓ PASS: Tab row did NOT scroll after click + cursor move");
                }
            } else {
                println!("  Could not find 'Web Fetch' tab, trying 'Counter App'");
                
                // Fallback to Counter App tab
                let counter_tab = find_in_semantics(&robot, |elem| find_button(elem, "Counter App"));
                if let Some((x, y, w, h)) = counter_tab {
                    let cx = x + w/2.0;
                    let cy = y + h/2.0;
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
                    for i in 0..15 {
                        let _ = robot.mouse_move(cx + (i as f32 * 10.0), cy);
                        std::thread::sleep(Duration::from_millis(30));
                    }
                    let _ = robot.wait_for_idle();
                    std::thread::sleep(Duration::from_millis(200));
                    
                    let ref_tab_after = find_in_semantics(&robot, |elem| find_button(elem, "Modifiers Showcase"));
                    let ref_x_after = ref_tab_after.map(|(x, _, _, _)| x).unwrap_or(0.0);
                    
                    let scroll_delta = (ref_x_after - ref_x_before).abs();
                    println!("  Reference tab after: x={:.1}, delta={:.1}px", ref_x_after, scroll_delta);
                    
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
