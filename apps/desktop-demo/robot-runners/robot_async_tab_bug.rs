use compose_app::AppLauncher;
use compose_testing::find_text_center;
use desktop_app::app;
use std::time::Duration;

fn main() {
    println!("=== Robot Async Tab Bug Test ===");
    println!("Testing if clicks stop working after switching to Async Runtime tab");

    const TEST_TIMEOUT_SECS: u64 = 60;

    AppLauncher::new()
        .with_title("Robot Async Tab Bug Test")
        .with_size(800, 600)
        .with_test_driver(|robot| {
            // Timeout after a full robot run budget.
            std::thread::spawn(move || {
                std::thread::sleep(std::time::Duration::from_secs(TEST_TIMEOUT_SECS));
                println!(
                    "Test finished (timeout after {} seconds)",
                    TEST_TIMEOUT_SECS
                );
                std::process::exit(1);
            });

            println!("\n✓ App launched");
            std::thread::sleep(Duration::from_millis(500));
            // Initial wait for idle is fine as app starts in Counter tab which is static
            let _ = robot.wait_for_idle();

            // 1. Switch to Async Runtime tab
            println!("\n--- Step 1: Switch to Async Runtime Tab ---");

            let semantics = robot.get_semantics().unwrap();
            let async_tab_pos = semantics
                .iter()
                .find_map(|root| find_text_center(root, "Async Runtime"));

            if let Some((x, y)) = async_tab_pos {
                println!("Found Async Runtime tab at ({}, {})", x, y);
                println!("Async Runtime button clicked");
                let _ = robot.mouse_move(x, y);
                let _ = robot.mouse_down();

                // Wait a bit to simulate manual interaction and allow recomposition to happen
                // This is crucial to reproduce the bug where ClickableNode state is lost
                std::thread::sleep(std::time::Duration::from_millis(100));

                let _ = robot.mouse_up();
                println!("Clicked Async Runtime tab");

                // Wait for tab switch
                std::thread::sleep(Duration::from_secs(1));
            } else {
                println!("✗ Failed to find Async Runtime tab");
                let _ = robot.exit();
            }

            // Verify we are on Async Runtime tab
            let semantics = robot.get_semantics().unwrap();
            if semantics
                .iter()
                .any(|root| find_text_center(root, "Async Runtime Demo").is_some())
            {
                println!("✓ Verified we are on Async Runtime tab");
            } else {
                println!("✗ Failed to verify Async Runtime tab content");
                let _ = robot.exit();
            }

            // 2. Switch back to Counter App tab
            println!("\n--- Step 2: Switch back to Counter App Tab ---");

            // We need to find the Counter App tab button.
            // Since the app is animating, we might need to retry getting semantics if it fails?
            // But usually it should be fine.

            let semantics = robot.get_semantics().unwrap();
            let counter_tab_pos = semantics
                .iter()
                .find_map(|root| find_text_center(root, "Counter App"));

            if let Some((x, y)) = counter_tab_pos {
                println!("Found Counter App tab at ({}, {})", x, y);
                let _ = robot.mouse_move(x, y);
                let _ = robot.mouse_down();
                println!("Counter App button clicked");

                // Wait a bit to simulate manual interaction
                std::thread::sleep(std::time::Duration::from_millis(100));

                let _ = robot.mouse_up();
                println!("Clicked Counter App tab");

                // Wait for tab switch
                std::thread::sleep(Duration::from_secs(1));
            } else {
                println!("✗ Failed to find Counter App tab");
                let _ = robot.exit();
            }

            // Verify we are on Counter App tab
            let semantics = robot.get_semantics().unwrap();
            if semantics
                .iter()
                .any(|root| find_text_center(root, "Counter: 0").is_some())
            {
                println!("✓ PASS: Successfully switched back to Counter App tab");
                println!("=== Test Summary ===");
                println!("✓ ALL TESTS PASSED");
                let _ = robot.exit();
            } else {
                println!("✗ Failed to verify Counter App tab content");
                println!("BUG REPRODUCED: Could not switch back to Counter App!");
                let _ = robot.exit();
            }
        })
        .run(app::combined_app);
}
