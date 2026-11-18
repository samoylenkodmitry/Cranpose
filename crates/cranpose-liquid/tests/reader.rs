// What the platform reader sees of a headless robot's screen.

use cranpose_testing::{
    PlacedSemanticsNode, RobotTestRule, TestRenderer, placed_semantics_from_shell,
};

pub(crate) type Robot = RobotTestRule<TestRenderer>;

// The reader nodes that are not hidden.
pub(crate) fn nodes(robot: &mut Robot) -> Vec<PlacedSemanticsNode> {
    let tree = placed_semantics_from_shell(robot.shell_mut()).expect("placed controls");
    tree.flatten()
        .into_iter()
        .filter(|node| !node.hidden)
        .cloned()
        .collect()
}

// The reader node named `label`.
pub(crate) fn node(robot: &mut Robot, label: &str) -> PlacedSemanticsNode {
    nodes(robot)
        .into_iter()
        .find(|node| node.label.as_deref() == Some(label))
        .unwrap_or_else(|| panic!("no reader node named {label}"))
}

// A press and release at (`x`, `y`), then the frames it starts.
pub(crate) fn tap(robot: &mut Robot, x: f32, y: f32) {
    robot.mouse_move(x, y);
    robot.mouse_down();
    robot.mouse_up();
    robot.wait_for_idle();
}

// A tap at the middle of the reader node named `label`.
pub(crate) fn tap_node(robot: &mut Robot, label: &str) {
    let bounds = node(robot, label).layout_bounds;
    tap(
        robot,
        bounds.x + bounds.width * 0.5,
        bounds.y + bounds.height * 0.5,
    );
}
