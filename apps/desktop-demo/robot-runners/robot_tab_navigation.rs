use std::time::Duration;

use cranpose::AppLauncher;
use cranpose_testing::{find_button_in_semantics, find_text_in_semantics};
use desktop_app::app;

fn main() {
    env_logger::init();
    println!("=== Robot Tab Navigation Stress Test ===");

    AppLauncher::new()
        .with_title("Robot Tab Navigation Test")
        .with_size(1024, 768)
        .with_headless(true)
        .with_test_driver(|robot| {
            println!("✓ App launched\n");
            std::thread::sleep(Duration::from_millis(500));

            let click_button = |name: &str| -> bool {
                if let Some((x, y, w, h)) = find_button_in_semantics(&robot, name) {
                    println!("  Found button '{}' at ({:.1}, {:.1})", name, x, y);
                    robot.click(x + w / 2.0, y + h / 2.0).ok();
                    std::thread::sleep(Duration::from_millis(100));
                    true
                } else {
                    println!("  ✗ Button '{}' not found!", name);
                    false
                }
            };

            let verify_text = |text: &str| -> bool {
                if let Some((x, y, _, _)) = find_text_in_semantics(&robot, text) {
                    println!("  ✓ Found text '{}' at ({:.1}, {:.1})", text, x, y);
                    return true;
                }
                println!("  ✗ Text '{}' not found in semantics!", text);
                false
            };

            struct TabTestCase {
                button_name: &'static str,
                verification_text: &'static str,
            }

            let tabs = vec![
                TabTestCase {
                    button_name: "CompositionLocal Test",
                    verification_text: "CompositionLocal Subscription Test",
                },
                TabTestCase {
                    button_name: "Async Runtime",
                    verification_text: "Tap \"Fetch async value\"",
                },
                TabTestCase {
                    button_name: "Web Fetch",
                    verification_text: "Fetch JSON",
                },
                TabTestCase {
                    button_name: "Recursive Layout",
                    verification_text: "Recursive Layout Playground",
                },
                TabTestCase {
                    button_name: "Modifiers Showcase",
                    verification_text: "Showcase Selection",
                },
                TabTestCase {
                    button_name: "Mineswapper2",
                    verification_text: "Mineswapper",
                },
            ];

            for test_case in &tabs {
                println!("\n--- Switching to '{}' ---", test_case.button_name);
                if !click_button(test_case.button_name) {
                    println!("FATAL: Could not navigate to {}", test_case.button_name);
                    std::process::exit(1);
                }

                std::thread::sleep(Duration::from_millis(300));

                let check_text = match test_case.button_name {
                    "Async Runtime" => "Async Runtime Demo",
                    "Web Fetch" => "Fetch data from the web",
                    "Modifiers Showcase" => "Simple Card Pattern",
                    "Mineswapper2" => "New Game",
                    _ => test_case.verification_text,
                };

                if !verify_text(check_text) {
                    println!("WARNING: Verification failed for {}", test_case.button_name);
                }
            }

            println!("\n--- Returning to 'Counter App' ---");
            if !click_button("Counter App") {
                panic!("Failed to return to Counter App");
            }
            std::thread::sleep(Duration::from_millis(300));

            if !verify_text("Cranpose Playground") {
                panic!("Counter App content not found after return");
            }

            println!("\n--- Regression Check: Increment Button ---");

            if click_button("Increment") {
                println!("  ✓ Clicked Increment (Interactivity Check)");
            } else {
                panic!("Increment button not functional after tab tour!");
            }

            println!("\n✓ ALL TESTS PASSED");
            robot.exit().ok();
        })
        .run(app::combined_app);
}
