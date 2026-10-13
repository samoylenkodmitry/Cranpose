//! Keyboard editing moves and deletes whole user-perceived characters
//! (extended grapheme clusters), as Compose's `BreakIterator` character
//! instance does, through the public shell input path.

use cranpose_ui::{BasicTextFieldOptions, KeyCode, Modifiers};

use crate::support_text_field::{COMMAND, FocusedField, SHIFT};

const FLAG: &str = "\u{1F1F7}\u{1F1F8}";
const TECHNOLOGIST: &str = "\u{1F469}\u{200D}\u{1F4BB}";
const THUMB_WITH_TONE: &str = "\u{1F44D}\u{1F3FD}";
const E_ACUTE: &str = "e\u{301}";
/// Devanagari "ni": a consonant and its vowel sign.
const NI: &str = "\u{928}\u{93F}";

fn focused(text: &'static str) -> FocusedField {
    FocusedField::new(text, BasicTextFieldOptions::default())
}

#[test]
fn backspace_deletes_the_whole_character_before_the_caret() {
    for cluster in [FLAG, TECHNOLOGIST, THUMB_WITH_TONE, E_ACUTE, NI] {
        let text: &'static str = format!("a{cluster}").leak();
        let mut field = focused(text);
        field.press(KeyCode::Backspace, Modifiers::NONE);
        assert_eq!(field.text(), "a", "{cluster:?}");
    }
}

#[test]
fn delete_removes_the_whole_character_after_the_caret() {
    for cluster in [FLAG, TECHNOLOGIST, THUMB_WITH_TONE, E_ACUTE, NI] {
        let text: &'static str = format!("{cluster}b").leak();
        let mut field = focused(text);
        field
            .press(KeyCode::Home, Modifiers::NONE)
            .press(KeyCode::Delete, Modifiers::NONE);
        assert_eq!(field.text(), "b", "{cluster:?}");
    }
}

#[test]
fn arrows_step_over_whole_characters() {
    let text: &'static str = format!("a{FLAG}{TECHNOLOGIST}b").leak();
    let mut field = focused(text);
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
    let mut field = focused(text);

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
    let mut field = focused(text);
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
    let mut field = focused("ab\n\u{65E5}\u{672C}\nxy");
    field
        .press(KeyCode::Home, COMMAND)
        .press(KeyCode::ArrowRight, Modifiers::NONE)
        .press(KeyCode::ArrowDown, Modifiers::NONE);
    let (second, _) = field.selection();
    assert!(
        [3, 6, 9].contains(&second),
        "the caret landed between characters of the second line, at {second}"
    );
    field.press(KeyCode::ArrowDown, Modifiers::NONE);
    let (third, _) = field.selection();
    assert!((10..=12).contains(&third), "the third line, at {third}");
    field
        .press(KeyCode::ArrowUp, Modifiers::NONE)
        .press(KeyCode::ArrowUp, Modifiers::NONE);
    assert_eq!(field.selection(), (1, 1), "back to the first column");

    let mut field = focused("ab\n\u{1F1F7}\u{1F1F8}x");
    field
        .press(KeyCode::Home, COMMAND)
        .press(KeyCode::ArrowRight, Modifiers::NONE)
        .press(KeyCode::ArrowDown, Modifiers::NONE);
    let (below, _) = field.selection();
    assert!(
        [3, 11, 12].contains(&below),
        "the caret landed inside the flag, at {below}"
    );
}
