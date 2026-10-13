//! Keyboard editing moves and deletes whole user-perceived characters
//! (extended grapheme clusters), as Compose's `BreakIterator` character
//! instance does, through the public shell input path.

use cranpose_app_shell::AppShell;
use cranpose_core::{location_key, remember};
use cranpose_testing::robot::TestRenderer;
use cranpose_ui::{
    BasicTextFieldOptions, BasicTextFieldWithOptions, FocusRequester, KeyCode, KeyEvent, Modifier,
    Modifiers, TextFieldState,
};

const FLAG: &str = "\u{1F1F7}\u{1F1F8}";
const TECHNOLOGIST: &str = "\u{1F469}\u{200D}\u{1F4BB}";
const THUMB_WITH_TONE: &str = "\u{1F44D}\u{1F3FD}";
const E_ACUTE: &str = "e\u{301}";
/// Devanagari "ni": a consonant and its vowel sign.
const NI: &str = "\u{928}\u{93F}";

const SHIFT: Modifiers = Modifiers {
    shift: true,
    ..Modifiers::NONE
};
const COMMAND: Modifiers = if cfg!(target_os = "macos") {
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

struct Field {
    shell: AppShell<TestRenderer>,
}

impl Field {
    /// A focused field holding `text`, with the caret at its end.
    fn new(text: &'static str) -> Self {
        let requester = FocusRequester::new();
        let content_requester = requester.clone();
        let mut shell = AppShell::new(
            TestRenderer::default(),
            location_key(file!(), line!(), column!()),
            move || {
                let state = remember(|| TextFieldState::new(text)).with(|state| *state);
                BasicTextFieldWithOptions(
                    state,
                    Modifier::empty()
                        .width(300.0)
                        .height(80.0)
                        .focus_requester(&content_requester),
                    BasicTextFieldOptions::default(),
                );
            },
        );
        shell.set_viewport(320.0, 100.0);
        shell.set_buffer_size(320, 100);
        shell.update();
        requester.request_focus().expect("field focus");
        let mut field = Self { shell };
        field.press(KeyCode::End, Modifiers::NONE);
        field
    }

    fn press(&mut self, key: KeyCode, modifiers: Modifiers) -> &mut Self {
        assert!(
            self.shell
                .on_key_event(&KeyEvent::key_down_with_modifiers(key, "", modifiers)),
            "the focused field takes {key:?}"
        );
        self
    }

    fn text(&mut self) -> String {
        self.shell.ime_editor_state().expect("focused field").text
    }

    /// The selection's start and end, lower first.
    fn selection(&mut self) -> (usize, usize) {
        let editor = self.shell.ime_editor_state().expect("focused field");
        (editor.selection_start, editor.selection_end)
    }
}

#[test]
fn backspace_deletes_the_whole_character_before_the_caret() {
    for cluster in [FLAG, TECHNOLOGIST, THUMB_WITH_TONE, E_ACUTE, NI] {
        let text: &'static str = format!("a{cluster}").leak();
        let mut field = Field::new(text);
        field.press(KeyCode::Backspace, Modifiers::NONE);
        assert_eq!(field.text(), "a", "{cluster:?}");
    }
}

#[test]
fn delete_removes_the_whole_character_after_the_caret() {
    for cluster in [FLAG, TECHNOLOGIST, THUMB_WITH_TONE, E_ACUTE, NI] {
        let text: &'static str = format!("{cluster}b").leak();
        let mut field = Field::new(text);
        field
            .press(KeyCode::Home, Modifiers::NONE)
            .press(KeyCode::Delete, Modifiers::NONE);
        assert_eq!(field.text(), "b", "{cluster:?}");
    }
}

#[test]
fn arrows_step_over_whole_characters() {
    let text: &'static str = format!("a{FLAG}{TECHNOLOGIST}b").leak();
    let mut field = Field::new(text);
    let after_flag = 1 + FLAG.len();
    let after_technologist = after_flag + TECHNOLOGIST.len();

    field.press(KeyCode::ArrowLeft, Modifiers::NONE);
    assert_eq!(field.selection(), (after_technologist, after_technologist));
    field.press(KeyCode::ArrowLeft, Modifiers::NONE);
    assert_eq!(field.selection(), (after_flag, after_flag));
    field.press(KeyCode::ArrowLeft, Modifiers::NONE);
    assert_eq!(field.selection(), (1, 1));
    field.press(KeyCode::ArrowRight, Modifiers::NONE);
    assert_eq!(field.selection(), (after_flag, after_flag));
}

#[test]
fn shift_arrows_select_whole_characters() {
    let text: &'static str = format!("a{THUMB_WITH_TONE}{E_ACUTE}").leak();
    let end = text.len();
    let mut field = Field::new(text);

    field.press(KeyCode::ArrowLeft, SHIFT);
    assert_eq!(field.selection(), (end - E_ACUTE.len(), end));
    field.press(KeyCode::ArrowLeft, SHIFT);
    assert_eq!(field.selection(), (1, end));
    field.press(KeyCode::ArrowRight, SHIFT);
    assert_eq!(field.selection(), (1 + THUMB_WITH_TONE.len(), end));
    field.press(KeyCode::Backspace, Modifiers::NONE);
    assert_eq!(field.text(), format!("a{THUMB_WITH_TONE}"));
}

#[test]
fn word_moves_keep_combining_marks_with_their_letter() {
    let text: &'static str = format!("caf{E_ACUTE} {NI}x").leak();
    let mut field = Field::new(text);
    let word_end = 3 + E_ACUTE.len();

    field
        .press(KeyCode::Home, Modifiers::NONE)
        .press(KeyCode::ArrowRight, COMMAND);
    assert_eq!(field.selection(), (word_end, word_end));
    field.press(KeyCode::ArrowRight, COMMAND);
    assert_eq!(field.selection(), (text.len(), text.len()));
    field.press(KeyCode::ArrowLeft, COMMAND);
    assert_eq!(field.selection(), (word_end + 1, word_end + 1));
}

#[test]
fn vertical_moves_keep_the_caret_on_character_boundaries() {
    let mut field = Field::new("ab\n\u{65E5}\u{672C}\n\u{1F1F7}\u{1F1F8}x");
    field
        .press(KeyCode::Home, COMMAND)
        .press(KeyCode::ArrowRight, Modifiers::NONE)
        .press(KeyCode::ArrowDown, Modifiers::NONE);
    assert_eq!(
        field.selection(),
        (6, 6),
        "one character into the second line"
    );
    field.press(KeyCode::ArrowDown, Modifiers::NONE);
    assert_eq!(
        field.selection(),
        (18, 18),
        "past the flag on the third line"
    );
    field
        .press(KeyCode::ArrowUp, Modifiers::NONE)
        .press(KeyCode::ArrowUp, Modifiers::NONE);
    assert_eq!(field.selection(), (1, 1), "back to the first column");
}
