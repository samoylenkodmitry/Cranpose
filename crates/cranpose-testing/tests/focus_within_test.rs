//! `FocusWithin` through the headless robot: the observer hears when focus
//! moves onto a control inside it and when it leaves them all, not when it
//! moves between them, and Tab does not stop on the observer itself.

use std::{cell::RefCell, rc::Rc};

use cranpose_testing::{RobotTestRule, TestRenderer, create_headless_robot_test};
use cranpose_ui::{
    Box, BoxSpec, Column, ColumnSpec, FocusRequester, FocusWithin, KeyCode, KeyEvent, Modifier,
    Size,
};

struct Screen {
    robot: RobotTestRule<TestRenderer>,
    heard: Rc<RefCell<Vec<bool>>>,
    /// Two controls inside the observer, then one after it.
    controls: [FocusRequester; 3],
}

fn screen() -> Screen {
    let heard = Rc::new(RefCell::new(Vec::new()));
    let controls = [
        FocusRequester::new(),
        FocusRequester::new(),
        FocusRequester::new(),
    ];
    let (recorded, built) = (Rc::clone(&heard), controls.clone());
    let mut robot = create_headless_robot_test(300, 300, move || {
        let recorded = Rc::clone(&recorded);
        let built = built.clone();
        Column(Modifier::empty(), ColumnSpec::default(), move || {
            let recorded = Rc::clone(&recorded);
            let inside = [built[0].clone(), built[1].clone()];
            FocusWithin(
                move |focused| recorded.borrow_mut().push(focused),
                move || {
                    for requester in &inside {
                        Control(requester);
                    }
                },
            );
            Control(&built[2]);
        });
    });
    robot.wait_for_idle();
    Screen {
        robot,
        heard,
        controls,
    }
}

#[allow(non_snake_case)]
fn Control(requester: &FocusRequester) {
    Box(
        Modifier::empty()
            .size(Size::new(100.0, 40.0))
            .focus_requester(requester)
            .focus_target(),
        BoxSpec::default(),
        || {},
    );
}

impl Screen {
    fn focus(&mut self, control: usize) {
        self.controls[control]
            .request_focus()
            .expect("the control is composed");
        self.robot.wait_for_idle();
    }

    fn tab(&mut self) {
        assert!(
            self.robot
                .shell_mut()
                .on_key_event(&KeyEvent::key_down(KeyCode::Tab, "\t"))
        );
        self.robot.wait_for_idle();
    }
}

#[test]
fn the_observer_hears_focus_enter_and_leave_but_not_move_inside() {
    let mut screen = screen();
    screen.focus(0);
    screen.focus(1);
    assert_eq!(*screen.heard.borrow(), [true]);
    screen.focus(2);
    assert_eq!(*screen.heard.borrow(), [true, false]);
}

#[test]
fn tab_does_not_stop_on_the_observer() {
    let mut screen = screen();
    screen.tab();
    screen.tab();
    assert_eq!(*screen.heard.borrow(), [true], "two stops inside");
    screen.tab();
    assert_eq!(*screen.heard.borrow(), [true, false], "then the one after");
}
