//! A text field's keyboard options reach the platform editor, and its
//! action key, or Enter in a single-line field, runs the field's keyboard
//! action through the public shell input path.

use std::{cell::RefCell, rc::Rc};

use cranpose_app_shell::AppShell;
use cranpose_core::remember;
use cranpose_foundation::text::TextFieldLineLimits;
use cranpose_testing::robot::TestRenderer;
use cranpose_ui::{
    BasicTextFieldOptions, BasicTextFieldWithOptions, Column, ColumnSpec, FocusRequester,
    ImeAction, KeyCode, KeyEvent, KeyboardActionHandler, KeyboardOptions, KeyboardType, Modifier,
    TextFieldState,
};

use crate::support_text_field::{RecordingKeyboard, shell_with_keyboard};

/// How the test's handler answers an action.
#[derive(Clone, Copy, PartialEq)]
enum Handler {
    /// No handler: the action's default runs.
    None,
    /// Records the action and leaves the default undone.
    Records,
    /// Records the action and runs its default.
    RecordsAndDefaults,
}

struct Form {
    shell: AppShell<TestRenderer>,
    keyboard: Rc<RecordingKeyboard>,
    second: FocusRequester,
    actions: Rc<RefCell<Vec<ImeAction>>>,
}

impl Form {
    /// Two fields: the first with `options` and `lines`, answered by
    /// `handler`, the second a plain single-line field. The first has focus.
    fn new(options: KeyboardOptions, lines: TextFieldLineLimits, handler: Handler) -> Self {
        let first = FocusRequester::new();
        let second = FocusRequester::new();
        let actions = Rc::new(RefCell::new(Vec::new()));
        let (content_first, content_second, content_actions) =
            (first.clone(), second.clone(), actions.clone());
        let (mut shell, keyboard) = shell_with_keyboard(move || {
            let (first, second, actions) = (
                content_first.clone(),
                content_second.clone(),
                content_actions.clone(),
            );
            Column(Modifier::empty(), ColumnSpec::default(), move || {
                let state = remember(|| TextFieldState::new("ab")).with(|state| *state);
                let other = remember(|| TextFieldState::new("")).with(|state| *state);
                let actions = actions.clone();
                let on_keyboard_action = (handler != Handler::None).then(|| {
                    KeyboardActionHandler::new(move |scope| {
                        actions.borrow_mut().push(scope.action());
                        if handler == Handler::RecordsAndDefaults {
                            scope.perform_default_action();
                        }
                    })
                });
                BasicTextFieldWithOptions(
                    state,
                    Modifier::empty()
                        .width(200.0)
                        .height(40.0)
                        .focus_requester(&first),
                    BasicTextFieldOptions {
                        line_limits: lines,
                        keyboard_options: options,
                        on_keyboard_action,
                        ..Default::default()
                    },
                );
                BasicTextFieldWithOptions(
                    other,
                    Modifier::empty()
                        .width(200.0)
                        .height(40.0)
                        .focus_requester(&second),
                    BasicTextFieldOptions::default(),
                );
            });
        });
        first.request_focus().expect("first field focus");
        shell.on_key_event(&KeyEvent::key_down(KeyCode::End, ""));
        keyboard.calls.borrow_mut().clear();
        Self {
            shell,
            keyboard,
            second,
            actions,
        }
    }

    fn press_enter(&mut self) -> bool {
        self.shell
            .on_key_event(&KeyEvent::key_down(KeyCode::Enter, "\n"))
    }

    fn text(&mut self) -> String {
        self.shell.ime_editor_state().expect("focused field").text
    }

    /// The first field holds "ab", the second nothing.
    fn first_has_focus(&mut self) -> bool {
        self.text() == "ab"
    }

    fn second_has_focus(&mut self) -> bool {
        self.text().is_empty()
    }
}

fn action(ime_action: ImeAction) -> KeyboardOptions {
    KeyboardOptions {
        ime_action,
        ..KeyboardOptions::default()
    }
}

