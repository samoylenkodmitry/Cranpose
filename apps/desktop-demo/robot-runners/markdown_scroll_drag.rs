#![allow(dead_code)]

use std::time::Duration;

use cranpose_testing::{
    find_button_in_semantics, find_in_semantics, find_text, print_semantics_with_bounds,
};

pub fn center(bounds: (f32, f32, f32, f32)) -> (f32, f32) {
    (bounds.0 + bounds.2 * 0.5, bounds.1 + bounds.3 * 0.5)
}

pub fn wait_for_text_bounds(
    robot: &cranpose::Robot,
    text: &str,
    timeout_ms: u64,
) -> Option<(f32, f32, f32, f32)> {
    let attempts = (timeout_ms / 100).max(1);
    for _ in 0..attempts {
        if let Some(bounds) = find_in_semantics(robot, |elem| find_text(elem, text)) {
            return Some(bounds);
        }
        std::thread::sleep(Duration::from_millis(100));
        let _ = robot.wait_for_idle();
    }
    None
}

pub fn fail_and_exit(robot: &cranpose::Robot, message: &str) -> ! {
    eprintln!("FATAL: {message}");
    if let Ok(semantics) = robot.get_semantics() {
        print_semantics_with_bounds(&semantics, 0);
    }
    let _ = robot.exit();
    std::process::exit(1);
}

pub fn click_button(robot: &cranpose::Robot, label: &str, settle_ms: u64) {
    let Some(bounds) = find_button_in_semantics(robot, label) else {
        fail_and_exit(robot, &format!("button '{label}' not found"));
    };
    let (x, y) = center(bounds);
    robot
        .click(x, y)
        .unwrap_or_else(|err| fail_and_exit(robot, &format!("click '{label}' failed: {err}")));
    std::thread::sleep(Duration::from_millis(settle_ms));
    let _ = robot.wait_for_idle();
}

#[allow(clippy::too_many_arguments)]
pub fn drag_scrollbar(
    robot: &cranpose::Robot,
    rail_bounds: (f32, f32, f32, f32),
    from_frac: f32,
    to_frac: f32,
    step_delay_ms: u64,
    wait_for_idle: bool,
) {
    let x = rail_bounds.0 + rail_bounds.2 * 0.5;
    let y0 = rail_bounds.1 + rail_bounds.3 * from_frac;
    let y1 = rail_bounds.1 + rail_bounds.3 * to_frac;
    let steps = 30;

    let _ = robot.mouse_move(x, y0);
    let _ = robot.mouse_down();
    for step in 0..=steps {
        let t = step as f32 / steps as f32;
        let y = y0 + (y1 - y0) * t;
        let _ = robot.mouse_move(x, y);
        std::thread::sleep(Duration::from_millis(step_delay_ms));
    }
    let _ = robot.mouse_up();
    std::thread::sleep(Duration::from_millis(120));
    if wait_for_idle {
        let _ = robot.wait_for_idle();
    }
}

pub struct DragViewportConfig {
    pub from_frac: f32,
    pub to_frac: f32,
    pub steps: u32,
    pub step_delay_ms: u64,
    pub settle_delay_ms: u64,
    pub wait_for_idle: bool,
}

pub fn drag_viewport(
    robot: &cranpose::Robot,
    viewport_bounds: (f32, f32, f32, f32),
    config: DragViewportConfig,
) {
    let x = viewport_bounds.0 + viewport_bounds.2 * 0.5;
    let y0 = viewport_bounds.1 + viewport_bounds.3 * config.from_frac;
    let y1 = viewport_bounds.1 + viewport_bounds.3 * config.to_frac;
    let _ = robot.mouse_move(x, y0);
    let _ = robot.mouse_down();
    for step in 0..=config.steps {
        let t = step as f32 / config.steps as f32;
        let y = y0 + (y1 - y0) * t;
        let _ = robot.mouse_move(x, y);
        std::thread::sleep(Duration::from_millis(config.step_delay_ms));
    }
    let _ = robot.mouse_up();
    std::thread::sleep(Duration::from_millis(config.settle_delay_ms));
    if config.wait_for_idle {
        let _ = robot.wait_for_idle();
    }
}
