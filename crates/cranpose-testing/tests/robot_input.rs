use std::{cell::Cell, rc::Rc};

use cranpose_testing::create_headless_robot_test;
use cranpose_ui::{Box, BoxSpec, Modifier, Size};

#[test]
fn headless_robot_dispatches_pointer_and_accessibility_activation() {
    let count = Rc::new(Cell::new(0));
    let recorded = Rc::clone(&count);
    let mut robot = create_headless_robot_test(200, 100, move || {
        let recorded = Rc::clone(&recorded);
        Box(
            Modifier::empty()
                .size(Size::new(100.0, 48.0))
                .content_description("Increment")
                .clickable(move |_| recorded.set(recorded.get() + 1)),
            BoxSpec::default(),
            || {},
        );
    });
    assert!(robot.click_at(50.0, 24.0));
    assert_eq!(count.get(), 1);
    robot.shell_mut().set_semantics_enabled(true);
    let tree = cranpose_testing::placed_semantics_from_shell(robot.shell_mut()).expect("control");
    let control = tree
        .flatten()
        .into_iter()
        .find(|node| node.clickable)
        .expect("button");
    assert!(
        robot
            .shell_mut()
            .accessibility_activate(control.node_id, None)
    );
    assert_eq!(count.get(), 2);
    assert!(!robot.click_at(150.0, 75.0));
    assert_eq!(count.get(), 2);
}
