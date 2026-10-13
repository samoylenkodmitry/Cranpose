//! A text field's input transformation changes or rejects the edits a
//! person makes, through the public shell input path, and leaves the app's
//! own edits alone.

use cranpose_foundation::text::InputTransformation;
use cranpose_ui::{BasicTextFieldOptions, KeyCode, Modifiers};

use crate::support_text_field::FocusedField;

fn field(text: &'static str, transformation: InputTransformation) -> FocusedField {
    FocusedField::new(
        text,
        BasicTextFieldOptions {
            input_transformation: Some(transformation),
            ..BasicTextFieldOptions::default()
        },
    )
}

fn digits_only() -> InputTransformation {
    InputTransformation::by_value(|current, proposed| {
        (!proposed.chars().all(|c| c.is_ascii_digit())).then(|| current.to_owned())
    })
}

#[test]
fn a_length_limit_rejects_typing_pasting_and_composing_past_it() {
    let mut limited = field("abc", InputTransformation::max_length(4));
    limited.type_text("de");
    assert_eq!(limited.text(), "abcd", "the fifth character was refused");
    assert!(limited.shell.on_paste("xy"));
    assert_eq!(limited.text(), "abcd");
    assert!(limited.shell.on_ime_preedit("xy", None));
    assert_eq!(
        limited.text(),
        "abcd",
        "composing past the limit was refused"
    );
    assert_eq!(limited.selection(), (4, 4));

    limited.press(KeyCode::Backspace, Modifiers::NONE);
    limited.type_text("\u{1F1F7}\u{1F1F8}");
    assert_eq!(
        limited.text(),
        "abc\u{1F1F7}\u{1F1F8}",
        "a flag is one character"
    );
}

#[test]
fn a_filter_keeps_only_the_edits_it_accepts() {
    let mut digits = field("12", digits_only());
    digits.type_text("3a4");
    assert_eq!(digits.text(), "1234");
    assert!(digits.shell.on_paste("5x"));
    assert_eq!(digits.text(), "1234", "a paste with a letter was refused");
    digits.press(KeyCode::Backspace, Modifiers::NONE);
    assert_eq!(digits.text(), "123", "deleting digits leaves digits");
}

#[test]
fn a_replacement_puts_the_caret_after_it() {
    let upper = InputTransformation::by_value(|_, proposed| {
        let upper = proposed.to_uppercase();
        (upper != proposed).then_some(upper)
    });
    let mut field = field("ab", upper);
    field.press(KeyCode::Home, Modifiers::NONE).type_text("c");
    assert_eq!(field.text(), "CAB");
    assert_eq!(field.selection(), (3, 3));
}

#[test]
fn transformations_chain_in_order() {
    let mut field = field("", digits_only().then(InputTransformation::max_length(3)));
    field.type_text("1a2345");
    assert_eq!(field.text(), "123");
}

#[test]
fn the_app_edits_past_the_transformation() {
    let mut field = field("12", InputTransformation::max_length(2));
    assert!(field.state().set_text("hello"));
    assert_eq!(field.text(), "hello");
    field.press(KeyCode::Backspace, Modifiers::NONE);
    assert_eq!(
        field.text(),
        "hell",
        "a deletion that keeps the text over the limit passes"
    );
}
