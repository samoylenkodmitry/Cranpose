use cranpose_ui::{ImeAction, KeyboardCapitalization, KeyboardType};

use super::{TextTraits, capitalize_next, editor_info};

#[test]
fn a_sentence_starts_a_capital() {
    let sentences = KeyboardCapitalization::Sentences;
    let cases = [
        ("", true),
        ("Hello", false),
        ("Hello ", false),
        ("Hello.", false),
        ("Hello. ", true),
        ("Hello?  ", true),
        ("Wow! ", true),
        ("line\n", true),
        ("e.g", false),
    ];
    for (before, expected) in cases {
        assert_eq!(
            capitalize_next(sentences, before),
            expected,
            "before {before:?}"
        );
    }
}

#[test]
fn a_word_starts_a_capital_only_where_the_field_asks() {
    assert!(capitalize_next(KeyboardCapitalization::Words, ""));
    assert!(capitalize_next(KeyboardCapitalization::Words, "New "));
    assert!(!capitalize_next(KeyboardCapitalization::Words, "New"));
    assert!(capitalize_next(KeyboardCapitalization::Characters, "ABC"));
    assert!(!capitalize_next(KeyboardCapitalization::None, ""));
}

#[test]
fn a_secure_field_is_a_password_with_no_capitals() {
    let traits = TextTraits {
        autocapitalization: 2,
        secure: true,
        ..TextTraits::default()
    };
    let editor = editor_info(traits, "");
    assert_eq!(editor.keyboard_type, KeyboardType::Password);
    assert_eq!(editor.capitalization, KeyboardCapitalization::None);
    assert!(!editor.capitalize_next);
}

#[test]
fn the_traits_name_the_board_and_its_action_key() {
    let traits = TextTraits {
        keyboard_type: 4,
        return_key_type: 6,
        ..TextTraits::default()
    };
    let editor = editor_info(traits, "");
    assert_eq!(editor.keyboard_type, KeyboardType::Number);
    assert_eq!(editor.ime_action, ImeAction::Search);
    let email = editor_info(
        TextTraits {
            keyboard_type: 7,
            return_key_type: 7,
            ..TextTraits::default()
        },
        "",
    );
    assert_eq!(email.keyboard_type, KeyboardType::Email);
    assert_eq!(email.ime_action, ImeAction::Send);
}
