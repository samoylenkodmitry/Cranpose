// A text field on a TV: a D-pad's arrows move focus onto it and away from it
// without opening the software keyboard, and its center button opens the
// keyboard. Elsewhere arrows on a focused field move its caret.

use std::{
    cell::{Cell, RefCell},
    rc::Rc,
};

use cranpose_app_shell::AppShell;
use cranpose_core::{location_key, remember};
use cranpose_testing::robot::TestRenderer;
use cranpose_ui::{
    BasicTextField, Box, BoxSpec, Column, ColumnSpec, Key, KeyEvent, KeyEventType, Modifier,
    PlatformTextInputHandler, Size, TextFieldState,
};

#[derive(Default)]
struct Keyboard {
    shown: Cell<u32>,
}

impl PlatformTextInputHandler for Keyboard {
    fn show_keyboard(&self) {
        self.shown.set(self.shown.get() + 1);
    }

    fn hide_keyboard(&self) {}
}

type Focused = Rc<RefCell<Option<&'static str>>>;

fn button(name: &'static str, focused: &Focused) {
    let focused = Rc::clone(focused);
    Box(
        Modifier::empty()
            .size(Size::new(200.0, 40.0))
            .on_focus_changed(move |state| {
                if state.is_focused() {
                    *focused.borrow_mut() = Some(name);
                }
            }),
        BoxSpec::default(),
        || {},
    );
}

struct Screen {
    shell: AppShell<TestRenderer>,
    focused: Focused,
    keyboard: Rc<Keyboard>,
}

impl Screen {
    // A button above a text field holding "abc" and a button below it, in a
    // window that a D-pad drives when `dpad` is set.
    fn new(dpad: bool) -> Self {
        let focused = Focused::default();
        let seen = Rc::clone(&focused);
        let mut shell = AppShell::new(
            TestRenderer::default(),
            location_key(file!(), line!(), column!()),
            move || {
                let seen = Rc::clone(&seen);
                Column(Modifier::empty(), ColumnSpec::default(), move || {
                    button("above", &seen);
                    let state = remember(|| TextFieldState::new("abc")).with(|state| *state);
                    BasicTextField(
                        state,
                        Modifier::empty().size(Size::new(200.0, 40.0)),
                        Default::default(),
                    );
                    button("below", &seen);
                });
            },
        );
        shell.set_viewport(300.0, 300.0);
        shell.set_buffer_size(300, 300);
        let keyboard = Rc::new(Keyboard::default());
        shell.set_platform_text_input(Rc::clone(&keyboard) as Rc<dyn PlatformTextInputHandler>);
        if dpad {
            shell.set_dpad_navigation(true);
        }
        shell.update();
        Self {
            shell,
            focused,
            keyboard,
        }
    }

    fn press(&mut self, key: Key) {
        for type_ in [KeyEventType::KeyDown, KeyEventType::KeyUp] {
            let event = KeyEvent::new(key, "", Default::default(), type_);
            self.shell.on_key_event(&event);
            self.shell.update();
        }
    }

    // The focused button, or "field" while the text field holds focus.
    fn focused(&mut self) -> Option<&'static str> {
        if self.shell.ime_editor_state().is_some() {
            return Some("field");
        }
        *self.focused.borrow()
    }

    fn caret(&mut self) -> Option<usize> {
        self.shell
            .ime_editor_state()
            .map(|editor| editor.selection_start)
    }
}

#[test]
fn a_dpad_walks_through_a_text_field_and_opens_its_keyboard_on_center() {
    let mut screen = Screen::new(true);
    assert_eq!(screen.focused(), Some("above"), "a TV starts with focus");
    screen.press(Key::DirectionDown);
    assert_eq!(screen.focused(), Some("field"));
    assert_eq!(
        screen.keyboard.shown.get(),
        0,
        "an arrow onto the field leaves the keyboard closed"
    );
    screen.press(Key::DirectionDown);
    assert_eq!(screen.focused(), Some("below"), "Down leaves the field");
    screen.press(Key::DirectionUp);
    screen.press(Key::DirectionCenter);
    assert_eq!(screen.focused(), Some("field"));
    assert_eq!(screen.keyboard.shown.get(), 1, "Center opens the keyboard");
    let caret = screen.caret();
    screen.press(Key::DirectionLeft);
    assert_eq!(
        screen.caret().map(|at| at + 1),
        caret,
        "with no target to its left, Left moves the caret"
    );
}

#[test]
fn without_a_dpad_arrows_stay_in_a_focused_text_field() {
    let mut screen = Screen::new(false);
    screen.press(Key::DirectionDown);
    screen.press(Key::DirectionDown);
    assert_eq!(screen.focused(), Some("field"));
    assert_eq!(screen.keyboard.shown.get(), 1);
    screen.press(Key::DirectionDown);
    assert_eq!(
        screen.focused(),
        Some("field"),
        "Down moves the caret, not focus"
    );
}
