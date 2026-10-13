//! A text field's output transformation changes what it shows and not its
//! text, and the caret, edits and taps map between the two, through the
//! public shell input path.

use cranpose_foundation::text::{OutputTransformation, TextFieldLineLimits};
use cranpose_ui::BasicTextFieldOptions;

use crate::support_text_field::FocusedField;

fn grouped() -> OutputTransformation {
    OutputTransformation::new(|text, changes| {
        for at in (3..text.len()).step_by(3) {
            changes.insert(at, " ");
        }
    })
}

fn field(text: &'static str, transformation: OutputTransformation) -> FocusedField {
    FocusedField::new(
        text,
        BasicTextFieldOptions {
            line_limits: TextFieldLineLimits::SingleLine,
            output_transformation: Some(transformation),
            ..BasicTextFieldOptions::default()
        },
    )
}

#[test]
fn the_field_shows_the_transformed_text_and_edits_the_text() {
    let mut field = field("123456", grouped());
    assert_eq!(field.shown_text(), "123 456");
    field.type_text("7");
    assert_eq!(field.text(), "1234567", "the text holds no spaces");
    assert_eq!(field.shown_text(), "123 456 7");
}

#[test]
fn a_tap_on_shown_text_places_the_caret_in_the_text() {
    let mut past_space = field("123456", grouped());
    past_space.shown_text();
    past_space.tap_after("123 4");
    assert_eq!(
        past_space.selection(),
        (4, 4),
        "after the 4, past the inserted space"
    );

    // A second tap on the same field would count as a double tap.
    let mut before_space = field("123456", grouped());
    before_space.shown_text();
    before_space.tap_after("123");
    assert_eq!(before_space.selection(), (3, 3));
}

#[test]
fn a_masked_field_shows_dots_and_keeps_the_typed_text() {
    let mut field = field("ab", OutputTransformation::mask('\u{2022}'));
    assert_eq!(field.shown_text(), "\u{2022}\u{2022}");
    field.type_text("c");
    assert!(field.shell.on_ime_preedit("d", None));
    assert_eq!(field.text(), "abcd");
    assert_eq!(field.shown_text(), "\u{2022}\u{2022}\u{2022}\u{2022}");
}
