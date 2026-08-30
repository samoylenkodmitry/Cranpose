mod robot_launch;

mod output_paths;

use std::time::Duration;

use desktop_app::app;

fn main() {
    let recording_path = output_paths::diagnostic_path("robot_recording_test.rs");

    println!("=== Robot Recorder Test ===");
    println!("Recording to: {:?}\n", recording_path);

    robot_launch::launch("Robot Recorder Test", 800, 600)
        .with_recording(&recording_path)
        .with_test_driver(move |robot| {
            std::thread::sleep(Duration::from_millis(500));
            let _ = robot.wait_for_idle();

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

            std::thread::sleep(Duration::from_millis(500));
            let _ = robot.exit();
        })
        .run(|| {
            app::combined_app();
        });
}
