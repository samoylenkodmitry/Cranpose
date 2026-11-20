//! The keyboard that a headset draws: whether it is open, the keys that it typed and how it
//! reaches the app's shell.
//!
//! A text field asks for the platform's software keyboard when it takes focus, as it does on
//! Android and iOS. A headset answers by opening the keyboard's panel. The keys do not call the
//! shell, because a key's click runs inside the shell's own pointer dispatch. A key pushes the key
//! events that a hardware keyboard would send, and the frame loop hands them to the shell after the
//! pointer's events, so the text reaches the field through the path of a hardware keyboard.

use std::{cell::RefCell, collections::VecDeque, fmt::Debug, rc::Rc};

use cranpose::{
    MutableState, mutableStateOf,
    prelude::{KeyEvent, PlatformTextInputHandler},
};
use cranpose_app_shell::AppShell;
use cranpose_render_common::Renderer;

use crate::space::Space;

#[derive(Clone)]
pub(crate) struct SoftKeyboard {
    inner: Rc<Inner>,
}

impl PartialEq for SoftKeyboard {
    fn eq(&self, other: &Self) -> bool {
        Rc::ptr_eq(&self.inner, &other.inner)
    }
}

struct Inner {
    // Whether the keyboard's panel shows. The panel composes when it changes.
    open: MutableState<bool>,
    // The key events that the keys typed and the shell has not taken yet
    keys: RefCell<VecDeque<KeyEvent>>,
}

impl SoftKeyboard {
    // A closed keyboard. Its state belongs to the composition's runtime that makes it.
    pub(crate) fn new() -> Self {
        Self {
            inner: Rc::new(Inner {
                open: mutableStateOf(false),
                keys: RefCell::new(VecDeque::new()),
            }),
        }
    }

    // Whether the keyboard shows. A composition that reads it composes again when it changes.
    pub(crate) fn is_open(&self) -> bool {
        self.inner.open.get()
    }

    // Closes the keyboard. The text field keeps its focus, and a tap on it opens the keyboard
    // again.
    pub(crate) fn hide(&self) {
        self.inner.open.set(false);
    }

    // Types `event` and lets the key go up, as a hardware keyboard's key does.
    pub(crate) fn type_key(&self, event: KeyEvent) {
        let up = KeyEvent::key_up(event.key());
        let mut keys = self.inner.keys.borrow_mut();
        keys.push_back(event);
        keys.push_back(up);
    }

    // The next key event for the shell.
    fn next_key(&self) -> Option<KeyEvent> {
        self.inner.keys.borrow_mut().pop_front()
    }

    // Makes the shell ask this keyboard for the keyboard that a focused text field wants.
    pub(crate) fn install<R>(&self, shell: &mut AppShell<R>)
    where
        R: Renderer,
        R::Error: Debug,
    {
        shell.set_platform_text_input(Rc::new(self.clone()));
    }

    // Runs once in each frame, after the pointer's events reached the shell. While the keyboard is
    // open, it finds the panel that holds the focused field, so that the keyboard stands below it.
    // Then it hands the typed keys to the shell.
    pub(crate) fn pump<R>(&self, shell: &mut AppShell<R>, space: &Space)
    where
        R: Renderer,
        R::Error: Debug,
    {
        if self.is_open()
            && let Some(bounds) = shell.ime_field_bounds()
        {
            space.set_field_panel(space.panel_at([
                bounds.x + bounds.width / 2.0,
                bounds.y + bounds.height / 2.0,
            ]));
        }
        while let Some(event) = self.next_key() {
            shell.on_key_event(&event);
        }
    }
}

impl PlatformTextInputHandler for SoftKeyboard {
    fn show_keyboard(&self) {
        self.inner.open.set(true);
    }

    fn hide_keyboard(&self) {
        self.hide();
    }
}

#[cfg(test)]
#[path = "tests/soft_keyboard_tests.rs"]
mod tests;
