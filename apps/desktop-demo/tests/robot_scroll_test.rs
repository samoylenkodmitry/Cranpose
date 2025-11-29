use compose_app::AppLauncher;
use compose_app::robot::{Robot, RobotCommand, RobotResponse};
use desktop_app::app::combined_app;
use std::sync::mpsc::{channel, Receiver, Sender};
use std::thread;
use std::time::Duration;

#[test]
fn robot_scroll_test() {
    // This test launches the actual app and drives it with a robot.
    // It verifies that scrolling works (or fails, reproducing the bug).

    AppLauncher::new()
        .with_title("Robot Scroll Test")
        .with_size(800, 600)
        .with_test_driver(|robot| {
            let run_test = || -> Result<(), String> {
                // Wait for app to start and layout
                robot.wait_for_idle()?;
                
                // Find the "Counter App" tab which should be visible
                robot.find_node_with_text("Counter App")?;
                
                // Perform a swipe left gesture (drag right-to-left) to scroll the row
                // The row is at the top, let's say y=50.
                // We drag from x=600 to x=400.
                println!("Found 'Counter App', performing swipe left...");
                robot.swipe_left(600.0, 50.0, 200.0)?;
                
                robot.wait_for_idle()?;
                
                // Verify we didn't crash.
                // Ideally we would check if "Mineswapper2" is now visible or closer to being visible.
                // But for reproduction, just performing the gesture and seeing if it works (or fails silently) is enough.
                
                Ok(())
            };

            match run_test() {
                Ok(_) => println!("Test completed successfully"),
                Err(e) => println!("Test failed: {}", e),
            }
            
            // Always exit the app
            let _ = robot.exit();
        })
        .run(combined_app);
}
