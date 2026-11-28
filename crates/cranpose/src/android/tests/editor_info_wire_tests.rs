use cranpose_ui::{ImeAction, KeyboardCapitalization, KeyboardType};

use super::editor_info;

// Values of android.text.InputType and android.view.inputmethod.EditorInfo.
const TEXT: i32 = 0x1;
const TEXT_MULTI_LINE: i32 = 0x2_0001;
const TEXT_EMAIL: i32 = 0x21;
const TEXT_PASSWORD: i32 = 0x81;
const TEXT_VISIBLE_PASSWORD: i32 = 0x91;
const NUMBER: i32 = 0x2;
const NUMBER_DECIMAL: i32 = 0x2002;
const NUMBER_PASSWORD: i32 = 0x12;
const PHONE: i32 = 0x3;
const ACTION_SEARCH: i32 = 0x3;
const ACTION_SEND: i32 = 0x4;
const FLAG_NO_ENTER_ACTION: i32 = 0x4000_0000;

#[test]
fn a_field_names_the_board_it_wants() {
    let cases = [
        (TEXT, KeyboardType::Text),
        (TEXT_EMAIL, KeyboardType::Email),
        (TEXT_PASSWORD, KeyboardType::Password),
        (TEXT_VISIBLE_PASSWORD, KeyboardType::Password),
        (NUMBER, KeyboardType::Number),
        (NUMBER_DECIMAL, KeyboardType::Decimal),
        (NUMBER_PASSWORD, KeyboardType::NumberPassword),
        (PHONE, KeyboardType::Phone),
    ];
    for (input_type, expected) in cases {
        assert_eq!(
            editor_info(input_type, 0).keyboard_type,
            expected,
            "input type {input_type:#x}"
        );
    }
}

#[test]
fn the_action_key_follows_the_field() {
    assert_eq!(
        editor_info(TEXT, ACTION_SEARCH).ime_action,
        ImeAction::Search
    );
    assert_eq!(editor_info(TEXT, ACTION_SEND).ime_action, ImeAction::Send);
    // A field that asks for no enter action gets a new line from the key.
    assert_eq!(
        editor_info(TEXT, ACTION_SEND | FLAG_NO_ENTER_ACTION).ime_action,
        ImeAction::None
    );
    assert!(editor_info(TEXT, 0).single_line);
    assert!(!editor_info(TEXT_MULTI_LINE, 0).single_line);
}

#[test]
fn the_field_names_its_capitals() {
    let cases = [
        (TEXT, KeyboardCapitalization::None),
        (TEXT | 0x1000, KeyboardCapitalization::Characters),
        (TEXT | 0x2000, KeyboardCapitalization::Words),
        (TEXT | 0x4000, KeyboardCapitalization::Sentences),
        // The flags mean nothing to a field of digits.
        (NUMBER | 0x1000, KeyboardCapitalization::None),
    ];
    for (input_type, expected) in cases {
        assert_eq!(
            editor_info(input_type, 0).capitalization,
            expected,
            "input type {input_type:#x}"
        );
    }
}
