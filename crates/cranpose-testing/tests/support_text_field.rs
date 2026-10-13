//! What the text field tests share: a software keyboard that records what
//! the framework asks of it, a shell around a field, and a focused field.

use std::{cell::RefCell, rc::Rc};

use cranpose_app_shell::AppShell;
use cranpose_core::{location_key, remember};
use cranpose_testing::robot::TestRenderer;
use cranpose_ui::{
    BasicTextFieldOptions, BasicTextFieldWithOptions, FocusRequester, KeyCode, KeyEvent, Modifier,
    Modifiers, PlatformTextInputHandler, TextFieldState,
};

pub(crate) const SHIFT: Modifiers = Modifiers {
    shift: true,
    ..Modifiers::NONE
};

/// The platform's command key: Cmd on macOS, Ctrl elsewhere.
pub(crate) const COMMAND: Modifiers = if cfg!(target_os = "macos") {
    Modifiers {
        meta: true,
        ..Modifiers::NONE
    }
} else {
    Modifiers {
        ctrl: true,
        ..Modifiers::NONE
    }
};

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

/// One focused text field in a shell, driven through the shell's key and
/// IME entry points.
pub(crate) struct FocusedField {
    pub(crate) shell: AppShell<TestRenderer>,
    state: Rc<std::cell::Cell<Option<TextFieldState>>>,
}

impl FocusedField {
    /// A focused field holding `text`, with `options` and the caret at its
    /// end.
    #[track_caller]
    pub(crate) fn new(text: &'static str, options: BasicTextFieldOptions) -> Self {
        let requester = FocusRequester::new();
        let state = Rc::new(std::cell::Cell::new(None));
        let (content_requester, content_state) = (requester.clone(), state.clone());
        let (shell, _keyboard) = shell_with_keyboard(move || {
            let field = remember(|| TextFieldState::new(text)).with(|state| *state);
            content_state.set(Some(field));
            BasicTextFieldWithOptions(
                field,
                Modifier::empty()
                    .width(300.0)
                    .height(80.0)
                    .focus_requester(&content_requester),
                options.clone(),
            );
        });
        requester.request_focus().expect("field focus");
        let mut field = Self { shell, state };
        field.press(KeyCode::End, Modifiers::NONE);
        field
    }

    /// The field's state, for edits the app makes.
    pub(crate) fn state(&self) -> TextFieldState {
        self.state.get().expect("composed field")
    }

    pub(crate) fn press(&mut self, key: KeyCode, modifiers: Modifiers) -> &mut Self {
        assert!(
            self.shell
                .on_key_event(&KeyEvent::key_down_with_modifiers(key, "", modifiers)),
            "the focused field takes {key:?}"
        );
        self
    }

    /// Types `text` one key at a time.
    pub(crate) fn type_text(&mut self, text: &str) -> &mut Self {
        for character in text.chars() {
            self.shell
                .on_key_event(&KeyEvent::key_down(KeyCode::Unknown, character.to_string()));
        }
        self
    }

    pub(crate) fn text(&mut self) -> String {
        self.shell.ime_editor_state().expect("focused field").text
    }

    /// The text the field shows, after the next frame lays it out.
    pub(crate) fn shown_text(&mut self) -> String {
        self.shell.update();
        self.shell.with_layout_tree(|tree| {
            fn find(node: &cranpose_ui::LayoutBox) -> Option<String> {
                node.node_data
                    .modifier_slices()
                    .text_content()
                    .map(|text| text.to_string())
                    .or_else(|| node.children.iter().find_map(find))
            }
            tree.and_then(|tree| find(tree.root())).unwrap_or_default()
        })
    }

    /// Taps the field just after `shown_prefix`, the start of what it shows.
    pub(crate) fn tap_after(&mut self, shown_prefix: &str) -> &mut Self {
        let x = self.shell.app_context().enter(|| {
            cranpose_ui::text::measure_text(
                &cranpose_ui::text::AnnotatedString::from(shown_prefix),
                &cranpose_ui::TextStyle::default(),
            )
            .width
        });
        assert!(self.shell.set_cursor(x, 10.0));
        assert!(self.shell.pointer_pressed());
        assert!(self.shell.pointer_released());
        self
    }

    /// The selection's start and end, lower first.
    pub(crate) fn selection(&mut self) -> (usize, usize) {
        let editor = self.shell.ime_editor_state().expect("focused field");
        (editor.selection_start, editor.selection_end)
    }
}
