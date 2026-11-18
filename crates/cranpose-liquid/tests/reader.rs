// What the platform reader sees of a headless robot's screen.

use std::{cell::RefCell, rc::Rc};

use cranpose_liquid::prelude::{LiquidTheme, LiquidThemeSpec};
use cranpose_testing::{
    PlacedSemanticsNode, RobotTestRule, TestRenderer, create_headless_robot_test,
    placed_semantics_from_shell,
};

pub(crate) type Robot = RobotTestRule<TestRenderer>;

// What a screen's taps ran, in order.
pub(crate) type Log = Rc<RefCell<Vec<&'static str>>>;

// A 400 by 800 window that shows `content` in the default Liquid theme,
// with the reader tree on. `content` gets the log its taps write to.
pub(crate) fn liquid_screen(content: impl Fn(&Log) + 'static) -> (Robot, Log) {
    let log: Log = Rc::default();
    let record = Rc::clone(&log);
    let content = Rc::new(content);
    let mut robot = create_headless_robot_test(400, 800, move || {
        let (record, content) = (Rc::clone(&record), Rc::clone(&content));
        LiquidTheme(LiquidThemeSpec::default(), move || content(&record));
    });
    robot.shell_mut().set_semantics_enabled(true);
    robot.wait_for_idle();
    (robot, log)
}

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

// A tap at the middle of where the reader node named `label` shows.
pub(crate) fn tap_node(robot: &mut Robot, label: &str) {
    let bounds = node(robot, label).target_bounds();
    tap(
        robot,
        bounds.x + bounds.width * 0.5,
        bounds.y + bounds.height * 0.5,
    );
}
