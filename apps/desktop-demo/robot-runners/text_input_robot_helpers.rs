use cranpose::Robot;
use cranpose_testing::{
    find_button_in_semantics, find_in_semantics, find_text, find_text_in_semantics,
};
use std::time::Duration;

type Bounds = (f32, f32, f32, f32);

const FIND_ATTEMPTS: usize = 40;
const FIND_DELAY: Duration = Duration::from_millis(100);

fn center_of((x, y, w, h): Bounds) -> (f32, f32) {
    (x + w * 0.5, y + h * 0.5)
}

pub(crate) fn wait_for_in_semantics(
    robot: &Robot,
    mut finder: impl FnMut(&Robot) -> Option<Bounds>,
) -> Option<Bounds> {
    for _ in 0..FIND_ATTEMPTS {
        if let Some(bounds) = finder(robot) {
            return Some(bounds);
        }
        let _ = robot.wait_for_idle();
        std::thread::sleep(FIND_DELAY);
    }
    None
}

fn text_input_tab_bounds(robot: &Robot) -> Option<Bounds> {
    find_button_in_semantics(robot, "Text Input")
        .or_else(|| find_text_in_semantics(robot, "Text Input"))
}

fn text_input_demo_visible(robot: &Robot) -> bool {
    find_in_semantics(robot, |elem| find_text(elem, "Text Input Demo"))
        .or_else(|| find_in_semantics(robot, |elem| find_text(elem, "Empty Text Field:")))
        .is_some()
}

pub(crate) fn click_bounds(robot: &Robot, bounds: Bounds) -> Result<(), String> {
    let (x, y) = center_of(bounds);
    robot.click(x, y)?;
    let _ = robot.wait_for_idle();
    std::thread::sleep(FIND_DELAY);
    Ok(())
}

pub(crate) fn open_text_input_tab(robot: &Robot) -> bool {
    for _ in 0..FIND_ATTEMPTS {
        if text_input_demo_visible(robot) {
            return true;
        }

        if let Some(tab_bounds) = text_input_tab_bounds(robot) {
            if click_bounds(robot, tab_bounds).is_ok() && text_input_demo_visible(robot) {
                return true;
            }
        }

        let _ = robot.wait_for_idle();
        std::thread::sleep(FIND_DELAY);
    }

    false
}
