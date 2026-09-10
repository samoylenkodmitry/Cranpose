#![allow(dead_code)]

use cranpose::{Robot, SemanticElement};

/// A node's rectangle, as `(x, y, width, height)`.
pub type Bounds = (f32, f32, f32, f32);

/// The control whose accessibility name is `name`, as `(reading, bounds)`.
/// Only a node that also publishes a state description answers: a demo often
/// prints the same name in a label beside the control, and a label has no
/// reading to give.
pub fn named_control(robot: &Robot, name: &str) -> (String, Bounds) {
    let semantics = robot.get_semantics().expect("semantics");
    semantics
        .iter()
        .find_map(|element| walk(element, name))
        .unwrap_or_else(|| {
            crate::robot_exit::fail_without_shutdown(&format!(
                "no control named '{name}' publishes a reading"
            ));
        })
}

/// Fail unless the control named `name` reads exactly `want`. `why` says what
/// the reading proves, so a failure names the behaviour and not the string.
pub fn expect_reading(robot: &Robot, name: &str, want: &str, why: &str) {
    let got = named_control(robot, name).0;
    if got != want {
        crate::robot_exit::fail_without_shutdown(&format!(
            "{name} reads '{got}', expected '{want}': {why}"
        ));
    }
}

fn walk(element: &SemanticElement, name: &str) -> Option<(String, Bounds)> {
    if element.text.as_deref() == Some(name) {
        if let Some(reading) = &element.state_description {
            let bounds = element.bounds;
            return Some((
                reading.clone(),
                (bounds.x, bounds.y, bounds.width, bounds.height),
            ));
        }
    }
    element.children.iter().find_map(|child| walk(child, name))
}
