use crate::{robot_exit, robot_launch};

use std::time::Duration;

use cranpose_testing::{find_button_exact_in_semantics, find_text_center};

pub(crate) fn main() {
    println!("=== Robot Async Tab Bug Test ===");
    println!("Testing if clicks stop working after switching to Async Runtime tab");

    const TEST_TIMEOUT_SECS: u64 = 60;

    robot_launch::launch("Robot Async Tab Bug Test", 1024, 768)
        .with_test_driver(|robot| {
            robot_exit::arm_timeout(TEST_TIMEOUT_SECS);

            println!("\n✓ App launched");
            std::thread::sleep(Duration::from_millis(500));
            let _ = robot.wait_for_idle();

            println!("\n--- Step 1: Switch to Async Runtime Tab ---");

            // The tab bar scrolls the tab into view when the window is too
            // narrow to show it.
            let async_tab_pos = find_button_exact_in_semantics(&robot, "Async Runtime")
                .map(|(x, y, width, height)| (x + width * 0.5, y + height * 0.5));

            if let Some((x, y)) = async_tab_pos {
                println!("Found Async Runtime tab at ({x}, {y})");
                println!("Async Runtime button clicked");
                let _ = robot.mouse_move(x, y);
                let _ = robot.mouse_down();

                std::thread::sleep(Duration::from_millis(100));

                let _ = robot.mouse_up();
                println!("Clicked Async Runtime tab");

                std::thread::sleep(Duration::from_secs(1));
            } else {
                println!("✗ Failed to find Async Runtime tab");
                let _ = robot.exit();
            }

            let semantics = robot.get_semantics().expect("the robot reads the semantics tree");
            if semantics
                .iter()
                .any(|root| find_text_center(root, "Async Runtime Demo").is_some())
            {
                println!("✓ Verified we are on Async Runtime tab");
            } else {
                println!("✗ Failed to verify Async Runtime tab content");
                let _ = robot.exit();
            }

            println!("\n--- Step 2: Switch back to Counter App Tab ---");

            // The tab bar scrolls the tab into view when the window is too
            // narrow to show it.
            let counter_tab_pos = find_button_exact_in_semantics(&robot, "Counter App")
                .map(|(x, y, width, height)| (x + width * 0.5, y + height * 0.5));

            if let Some((x, y)) = counter_tab_pos {
                println!("Found Counter App tab at ({x}, {y})");
                let _ = robot.mouse_move(x, y);
                let _ = robot.mouse_down();
                println!("Counter App button clicked");

                std::thread::sleep(Duration::from_millis(100));

                let _ = robot.mouse_up();
                println!("Clicked Counter App tab");

                std::thread::sleep(Duration::from_secs(1));
            } else {
                println!("✗ Failed to find Counter App tab");
                let _ = robot.exit();
            }

            let semantics = robot.get_semantics().expect("the robot reads the semantics tree");
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
        .run(robot_launch::counter_demo);
}
