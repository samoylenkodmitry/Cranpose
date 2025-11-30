//! Robot demonstration - watch the robot interact with the app
//!
//! Run with:
//! ```bash
//! cargo run --package desktop-app --example robot_demo --features robot-app
//! ```

use desktop_app::app;
use compose_app::AppLauncher;
use std::time::Duration;

fn main() {
    println!("Launching app with robot control...");

    AppLauncher::new()
        .with_title("Robot Demo")
        .with_size(800, 600)
        .with_test_driver(|robot| {
            println!("App launched! Starting robot interactions in 1 second...");
            std::thread::sleep(Duration::from_secs(1));

            // Wait for app to be ready
            robot.wait_for_idle().expect("Failed to wait for idle");

            // Click the increment button 5 times
            println!("Clicking increment button 5 times...");
            for i in 1..=5 {
                println!("  Click {}/5", i);
                robot.click(150.0, 560.0).expect("Failed to click");
                std::thread::sleep(Duration::from_millis(300));
            }

            println!("Switching to Async Runtime tab...");
            robot.click(400.0, 50.0).expect("Failed to click tab");
            match robot.wait_for_idle() {
                Ok(_) => println!("Tab ready (idle achieved)"),
                Err(e) => println!("Tab switched ({})", e),
            }
            std::thread::sleep(Duration::from_secs(1));

            println!("Switching to Modifiers Showcase tab...");
            robot.click(800.0, 50.0).expect("Failed to click tab");
            // Wait for idle - animations are a valid app state, so timeout is not a failure
            match robot.wait_for_idle() {
                Ok(_) => println!("Tab ready (idle achieved)"),
                Err(e) => println!("Tab switched ({})", e),
            }
            std::thread::sleep(Duration::from_secs(1));

            println!("Going back to Counter App tab...");
            robot.click(70.0, 50.0).expect("Failed to click tab");
            match robot.wait_for_idle() {
                Ok(_) => println!("Tab ready (idle achieved)"),
                Err(e) => println!("Tab switched ({})", e),
            }
            std::thread::sleep(Duration::from_secs(1));

            println!("Demo complete! Keeping window open for 5 more seconds...");
            std::thread::sleep(Duration::from_secs(5));

            println!("Shutting down...");
            robot.exit().expect("Failed to exit");
        })
        .run(|| {
            app::combined_app();
        });
}
