//! Arrow keys move the caret the way the text is laid out: up and down by
//! wrapped line, Home and End to a wrapped line's ends, and left and right
//! by the paragraph's reading direction.

use cranpose_ui::{BasicTextFieldOptions, KeyCode, Modifiers};

use crate::support_text_field::{COMMAND, FocusedField, SHIFT};

/// One paragraph long enough to wrap in the 300 px field.
const PARAGRAPH: &str = "one two three four five six seven eight nine ten eleven twelve \
                         thirteen fourteen fifteen sixteen seventeen eighteen nineteen twenty";
const HEBREW: &str = "\u{5E9}\u{5DC}\u{5D5}\u{5DD}";

fn focused(text: &'static str) -> FocusedField {
    FocusedField::new(text, BasicTextFieldOptions::default())
}

fn caret(field: &mut FocusedField) -> usize {
    field.selection().1
}

#[test]
fn down_and_up_follow_wrapped_lines() {
    let mut field = focused(PARAGRAPH);
    field.press(KeyCode::Home, COMMAND);
    // The paragraph has no line break: a move by hard line went to its end.
    field.press(KeyCode::ArrowDown, Modifiers::NONE);
    let second_line = caret(&mut field);
    assert!(
        second_line > 0 && second_line < PARAGRAPH.len(),
        "Down went to the next wrapped line, at {second_line}"
    );
    field.press(KeyCode::ArrowUp, Modifiers::NONE);
    assert_eq!(caret(&mut field), 0);
}

#[test]
fn home_and_end_go_to_the_ends_of_a_wrapped_line() {
    let mut field = focused(PARAGRAPH);
    field.press(KeyCode::Home, Modifiers::NONE);
    let last_line = caret(&mut field);
    assert!(
        last_line > 0,
        "Home from the end stopped at the last wrapped line's start, not the text's"
    );
    field
        .press(KeyCode::Home, COMMAND)
        .press(KeyCode::End, Modifiers::NONE);
    let line_end = caret(&mut field);
    assert!(
        line_end > 0 && line_end < PARAGRAPH.len(),
        "End stopped at the first line's end, at {line_end}"
    );
    field.press(KeyCode::Home, Modifiers::NONE);
    assert_eq!(caret(&mut field), 0);
}

#[test]
fn left_and_right_follow_the_reading_direction() {
    let mut hebrew = focused(HEBREW);
    hebrew.press(KeyCode::ArrowRight, Modifiers::NONE);
    assert_eq!(
        caret(&mut hebrew),
        HEBREW.len() - 2,
        "Right moves back in a right-to-left paragraph"
    );
    hebrew.press(KeyCode::ArrowLeft, Modifiers::NONE);
    assert_eq!(caret(&mut hebrew), HEBREW.len());

    let mut latin = focused("abc");
    latin.press(KeyCode::ArrowLeft, Modifiers::NONE);
    assert_eq!(
        caret(&mut latin),
        2,
        "Left moves back in a left-to-right one"
    );
}

#[test]
fn shift_selection_follows_the_reading_direction() {
    let mut hebrew = focused(HEBREW);
    hebrew.press(KeyCode::ArrowRight, SHIFT);
    assert_eq!(hebrew.selection(), (HEBREW.len() - 2, HEBREW.len()));
}
