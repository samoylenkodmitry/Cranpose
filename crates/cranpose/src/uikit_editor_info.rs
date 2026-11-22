// UIKit's text traits of the focused field as the board's `EditorInfo`, and
// whether a capital comes next, which UIKit leaves to the keyboard.

use cranpose_ui::{ImeAction, KeyboardCapitalization, KeyboardType};

use crate::input_method::EditorInfo;

// The raw values of a field's `UITextInputTraits`.
#[derive(Clone, Copy, Debug, Default)]
pub(crate) struct TextTraits {
    pub(crate) keyboard_type: isize,
    pub(crate) return_key_type: isize,
    pub(crate) autocapitalization: isize,
    pub(crate) secure: bool,
}

pub(crate) fn editor_info(traits: TextTraits, before_cursor: &str) -> EditorInfo {
    let capitalization = if traits.secure {
        KeyboardCapitalization::None
    } else {
        capitalization(traits.autocapitalization)
    };
    EditorInfo {
        keyboard_type: if traits.secure {
            KeyboardType::Password
        } else {
            keyboard_type(traits.keyboard_type)
        },
        ime_action: ime_action(traits.return_key_type),
        // UIKit does not tell a field of one line from one of several.
        single_line: false,
        capitalization,
        capitalize_next: capitalize_next(capitalization, before_cursor),
    }
}

// `UIKeyboardType`.
fn keyboard_type(raw: isize) -> KeyboardType {
    match raw {
        1 => KeyboardType::Ascii,
        2 | 4 | 11 => KeyboardType::Number,
        3 => KeyboardType::Uri,
        5 | 6 => KeyboardType::Phone,
        7 => KeyboardType::Email,
        8 => KeyboardType::Decimal,
        _ => KeyboardType::Text,
    }
}

// `UIReturnKeyType`.
fn ime_action(raw: isize) -> ImeAction {
    match raw {
        1 | 3 | 5 | 10 => ImeAction::Go,
        2 | 6 | 8 => ImeAction::Search,
        4 | 11 => ImeAction::Next,
        7 => ImeAction::Send,
        9 => ImeAction::Done,
        _ => ImeAction::Default,
    }
}

// `UITextAutocapitalizationType`.
fn capitalization(raw: isize) -> KeyboardCapitalization {
    match raw {
        1 => KeyboardCapitalization::Words,
        2 => KeyboardCapitalization::Sentences,
        3 => KeyboardCapitalization::Characters,
        _ => KeyboardCapitalization::None,
    }
}

// Whether the next letter typed after `before_cursor` is a capital.
pub(crate) fn capitalize_next(capitalization: KeyboardCapitalization, before_cursor: &str) -> bool {
    let trimmed = before_cursor.trim_end_matches([' ', '\t']);
    match capitalization {
        KeyboardCapitalization::None => false,
        KeyboardCapitalization::Characters => true,
        KeyboardCapitalization::Words => before_cursor
            .chars()
            .next_back()
            .is_none_or(char::is_whitespace),
        KeyboardCapitalization::Sentences => {
            let spaced = trimmed.len() < before_cursor.len();
            match trimmed.chars().next_back() {
                None | Some('\n') => true,
                Some('.' | '!' | '?') => spaced,
                Some(_) => false,
            }
        }
    }
}

#[cfg(test)]
#[path = "tests/uikit_editor_info_tests.rs"]
mod tests;
