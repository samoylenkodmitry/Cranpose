use std::time::Duration;

use cranpose::Robot;
use cranpose_testing::{find_button_in_semantics, find_text_by_prefix_in_semantics};

pub(crate) fn spawn_timeout(seconds: u64, label: &'static str) {
    std::thread::spawn(move || {
        std::thread::sleep(Duration::from_secs(seconds));
        eprintln!("TIMEOUT: {label} exceeded {seconds} seconds");
        std::process::exit(1);
    });
}

pub(crate) fn click_button(robot: &Robot, label: &str) -> Result<(), String> {
    let (x, y, w, h) = find_button_in_semantics(robot, label)
        .ok_or_else(|| format!("button '{label}' not found"))?;
    robot.click(x + w * 0.5, y + h * 0.5)?;
    robot.wait_for_idle().map_err(|err| err.to_string())?;
    Ok(())
}

pub(crate) fn wait_for_text(
    robot: &Robot,
    text: &str,
    attempts: usize,
    delay: Duration,
) -> Result<(f32, f32, f32, f32), String> {
    for _ in 0..attempts {
        if let Some(bounds) = robot.find_text_bounds(text)? {
            return Ok(bounds);
        }
        let _ = robot.wait_for_idle();
        std::thread::sleep(delay);
    }

    Err(format!("text '{text}' did not appear"))
}

pub(crate) fn wait_for_text_prefix(
    robot: &Robot,
    prefix: &str,
    attempts: usize,
    delay: Duration,
) -> Result<(f32, f32, f32, f32, String), String> {
    for _ in 0..attempts {
        if let Some(bounds) = find_text_by_prefix_in_semantics(robot, prefix) {
            return Ok(bounds);
        }
        let _ = robot.wait_for_idle();
        std::thread::sleep(delay);
    }

    Err(format!("text prefix '{prefix}' did not appear"))
}

pub(crate) fn semantics_dump(robot: &Robot) -> String {
    match robot.get_semantics() {
        Ok(semantics) => Robot::format_semantics(&semantics, 0),
        Err(err) => format!("failed to fetch semantics: {err}"),
    }
}
