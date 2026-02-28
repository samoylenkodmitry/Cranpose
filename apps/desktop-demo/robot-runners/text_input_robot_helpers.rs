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

pub(crate) fn click_bounds(robot: &Robot, bounds: Bounds) -> Result<(), String> {
    let (x, y) = center_of(bounds);
    robot.click(x, y)?;
    let _ = robot.wait_for_idle();
    std::thread::sleep(FIND_DELAY);
    Ok(())
}

pub(crate) fn open_text_input_tab(robot: &Robot) -> bool {
    let Some(tab_bounds) = wait_for_in_semantics(robot, |robot| {
        find_button_in_semantics(robot, "Text Input")
            .or_else(|| find_text_in_semantics(robot, "Text Input"))
    }) else {
        return false;
    };

    if click_bounds(robot, tab_bounds).is_err() {
        return false;
    }

    wait_for_in_semantics(robot, |robot| {
        find_in_semantics(robot, |elem| find_text(elem, "Text Input Demo"))
    })
    .is_some()
}
