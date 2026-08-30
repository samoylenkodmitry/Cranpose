mod robot_exit;

use std::time::Duration;

use cranpose::AppLauncher;
use cranpose_testing::{
    exit_with_timeout, find_button_in_semantics, find_text_by_prefix_in_semantics,
    find_text_in_semantics,
};
use desktop_app::app;

fn click_button(robot: &cranpose::Robot, label: &str) {
    let Some((x, y, w, h)) = find_button_in_semantics(robot, label) else {
        robot_exit::fail(robot, &format!("button {label:?} not found"));
    };
    let cx = x + w / 2.0;
    let cy = y + h / 2.0;
    robot.mouse_move(cx, cy).ok();
    std::thread::sleep(Duration::from_millis(50));
    robot.mouse_down().ok();
    std::thread::sleep(Duration::from_millis(50));
    robot.mouse_up().ok();
}

fn read_progress_percent(robot: &cranpose::Robot) -> Option<i32> {
    let (_, _, _, _, text) = find_text_by_prefix_in_semantics(robot, "Progress: ")?;
    let digits = text
        .strip_prefix("Progress: ")?
        .trim()
        .trim_end_matches('%')
        .trim();
    digits.parse::<i32>().ok()
}

fn visible_texts(robot: &cranpose::Robot) -> Vec<String> {
    let Ok(semantics) = robot.get_semantics() else {
        return Vec::new();
    };
    fn collect(node: &cranpose::SemanticElement, out: &mut Vec<String>) {
        if let Some(text) = &node.text {
            out.push(text.clone());
        }
        for child in &node.children {
            collect(child, out);
        }
    }
    let mut texts = Vec::new();
    for root in &semantics {
        collect(root, &mut texts);
    }
    texts
}

fn wait_for_text(robot: &cranpose::Robot, text: &str, timeout: Duration) {
    let deadline = std::time::Instant::now() + timeout;
    while std::time::Instant::now() < deadline {
        if find_text_in_semantics(robot, text).is_some() {
            return;
        }
        std::thread::sleep(Duration::from_millis(50));
    }
    robot_exit::fail(
        robot,
        &format!(
            "timed out waiting for text {text:?}; visible_texts={:?}",
            visible_texts(robot)
        ),
    );
}

fn main() {
    env_logger::init();
    println!("=== Robot Async Progress After Animations Test ===");

    AppLauncher::new()
        .with_title("Robot Async Progress After Animations Test")
        .with_size(1200, 800)
        .with_headless(true)
        .with_test_driver(|robot| {
            println!("✓ App launched");
            wait_for_text(&robot, "Counter App", Duration::from_secs(5));

            click_button(&robot, "Animations");
            wait_for_text(&robot, "Animations Showcase", Duration::from_secs(5));

            click_button(&robot, "Async Runtime");
            wait_for_text(&robot, "Async Runtime Demo", Duration::from_secs(5));

            let initial_progress = read_progress_percent(&robot).unwrap_or_else(|| {
                robot_exit::fail(
                    &robot,
                    &format!(
                        "Async progress text missing after switching from Animations; visible_texts={:?}",
                        visible_texts(&robot)
                    ));
            });

            println!("Initial async progress after switch: {initial_progress}%");

            let mut progressed = false;
            let mut last_progress = initial_progress;
            for _ in 0..30 {
                std::thread::sleep(Duration::from_millis(100));
                if let Some(progress) = read_progress_percent(&robot) {
                    last_progress = progress;
                    if progress > 0 {
                        progressed = true;
                        break;
                    }
                }
            }

            if !progressed {
                robot_exit::fail(
                    &robot,
                    &format!(
                        "Async Runtime progress stayed stuck after Animations -> Async switch; initial={initial_progress}% last={last_progress}% visible_texts={:?}",
                        visible_texts(&robot)
                    ));
            }

            println!(
                "PASS: Async Runtime progress advanced after Animations -> Async switch ({}% -> {}%)",
                initial_progress, last_progress
            );
            exit_with_timeout(&robot, Duration::from_secs(2));
        })
        .run(app::combined_app);
}
