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
