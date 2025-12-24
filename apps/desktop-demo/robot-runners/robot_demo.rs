//! Robot demonstration - watch the robot interact with the app
//!
//! Run with:
//! ```bash
//! cargo run --package desktop-app --example robot_demo --features robot-app
//! ```

use compose_app::{AppLauncher, Robot};
use desktop_app::app;
use std::time::Duration;

fn wait_for_content(robot: &Robot, expected: &str, attempts: usize, delay: Duration) -> bool {
    for _ in 0..attempts {
        if robot.validate_content(expected).is_ok() {
            return true;
        }
        std::thread::sleep(delay);
    }
    false
}

fn main() {
    println!("Launching app with robot control...");

    AppLauncher::new()
        .with_title("Robot Demo")
        .with_size(800, 600)
        .with_test_driver(|robot| {
            println!("App launched! Starting robot interactions in 1 second...");
            std::thread::sleep(Duration::from_secs(1));

            // Wait for app to be ready
            let _ = wait_for_content(&robot, "Increment", 10, Duration::from_millis(200));

            // Click the increment button 5 times
            println!("Clicking increment button 5 times...");
            for i in 1..=5 {
                println!("  Click {}/5", i);
                robot.click(150.0, 560.0).expect("Failed to click");
                std::thread::sleep(Duration::from_millis(300));
            }

            println!("Switching to Async Runtime tab...");
            robot.click(400.0, 50.0).expect("Failed to click tab");
            if wait_for_content(&robot, "Async Runtime Demo", 10, Duration::from_millis(200)) {
                println!("Tab ready (Async Runtime Demo found)");
            } else {
                println!("Tab switched (Async Runtime Demo not found)");
            }
            std::thread::sleep(Duration::from_secs(1));

            println!("Switching to Modifiers Showcase tab...");
            robot.click(800.0, 50.0).expect("Failed to click tab");
            if wait_for_content(&robot, "Select Showcase", 10, Duration::from_millis(200)) {
                println!("Tab ready (Modifiers Showcase found)");
            } else {
                println!("Tab switched (Modifiers Showcase not found)");
            }
            std::thread::sleep(Duration::from_secs(1));

            println!("Going back to Counter App tab...");
            robot.click(70.0, 50.0).expect("Failed to click tab");
            if wait_for_content(&robot, "Increment", 10, Duration::from_millis(200)) {
                println!("Tab ready (Counter App found)");
            } else {
                println!("Tab switched (Counter App not found)");
            }

            println!("Demo complete! Keeping window open for 1 more seconds...");
            std::thread::sleep(Duration::from_secs(1));

            println!("Shutting down...");
            robot.exit().expect("Failed to exit");
        })
        .run(|| {
            app::combined_app();
        });
}
