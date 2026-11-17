// A focused text field that leaves the composition gives up focus, as it
// does in Jetpack Compose: no node holds focus afterwards and the platform's
// software keyboard closes. A field that stays keeps its focus when a
// sibling leaves.

use std::{cell::RefCell, rc::Rc};

use cranpose_core::{MutableState, key, remember, rememberMutableStateOf};
use cranpose_testing::{RobotTestRule, TestRenderer, create_headless_robot_test};
use cranpose_ui::{
    BasicTextFieldDecorated, BasicTextFieldOptions, BasicTextFieldWithOptions, Column, ColumnSpec,
    FocusRequester, Modifier, PlatformTextInputHandler, TextFieldState, active_focus_target,
    has_focused_field,
};

use crate::support_text_field::RecordingKeyboard;

fn field(state: TextFieldState, requester: &FocusRequester, decorated: bool) {
    let modifier = Modifier::empty()
        .width(200.0)
        .height(40.0)
        .focus_requester(requester);
    if decorated {
        BasicTextFieldDecorated(state, modifier, BasicTextFieldOptions::default(), |inner| {
            inner.inner_text_field();
        });
    } else {
        BasicTextFieldWithOptions(state, modifier, BasicTextFieldOptions::default());
    }
}

// A column of a field that can leave, followed by one that stays, with a
// keyboard that records what the framework asks of it.
struct Screen {
    robot: RobotTestRule<TestRenderer>,
    keyboard: Rc<RecordingKeyboard>,
    leaving: FocusRequester,
    staying: FocusRequester,
    present: MutableState<bool>,
}

impl Screen {
    fn new(decorated: bool) -> Self {
        let (leaving, staying) = (FocusRequester::new(), FocusRequester::new());
        let switch = Rc::new(RefCell::new(None));
        let (content_switch, content_leaving, content_staying) =
            (Rc::clone(&switch), leaving.clone(), staying.clone());
        let mut robot = create_headless_robot_test(320, 200, move || {
            let (switch, leaving, staying) = (
                Rc::clone(&content_switch),
                content_leaving.clone(),
                content_staying.clone(),
            );
            Column(Modifier::empty(), ColumnSpec::default(), move || {
                let present = rememberMutableStateOf(|| true);
                *switch.borrow_mut() = Some(present);
                // Each field keeps its own identity when the other one leaves.
                if present.get() {
                    key("leaving", || {
                        let state =
                            remember(|| TextFieldState::new("Leaving")).with(|state| *state);
                        field(state, &leaving, decorated);
                    });
                }
                key("staying", || {
                    let state = remember(|| TextFieldState::new("Staying")).with(|state| *state);
                    field(state, &staying, decorated);
                });
            });
        });
        let keyboard = Rc::new(RecordingKeyboard::default());
        robot
            .shell_mut()
            .set_platform_text_input(Rc::clone(&keyboard) as Rc<dyn PlatformTextInputHandler>);
        robot.wait_for_idle();
        let present = switch.borrow().expect("composed switch");
        Self {
            robot,
            keyboard,
            leaving,
            staying,
            present,
        }
    }

    fn remove_leaving_field(&mut self) {
        self.present.set(false);
        self.robot.wait_for_idle();
    }

    // The node that holds focus and whether a text field does.
    fn focus(&mut self) -> (Option<cranpose_core::NodeId>, bool) {
        self.robot
            .shell_mut()
            .app_context()
            .enter(|| (active_focus_target(), has_focused_field()))
    }
}

#[test]
fn the_keyboard_closes_when_the_focused_field_leaves() {
    for decorated in [false, true] {
        let mut screen = Screen::new(decorated);
        screen.leaving.request_focus().expect("field focus");
        assert_eq!(screen.keyboard.calls.borrow().as_slice(), &["show"]);

        screen.remove_leaving_field();

        // Read before `has_focused_field`, whose stale-focus check hides the
        // keyboard itself.
        assert_eq!(
            screen.keyboard.calls.borrow().as_slice(),
            &["show", "hide"],
            "decorated: {decorated}"
        );
        assert_eq!(screen.focus(), (None, false), "decorated: {decorated}");
        assert!(
            screen.robot.shell_mut().ime_editor_state().is_none(),
            "decorated: {decorated}"
        );
    }
}

#[test]
fn a_focused_field_keeps_focus_when_a_sibling_leaves() {
    for decorated in [false, true] {
        let mut screen = Screen::new(decorated);
        screen.staying.request_focus().expect("field focus");
        let focused = screen.focus();
        assert!(focused.0.is_some() && focused.1, "decorated: {decorated}");

        screen.remove_leaving_field();

        assert_eq!(screen.focus(), focused, "decorated: {decorated}");
        assert_eq!(
            screen
                .robot
                .shell_mut()
                .ime_editor_state()
                .expect("the staying field")
                .text,
            "Staying",
            "decorated: {decorated}"
        );
        assert_eq!(
            screen.keyboard.calls.borrow().as_slice(),
            &["show"],
            "decorated: {decorated}"
        );
    }
}