#[test]
fn enter_in_a_single_line_field_runs_done_and_keeps_focus() {
    let mut form = Form::new(
        KeyboardOptions::default(),
        TextFieldLineLimits::SingleLine,
        Handler::None,
    );
    assert!(form.press_enter(), "the field takes Enter");
    assert_eq!(
        form.text(),
        "ab",
        "Enter adds no line to a single-line field"
    );
    assert_eq!(form.keyboard.calls.borrow().as_slice(), &["hide"]);
    assert!(
        form.first_has_focus(),
        "Done hides the keyboard, not the focus"
    );
}

#[test]
fn next_and_previous_move_focus_between_fields() {
    let mut form = Form::new(
        action(ImeAction::Next),
        TextFieldLineLimits::SingleLine,
        Handler::None,
    );
    assert!(form.press_enter());
    assert!(form.second_has_focus(), "Next moves to the following field");

    let mut form = Form::new(
        action(ImeAction::Previous),
        TextFieldLineLimits::SingleLine,
        Handler::None,
    );
    form.second.request_focus().expect("second field focus");
    assert!(form.shell.on_ime_action(ImeAction::Previous));
    assert!(form.first_has_focus(), "Previous moves to the field before");
}

#[test]
fn a_handler_gets_the_action_and_runs_the_default_only_when_it_asks() {
    let mut form = Form::new(
        action(ImeAction::Next),
        TextFieldLineLimits::SingleLine,
        Handler::Records,
    );
    assert!(form.press_enter());
    assert_eq!(form.actions.borrow().as_slice(), &[ImeAction::Next]);
    assert!(
        form.first_has_focus(),
        "the handler left the default undone"
    );

    let mut form = Form::new(
        action(ImeAction::Next),
        TextFieldLineLimits::SingleLine,
        Handler::RecordsAndDefaults,
    );
    assert!(form.press_enter());
    assert_eq!(form.actions.borrow().as_slice(), &[ImeAction::Next]);
    assert!(form.second_has_focus(), "the handler ran the default");
}

#[test]
fn the_action_key_of_a_multi_line_field_adds_a_line_unless_it_has_an_action() {
    let mut form = Form::new(
        KeyboardOptions::default(),
        TextFieldLineLimits::default(),
        Handler::Records,
    );
    assert!(form.shell.on_ime_action(ImeAction::Default));
    assert_eq!(form.text(), "ab\n");
    assert!(form.actions.borrow().is_empty());

    let mut form = Form::new(
        action(ImeAction::Send),
        TextFieldLineLimits::default(),
        Handler::Records,
    );
    assert!(form.shell.on_ime_action(ImeAction::Default));
    assert_eq!(form.text(), "ab", "Send adds no line");
    assert_eq!(form.actions.borrow().as_slice(), &[ImeAction::Send]);
    assert!(form.press_enter());
    assert_eq!(
        form.text(),
        "ab\n",
        "a hardware Enter still adds a line to a multi-line field"
    );
}

#[test]
fn a_single_line_field_without_an_action_leaves_enter_unhandled() {
    let mut form = Form::new(
        action(ImeAction::None),
        TextFieldLineLimits::SingleLine,
        Handler::Records,
    );
    assert!(!form.press_enter());
    assert!(form.actions.borrow().is_empty());
    assert_eq!(form.text(), "ab");
}

#[test]
fn an_action_finishes_the_composition_first() {
    let mut form = Form::new(
        action(ImeAction::Search),
        TextFieldLineLimits::SingleLine,
        Handler::Records,
    );
    assert!(form.shell.on_ime_preedit("cd", None));
    assert!(form.shell.on_ime_action(ImeAction::Search));
    let editor = form.shell.ime_editor_state().expect("focused field");
    assert_eq!(editor.text, "abcd");
    assert_eq!(editor.composition, None, "the composed word was committed");
    assert_eq!(form.actions.borrow().as_slice(), &[ImeAction::Search]);
}

#[test]
fn the_platform_editor_reads_the_field_keyboard_options() {
    let options = KeyboardOptions {
        keyboard_type: KeyboardType::Email,
        auto_correct: false,
        ime_action: ImeAction::Go,
        ..KeyboardOptions::default()
    };
    let mut form = Form::new(options, TextFieldLineLimits::SingleLine, Handler::None);
    let editor = form.shell.ime_editor_state().expect("focused field");
    assert_eq!(editor.keyboard_options, options);
    assert!(editor.single_line);
}
