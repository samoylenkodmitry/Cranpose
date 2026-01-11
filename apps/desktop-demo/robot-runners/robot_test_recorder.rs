//! Robot test to verify recorder generates valid output
//!
//! Uses with_test_driver + with_recording together
//!
//! Run with:
//! ```bash
//! cargo run --package desktop-app --example robot_test_recorder --features robot-app
//! ```

use compose_app::AppLauncher;
use desktop_app::app;
use std::path::PathBuf;
use std::time::Duration;

fn main() {
    let recording_path = PathBuf::from("/tmp/robot_recording_test.rs");

    println!("=== Robot Recorder Test ===");
    println!("Recording to: {:?}\n", recording_path);

    AppLauncher::new()
        .with_title("Robot Recorder Test")
        .with_size(800, 600)
        .with_headless(true)
        .with_recording(&recording_path)
        .with_test_driver(move |robot| {
            std::thread::sleep(Duration::from_millis(500));
            let _ = robot.wait_for_idle();

            // Do some mouse movements
            let _ = robot.mouse_move(100.0, 100.0);
            std::thread::sleep(Duration::from_millis(50));
            let _ = robot.mouse_move(200.0, 200.0);
            std::thread::sleep(Duration::from_millis(50));
            let _ = robot.mouse_down();
            std::thread::sleep(Duration::from_millis(30));
            let _ = robot.mouse_up();
            std::thread::sleep(Duration::from_millis(50));
            let _ = robot.mouse_move(300.0, 300.0);

            println!("✓ Recorded some mouse events");

            // Exit - this will trigger recording save
            std::thread::sleep(Duration::from_millis(500));
            let _ = robot.exit();
        })
        .run(|| {
            app::combined_app();
        });
}
