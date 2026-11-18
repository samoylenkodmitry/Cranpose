// `LiquidConfirmationDialog` and `LiquidPopover` through the headless robot:
// the card reads its title, message and actions but not the cancel action; it
// sits above a wide control and beside a small one near a side; an action
// runs and then the dismiss request; a tap outside the card and the back
// gesture run the cancel action and then the dismiss request.

use std::{cell::RefCell, rc::Rc};

use cranpose_core::{MutableState, rememberMutableStateOf};
use cranpose_liquid::prelude::*;
use cranpose_testing::create_headless_robot_test;
use cranpose_ui::{
    Alignment, Modifier, SemanticsWidgetRole,
    text::TextStyle,
    widgets::{Box, BoxSpec, Text},
};

use crate::reader::{Robot, node, nodes};

const TITLE: &str = "Delete Photo?";
const MESSAGE: &str = "This photo will be deleted from all your devices.";

type Log = Rc<RefCell<Vec<&'static str>>>;

// A confirmation dialog pointing at a `width` by 44 source placed by
// `alignment` in a 400 by 800 window, with its open state.
fn confirmation(
    alignment: Alignment,
    width: f32,
) -> (Robot, Log, Rc<RefCell<Option<MutableState<bool>>>>) {
    let log: Log = Rc::default();
    let slot: Rc<RefCell<Option<MutableState<bool>>>> = Rc::default();
    let (record, captured) = (Rc::clone(&log), Rc::clone(&slot));
    let mut robot = create_headless_robot_test(400, 800, move || {
        let (record, captured) = (Rc::clone(&record), Rc::clone(&captured));
        LiquidTheme(LiquidThemeSpec::default(), move || {
            let open = rememberMutableStateOf(|| true);
            *captured.borrow_mut() = Some(open);
            let record = Rc::clone(&record);
            Box(
                Modifier::empty().fill_max_size().padding(24.0),
                BoxSpec::default().content_alignment(alignment),
                move || {
                    let (delete, cancel, dismiss) =
                        (Rc::clone(&record), Rc::clone(&record), Rc::clone(&record));
                    LiquidConfirmationDialog(
                        Modifier::empty().width(width).height(44.0),
                        open.get(),
                        move || {
                            dismiss.borrow_mut().push("dismiss");
                            open.set(false);
                        },
                        TITLE,
                        MESSAGE,
                        move |scope| {
                            let (delete, cancel) = (Rc::clone(&delete), Rc::clone(&cancel));
                            scope.action("Delete", LiquidAlertRole::Destructive, move || {
                                delete.borrow_mut().push("delete");
                            });
                            scope.action("Cancel", LiquidAlertRole::Cancel, move || {
                                cancel.borrow_mut().push("cancel");
                            });
                        },
                        || {},
                    );
                },
            );
        });
    });
    robot.shell_mut().set_semantics_enabled(true);
    robot.wait_for_idle();
    (robot, log, slot)
}

fn closed(robot: &mut Robot) -> bool {
    nodes(robot)
        .iter()
        .all(|node| node.label.as_deref() != Some(TITLE))
}

#[test]
fn a_confirmation_reads_its_actions_but_not_cancel_and_runs_the_chosen_one() {
    let (mut robot, log, slot) = confirmation(Alignment::CENTER, 300.0);
    node(&mut robot, TITLE);
    node(&mut robot, MESSAGE);
    let delete = node(&mut robot, "Delete");
    assert_eq!(delete.widget_role, Some(SemanticsWidgetRole::Button));
    assert!(
        nodes(&mut robot)
            .iter()
            .all(|node| node.label.as_deref() != Some("Cancel")),
        "the cancel action is not drawn"
    );

    assert!(
        robot
            .shell_mut()
            .accessibility_activate(delete.node_id, None)
    );
    robot.wait_for_idle();
    assert_eq!(*log.borrow(), ["delete", "dismiss"]);
    assert!(closed(&mut robot), "the dialog closes");
    assert_eq!(slot.borrow().map(|open| open.get()), Some(false));
}

