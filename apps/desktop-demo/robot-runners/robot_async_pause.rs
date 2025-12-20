//! Robot test for Async Runtime pause button clicks.
//!
//! This reproduces a bug where button clicks stop working after switching tabs.
//! The test:
//! 1. Switches to Async Runtime tab
//! 2. Clicks the "Pause Animation" button
//! 3. Verifies the button text changed to "Resume Animation"

use compose_app::AppLauncher;
use compose_testing::find_text_center;
use desktop_app::app;
use std::time::Duration;

fn main() {
    env_logger::init();
    println!("=== Robot Async Pause Button Test ===");
    println!("Testing if pause button works after switching to Async Runtime tab");

    AppLauncher::new()
        .with_title("Robot Async Pause Button Test")
        .with_size(800, 600)
        .with_test_driver(|robot| {
            // Timeout after 10 seconds
            std::thread::spawn(|| {
                std::thread::sleep(Duration::from_secs(10));
                println!("✗ Test TIMEOUT");
                std::process::exit(1);
            });

            println!("\n✓ App launched");
            std::thread::sleep(Duration::from_millis(500));
            let _ = robot.wait_for_idle();

            // 1. Switch to Async Runtime tab
            println!("\n--- Step 1: Switch to Async Runtime Tab ---");
            
            let semantics = robot.get_semantics().unwrap();
            let async_tab_pos = semantics.iter().find_map(|root| find_text_center(root, "Async Runtime"));

            if let Some((x, y)) = async_tab_pos {
                println!("Found Async Runtime tab at ({:.1}, {:.1})", x, y);
                let _ = robot.mouse_move(x, y);
                let _ = robot.mouse_down();
                std::thread::sleep(Duration::from_millis(50));
                let _ = robot.mouse_up();
                println!("✓ Clicked Async Runtime tab");
                
                // Wait for tab switch and animation to start
                std::thread::sleep(Duration::from_millis(500));
            } else {
                println!("✗ Failed to find Async Runtime tab");
                let _ = robot.exit();
            }

            // Verify we are on Async Runtime tab
            let semantics = robot.get_semantics().unwrap();
            let on_async_tab = semantics.iter().any(|root| 
                find_text_center(root, "Async Runtime Demo").is_some()
            );
            
            if on_async_tab {
                println!("✓ Verified we are on Async Runtime tab");
            } else {
                println!("✗ Failed to verify Async Runtime tab content");
                let _ = robot.exit();
            }

            // 2. Click the pause button
            println!("\n--- Step 2: Click Pause Animation Button ---");
            
            // Look for the pause button - could be "Pause animation" or "Resume animation"
            let semantics = robot.get_semantics().unwrap();
            let pause_pos = semantics.iter().find_map(|root| find_text_center(root, "Pause animation"));
            
            if let Some((x, y)) = pause_pos {
                println!("Found Pause animation button at ({:.1}, {:.1})", x, y);
                let _ = robot.mouse_move(x, y);
                std::thread::sleep(Duration::from_millis(50));
                let _ = robot.mouse_down();
                std::thread::sleep(Duration::from_millis(50));
                let _ = robot.mouse_up();
                println!("✓ Clicked Pause animation button");
                
                // Wait for state change
                std::thread::sleep(Duration::from_millis(300));
            } else {
                println!("✗ Failed to find Pause animation button");
                // Print what we found for debugging
                println!("Available text elements:");
                for root in semantics.iter() {
                    print_all_texts(root, 0);
                }
                let _ = robot.exit();
            }

            // 3. Verify the button text changed
            println!("\n--- Step 3: Verify Button State Changed ---");
            
            let semantics = robot.get_semantics().unwrap();
            let has_resume = semantics.iter().any(|root| 
                find_text_center(root, "Resume animation").is_some()
            );
            
            if has_resume {
                println!("✓ Button text changed to 'Resume animation'");
                println!("\n=== Test Summary ===");
                println!("✓ ALL TESTS PASSED");
                let _ = robot.exit();
            } else {
                // Check if pause button still shows "Pause Animation"
                let still_pause = semantics.iter().any(|root| 
                    find_text_center(root, "Pause animation").is_some()
                );
                
                if still_pause {
                    println!("✗ BUG REPRODUCED: Button still shows 'Pause animation'");
                    println!("   The click was not registered!");
                } else {
                    println!("✗ Could not find either button state");
                    println!("Available text elements:");
                    for root in semantics.iter() {
                        print_all_texts(root, 0);
                    }
                }
                let _ = robot.exit();
            }
        })
        .run(app::combined_app);
}

fn print_all_texts(element: &compose_app::SemanticElement, depth: usize) {
    let indent = "  ".repeat(depth);
    if let Some(text) = &element.text {
        println!("{}Text: '{}'", indent, text);
    }
    for child in &element.children {
        print_all_texts(child, depth + 1);
    }
}
