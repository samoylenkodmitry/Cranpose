//! A software keyboard that records what the framework asks of it, and the
//! shell setup the text field tests share.

use std::{cell::RefCell, rc::Rc};

use cranpose_app_shell::AppShell;
use cranpose_core::location_key;
use cranpose_testing::robot::TestRenderer;
use cranpose_ui::PlatformTextInputHandler;

#[derive(Default)]
pub(crate) struct RecordingKeyboard {
    pub(crate) calls: RefCell<Vec<&'static str>>,
}

impl PlatformTextInputHandler for RecordingKeyboard {
    fn show_keyboard(&self) {
        self.calls.borrow_mut().push("show");
    }

    fn hide_keyboard(&self) {
        self.calls.borrow_mut().push("hide");
    }
}

/// A 320 × 200 shell around `content`, with a recording keyboard installed
/// and a first frame composed.
#[track_caller]
pub(crate) fn shell_with_keyboard(
    content: impl FnMut() + 'static,
) -> (AppShell<TestRenderer>, Rc<RecordingKeyboard>) {
    let caller = std::panic::Location::caller();
    let mut shell = AppShell::new(
        TestRenderer::default(),
        location_key(caller.file(), caller.line(), caller.column()),
        content,
    );
    shell.set_viewport(320.0, 200.0);
    shell.set_buffer_size(320, 200);
    let keyboard = Rc::new(RecordingKeyboard::default());
    shell.set_platform_text_input(keyboard.clone());
    shell.update();
    (shell, keyboard)
}
