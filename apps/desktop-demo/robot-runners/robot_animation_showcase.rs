mod robot_launch;

use std::time::Duration;

use cranpose_testing::{
    find_button_in_semantics, find_text_by_prefix_in_semantics, find_text_in_semantics,
};
use desktop_app::app;

fn read_lazy_pulse(robot: &cranpose::Robot) -> Option<String> {
    find_text_by_prefix_in_semantics(robot, "Lazy Pulse:").map(|(_, _, _, _, text)| text)
}

fn read_alpha_text(robot: &cranpose::Robot) -> Option<String> {
    find_text_by_prefix_in_semantics(robot, "Alpha ").map(|(_, _, _, _, text)| text)
}

fn wait_for_text_change(
    robot: &cranpose::Robot,
    initial: &str,
    read: impl Fn(&cranpose::Robot) -> Option<String>,
) -> Option<String> {
    for _ in 0..30 {
        std::thread::sleep(Duration::from_millis(100));
        let current = read(robot)?;
        if current != initial {
            return Some(current);
        }
    }
    None
}

fn main() {
    env_logger::init();
    println!("=== Robot Animation Showcase Test ===");

    robot_launch::launch("Robot Animation Showcase Test", 1200, 800)
        .with_test_driver(|robot| {
            println!("✓ App launched");
            std::thread::sleep(Duration::from_millis(500));
            let _ = robot.wait_for_idle();

            if let Some((x, y, w, h)) = find_button_in_semantics(&robot, "Animations") {
                robot.click(x + w / 2.0, y + h / 2.0).ok();
            } else {
                println!("FATAL: 'Animations' tab not found");
                robot.exit().ok();
                std::process::exit(1);
            }

            std::thread::sleep(Duration::from_millis(400));

            if find_text_in_semantics(&robot, "Animations Showcase").is_none() {
                println!("FATAL: Animations tab content not visible");
                robot.exit().ok();
                std::process::exit(1);
            }

            let alpha_first = read_alpha_text(&robot).unwrap_or_else(|| {
                println!("FATAL: Alpha text not found");
                robot.exit().ok();
                std::process::exit(1);
            });

            let first = read_lazy_pulse(&robot).unwrap_or_else(|| {
                println!("FATAL: Lazy Pulse text not found");
                robot.exit().ok();
                std::process::exit(1);
            });

            let alpha_second = wait_for_text_change(&robot, &alpha_first, read_alpha_text)
                .unwrap_or_else(|| {
                    println!("FAIL: Alpha text did not change: '{}'", alpha_first);
                    robot.exit().ok();
                    std::process::exit(1);
                });

            let second =
                wait_for_text_change(&robot, &first, read_lazy_pulse).unwrap_or_else(|| {
                    println!("FAIL: Lazy Pulse did not change: '{}'", first);
                    robot.exit().ok();
                    std::process::exit(1);
                });

            println!(
                "PASS: Alpha updated from '{}' to '{}'",
                alpha_first, alpha_second
            );
            println!("PASS: Lazy Pulse updated from '{}' to '{}'", first, second);
            robot.exit().ok();
        })
        .run(app::combined_app);
}
