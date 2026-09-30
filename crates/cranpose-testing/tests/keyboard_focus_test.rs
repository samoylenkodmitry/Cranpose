use std::{cell::RefCell, rc::Rc};

use cranpose_app_shell::AppShell;
use cranpose_core::{location_key, remember, rememberMutableStateOf};
use cranpose_testing::robot::TestRenderer;
use cranpose_ui::{
    BasicTextFieldDecorated, BasicTextFieldOptions, BasicTextFieldWithOptions, Box, BoxSpec,
    Column, ColumnSpec, FocusRequester, Modifier, PlatformTextInputHandler, TextFieldState,
};

#[derive(Default)]
struct Keyboard {
    calls: RefCell<Vec<&'static str>>,
}

impl PlatformTextInputHandler for Keyboard {
    fn show_keyboard(&self) {
        self.calls.borrow_mut().push("show");
    }

    fn hide_keyboard(&self) {
        self.calls.borrow_mut().push("hide");
    }
}

fn check_keyboard_focus(decorated: bool, show_keyboard_on_focus: bool) {
    let requester = FocusRequester::new();
    let other = FocusRequester::new();
    let content_requester = requester.clone();
    let content_other = other.clone();
    let preference = Rc::new(RefCell::new(None));
    let content_preference = preference.clone();
    let mut shell = AppShell::new(
        TestRenderer::default(),
        location_key(file!(), line!(), column!()),
        move || {
            let requester = content_requester.clone();
            let other = content_other.clone();
            let preference = content_preference.clone();
            Column(Modifier::empty(), ColumnSpec::default(), move || {
                let state = remember(|| TextFieldState::new("Search")).with(|state| *state);
                let show_keyboard = rememberMutableStateOf(move || show_keyboard_on_focus);
                *preference.borrow_mut() = Some(show_keyboard);
                let modifier = Modifier::empty()
                    .width(200.0)
                    .height(40.0)
                    .focus_requester(&requester);
                let options = if show_keyboard.get() {
                    BasicTextFieldOptions::default()
                } else {
                    BasicTextFieldOptions {
                        show_keyboard_on_focus: false,
                        ..Default::default()
                    }
                };
                if decorated {
                    BasicTextFieldDecorated(state, modifier, options, |inner| {
                        inner.inner_text_field();
                    });
                } else {
                    BasicTextFieldWithOptions(state, modifier, options);
                }
                Box(
                    Modifier::empty()
                        .size(cranpose_ui::Size::new(200.0, 40.0))
                        .focus_requester(&other)
                        .focus_target(),
                    BoxSpec::default(),
                    || {},
                );
            });
        },
    );
    shell.set_viewport(320.0, 200.0);
    shell.set_buffer_size(320, 200);
    let keyboard = Rc::new(Keyboard::default());
    shell.set_platform_text_input(keyboard.clone());
    shell.update();

    requester.request_focus().expect("programmatic field focus");
    let expected: &[&str] = if show_keyboard_on_focus {
        &["show"]
    } else {
        &[]
    };
    assert_eq!(
        keyboard.calls.borrow().as_slice(),
        expected,
        "programmatic focus must respect keyboard visibility"
    );
    assert!(
        shell.on_key_event(&cranpose_ui::KeyEvent::key_down(
            cranpose_ui::KeyCode::End,
            ""
        )),
        "the focused field must accept physical keyboard input"
    );
    assert!(shell.on_key_event(&cranpose_ui::KeyEvent::key_down(
        cranpose_ui::KeyCode::A,
        "a"
    )));
    let editor = shell.ime_editor_state().expect("focused editable field");
    assert_eq!(
        editor.text, "Searcha",
        "hardware input edits the focused field"
    );
    assert_eq!(
        (editor.selection_start, editor.selection_end),
        (7, 7),
        "typing advances the caret without requesting the software keyboard"
    );

    other.request_focus().expect("focus another control");
    let expected: &[&str] = if show_keyboard_on_focus {
        &["show", "hide"]
    } else {
        &[]
    };
    assert_eq!(
        keyboard.calls.borrow().as_slice(),
        expected,
        "blur must hide only a keyboard that was requested"
    );
    keyboard.calls.borrow_mut().clear();
    shell.update();
    for _ in 0..2 {
        assert!(shell.set_cursor(20.0, 20.0));
        assert!(shell.pointer_pressed());
        assert!(shell.pointer_released());
    }
    assert_eq!(
        keyboard.calls.borrow().as_slice(),
        &["show", "show"],
        "first and repeated taps must explicitly request the keyboard"
    );
    other
        .request_focus()
        .expect("move focus away from the field");
    assert_eq!(
        keyboard.calls.borrow().last(),
        Some(&"hide"),
        "losing field focus closes the requested keyboard"
    );

    preference
        .borrow()
        .expect("composed keyboard preference")
        .set(!show_keyboard_on_focus);
    shell.update();
    keyboard.calls.borrow_mut().clear();
    requester
        .request_focus()
        .expect("focus after updating the option");
    let expected: &[&str] = if show_keyboard_on_focus {
        &[]
    } else {
        &["show"]
    };
    assert_eq!(
        keyboard.calls.borrow().as_slice(),
        expected,
        "the existing field must use its updated focus option"
    );

    keyboard.calls.borrow_mut().clear();
    preference
        .borrow()
        .expect("composed keyboard preference")
        .set(show_keyboard_on_focus);
    shell.update();
    let expected: &[&str] = if show_keyboard_on_focus {
        &["show"]
    } else {
        &[]
    };
    assert_eq!(
        keyboard.calls.borrow().as_slice(),
        expected,
        "enabling the keyboard while focused requests it immediately; disabling does not hide an existing session"
    );
    assert!(
        shell.ime_editor_state().is_some(),
        "changing the option preserves editing focus"
    );
}

#[test]
fn programmatic_focus_can_keep_the_keyboard_hidden() {
    for decorated in [false, true] {
        check_keyboard_focus(decorated, false);
    }
}

#[test]
fn focus_opens_the_keyboard_by_default() {
    for decorated in [false, true] {
        check_keyboard_focus(decorated, true);
    }
}
