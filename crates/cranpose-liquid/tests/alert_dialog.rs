// `LiquidAlertDialog` through the headless robot: it reads its title and
// message, puts the cancel action of two on the left, and runs an action and
// then the dismiss request when the action is chosen.

use std::{cell::RefCell, rc::Rc};

use cranpose_core::{MutableState, rememberMutableStateOf};
use cranpose_liquid::prelude::*;
use cranpose_testing::create_headless_robot_test;
use cranpose_ui::SemanticsWidgetRole;

use crate::reader::{node, nodes};

#[test]
fn an_alert_runs_the_chosen_action_and_closes() {
    let log = Rc::new(RefCell::new(Vec::<&str>::new()));
    let slot: Rc<RefCell<Option<MutableState<bool>>>> = Rc::default();
    let (record, captured) = (Rc::clone(&log), Rc::clone(&slot));
    let mut robot = create_headless_robot_test(400, 800, move || {
        let (record, captured) = (Rc::clone(&record), Rc::clone(&captured));
        LiquidTheme(LiquidThemeSpec::default(), move || {
            let open = rememberMutableStateOf(|| true);
            *captured.borrow_mut() = Some(open);
            if !open.get() {
                return;
            }
            let (delete, dismiss) = (Rc::clone(&record), Rc::clone(&record));
            LiquidAlertDialog(
                move || {
                    dismiss.borrow_mut().push("dismiss");
                    open.set(false);
                },
                "Delete Photo?",
                "This photo will be deleted from all your devices.",
                move |scope| {
                    scope.action("Delete", LiquidAlertRole::Destructive, move || {
                        delete.borrow_mut().push("delete");
                    });
                    scope.action("Cancel", LiquidAlertRole::Cancel, || {});
                },
            );
        });
    });
    robot.shell_mut().set_semantics_enabled(true);
    robot.wait_for_idle();

    node(&mut robot, "Delete Photo?");
    node(
        &mut robot,
        "This photo will be deleted from all your devices.",
    );
    let (delete, cancel) = (node(&mut robot, "Delete"), node(&mut robot, "Cancel"));
    for button in [&delete, &cancel] {
        assert_eq!(button.widget_role, Some(SemanticsWidgetRole::Button));
    }
    assert!(
        cancel.layout_bounds.x < delete.layout_bounds.x,
        "the cancel action of two sits on the left"
    );

    assert!(
        robot
            .shell_mut()
            .accessibility_activate(delete.node_id, None)
    );
    robot.wait_for_idle();
    assert_eq!(*log.borrow(), ["delete", "dismiss"]);
    assert!(
        nodes(&mut robot)
            .iter()
            .all(|node| node.label.as_deref() != Some("Delete Photo?")),
        "the alert closes"
    );
    assert_eq!(slot.borrow().map(|open| open.get()), Some(false));
}