#[test]
fn a_confirmation_sits_above_a_wide_control_and_beside_a_small_one() {
    let (mut wide, ..) = confirmation(Alignment::CENTER, 300.0);
    let title = node(&mut wide, TITLE).layout_bounds;
    let delete = node(&mut wide, "Delete").layout_bounds;
    // The source is 44 points tall in the middle of the window.
    let source_top = 400.0 - 22.0;
    assert!(
        delete.y + delete.height < source_top && title.y < delete.y,
        "the card sits above the wide control: title {title:?}, delete {delete:?}"
    );

    let (mut near_left, ..) = confirmation(Alignment::TOP_START, 44.0);
    let delete = node(&mut near_left, "Delete").layout_bounds;
    assert!(
        delete.x > 24.0 + 44.0,
        "the card sits right of a small control near the left side: delete {delete:?}"
    );

    let (mut near_right, ..) = confirmation(Alignment::BOTTOM_END, 44.0);
    let delete = node(&mut near_right, "Delete").layout_bounds;
    assert!(
        delete.x + delete.width < 400.0 - 24.0 - 44.0,
        "the card sits left of a small control near the right side: delete {delete:?}"
    );
}

#[test]
fn a_tap_outside_or_the_back_gesture_runs_cancel_and_closes() {
    let (mut tapped, log, _) = confirmation(Alignment::CENTER, 300.0);
    assert!(!closed(&mut tapped));
    tapped.click_at(200.0, 760.0);
    assert_eq!(*log.borrow(), ["cancel", "dismiss"]);
    assert!(closed(&mut tapped), "a tap outside the card closes it");

    let (mut backed, log, _) = confirmation(Alignment::CENTER, 300.0);
    assert!(backed.shell_mut().dismiss_top_modal());
    backed.wait_for_idle();
    assert_eq!(*log.borrow(), ["cancel", "dismiss"]);
    assert!(closed(&mut backed), "the back gesture closes the dialog");
}

#[test]
fn a_tap_on_the_card_keeps_it_open() {
    let (mut robot, log, _) = confirmation(Alignment::CENTER, 300.0);
    let title = node(&mut robot, TITLE).layout_bounds;
    robot.click_at(title.x + title.width * 0.5, title.y + title.height * 0.5);
    assert!(log.borrow().is_empty());
    assert!(!closed(&mut robot), "a tap on the card leaves it open");
}

#[test]
fn a_popover_shows_its_content_only_while_presented() {
    let presented: Rc<RefCell<Option<MutableState<bool>>>> = Rc::default();
    let captured = Rc::clone(&presented);
    let mut robot = create_headless_robot_test(400, 800, move || {
        let captured = Rc::clone(&captured);
        LiquidTheme(LiquidThemeSpec::default(), move || {
            let shown = rememberMutableStateOf(|| false);
            *captured.borrow_mut() = Some(shown);
            LiquidPopover(
                Modifier::empty().width(44.0).height(44.0),
                shown.get(),
                move || shown.set(false),
                || {},
                || {
                    Text(
                        "Popover body",
                        Modifier::empty().padding(20.0),
                        TextStyle::default(),
                    );
                },
            );
        });
    });
    robot.shell_mut().set_semantics_enabled(true);
    robot.wait_for_idle();
    let has_body = |robot: &mut Robot| {
        nodes(robot)
            .iter()
            .any(|node| node.label.as_deref() == Some("Popover body"))
    };
    assert!(
        !has_body(&mut robot),
        "nothing shows before it is presented"
    );
    presented.borrow().expect("composed").set(true);
    robot.wait_for_idle();
    assert!(has_body(&mut robot), "the content shows while presented");
    assert!(robot.shell_mut().dismiss_top_modal());
    robot.wait_for_idle();
    assert!(!has_body(&mut robot), "the back gesture dismisses it");
}
