use cranpose_ui::{ImeAction, KeyboardCapitalization, KeyboardType};

use crate::input_method::EditorInfo;

// Values of android.text.InputType and android.view.inputmethod.EditorInfo.
const CLASS_MASK: i32 = 0x0f;
const CLASS_TEXT: i32 = 1;
const CLASS_NUMBER: i32 = 2;
const CLASS_PHONE: i32 = 3;
const VARIATION_MASK: i32 = 0xff0;
const VARIATION_URI: i32 = 0x10;
const VARIATION_EMAIL: i32 = 0x20;
const VARIATION_PASSWORDS: [i32; 3] = [0x80, 0x90, 0xe0];
const NUMBER_PASSWORD: i32 = 0x10;
const NUMBER_DECIMAL: i32 = 0x2000;
const MULTI_LINE: i32 = 0x20000;
const CAP_CHARACTERS: i32 = 0x1000;
const CAP_WORDS: i32 = 0x2000;
const CAP_SENTENCES: i32 = 0x4000;
const ACTION_MASK: i32 = 0xff;
const NO_ENTER_ACTION: i32 = 0x4000_0000;

// Android's `inputType` and `imeOptions` as the board's `EditorInfo`. The
// service reads whether a capital comes next from the text, apart.
pub(crate) fn editor_info(input_type: i32, ime_options: i32) -> EditorInfo {
    let text = input_type & CLASS_MASK == CLASS_TEXT;
    EditorInfo {
        keyboard_type: keyboard_type(input_type),
        ime_action: ime_action(ime_options),
        single_line: !text || input_type & MULTI_LINE == 0,
        capitalization: if text {
            capitalization(input_type)
        } else {
            KeyboardCapitalization::None
        },
        capitalize_next: false,
    }
}

fn keyboard_type(input_type: i32) -> KeyboardType {
    let variation = input_type & VARIATION_MASK;
    match input_type & CLASS_MASK {
        CLASS_NUMBER if variation == NUMBER_PASSWORD => KeyboardType::NumberPassword,
        CLASS_NUMBER if input_type & NUMBER_DECIMAL != 0 => KeyboardType::Decimal,
        CLASS_NUMBER => KeyboardType::Number,
        CLASS_PHONE => KeyboardType::Phone,
        CLASS_TEXT if variation == VARIATION_URI => KeyboardType::Uri,
        CLASS_TEXT if variation == VARIATION_EMAIL => KeyboardType::Email,
        CLASS_TEXT if VARIATION_PASSWORDS.contains(&variation) => KeyboardType::Password,
        _ => KeyboardType::Text,
    }
}

fn ime_action(ime_options: i32) -> ImeAction {
    if ime_options & NO_ENTER_ACTION != 0 {
        return ImeAction::None;
    }
    match ime_options & ACTION_MASK {
        1 => ImeAction::None,
        2 => ImeAction::Go,
        3 => ImeAction::Search,
        4 => ImeAction::Send,
        5 => ImeAction::Next,
        6 => ImeAction::Done,
        7 => ImeAction::Previous,
        _ => ImeAction::Default,
    }
}

fn capitalization(input_type: i32) -> KeyboardCapitalization {
    if input_type & CAP_CHARACTERS != 0 {
        KeyboardCapitalization::Characters
    } else if input_type & CAP_WORDS != 0 {
        KeyboardCapitalization::Words
    } else if input_type & CAP_SENTENCES != 0 {
        KeyboardCapitalization::Sentences
    } else {
        KeyboardCapitalization::None
    }
}

#[cfg(test)]
#[path = "tests/editor_info_wire_tests.rs"]
mod tests;
