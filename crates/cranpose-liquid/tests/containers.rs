// `LiquidControlGroup` and `LiquidContentUnavailableView` through the
// headless robot: the group's buttons share its width and run their own
// actions; the content-unavailable view shows its title and description
// and composes its actions.

use std::{cell::Cell, rc::Rc};

use cranpose_liquid::prelude::*;
use cranpose_testing::create_headless_robot_test;
use cranpose_ui::{Modifier, SemanticsWidgetRole};

use crate::reader::{node, nodes};

#[test]
fn a_control_group_splits_its_width_and_runs_each_buttons_action() {
    let taps = Rc::new(Cell::new((0, 0)));
    let counter = Rc::clone(&taps);
    let mut robot = create_headless_robot_test(400, 200, move || {
        let counter = Rc::clone(&counter);
        LiquidTheme(LiquidThemeSpec::default(), move || {
            let (previous, next) = (Rc::clone(&counter), Rc::clone(&counter));
            LiquidControlGroup(Modifier::empty().width(300.0), move |scope| {
                scope.button("Previous", move || {
                    previous.set((previous.get().0 + 1, previous.get().1));
                });
                scope.button("Next", move || next.set((next.get().0, next.get().1 + 1)));
            });
        });
    });
    robot.shell_mut().set_semantics_enabled(true);
    robot.wait_for_idle();

    let (previous, next) = (node(&mut robot, "Previous"), node(&mut robot, "Next"));
    for button in [&previous, &next] {
        assert_eq!(button.widget_role, Some(SemanticsWidgetRole::Button));
        assert_eq!(button.layout_bounds.width, 150.0, "each button takes half");
    }
    assert!(next.layout_bounds.x > previous.layout_bounds.x);
    assert!(robot.shell_mut().accessibility_activate(next.node_id, None));
    robot.wait_for_idle();
    assert_eq!(taps.get(), (0, 1));
}

#[test]
fn a_content_unavailable_view_reads_its_message_and_composes_its_actions() {
    let taps = Rc::new(Cell::new(0));
    let counter = Rc::clone(&taps);
    let mut robot = create_headless_robot_test(400, 400, move || {
        let counter = Rc::clone(&counter);
        LiquidTheme(LiquidThemeSpec::default(), move || {
            let counter = Rc::clone(&counter);
            LiquidContentUnavailableView(
                Modifier::empty().width(320.0),
                "No Photos",
                "Photos you take appear here.",
                move || {
                    let counter = Rc::clone(&counter);
                    GlassButton(
                        Modifier::empty(),
                        GlassButtonSpec::plain(),
                        move || counter.set(counter.get() + 1),
                        || GlassButtonLabel("Take Photo".into(), GlassButtonSpec::plain()),
                    );
                },
            );
        });
    });
    robot.shell_mut().set_semantics_enabled(true);
    robot.wait_for_idle();

    let title = node(&mut robot, "No Photos");
    let description = node(&mut robot, "Photos you take appear here.");
    assert!(description.layout_bounds.y > title.layout_bounds.y);
    let action = nodes(&mut robot)
        .into_iter()
        .find(|node| node.clickable)
        .expect("the action is a button");
    assert!(action.layout_bounds.y > description.layout_bounds.y);
    assert!(
        robot
            .shell_mut()
            .accessibility_activate(action.node_id, None)
    );
    robot.wait_for_idle();
    assert_eq!(taps.get(), 1);
}
