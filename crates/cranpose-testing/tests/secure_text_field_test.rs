//! A secure field hides its text on screen, from copy and cut, and from
//! accessibility, and asks for the platform's password keyboard, through
//! the public shell input path.

use cranpose_ui::{
    BasicSecureTextField, KeyCode, KeyboardType, SecureTextFieldOptions, TextObfuscationMode,
};

use crate::support_text_field::{COMMAND, FocusedField};

const DOT: char = '\u{2022}';

fn secure(text: &'static str, mode: TextObfuscationMode) -> FocusedField {
    FocusedField::composed(text, move |state, modifier| {
        BasicSecureTextField(
            state,
            modifier,
            SecureTextFieldOptions {
                obfuscation_mode: mode,
                ..SecureTextFieldOptions::default()
            },
        );
    })
}

fn dots(count: usize) -> String {
    std::iter::repeat_n(DOT, count).collect()
}

#[test]
fn the_last_typed_character_shows_until_the_next_one() {
    let mut field = secure("pw", TextObfuscationMode::RevealLastTyped);
    assert_eq!(field.shown_text(), dots(2));
    field.type_text("x");
    assert_eq!(field.shown_text(), format!("{}x", dots(2)));
    field.type_text("y");
    assert_eq!(field.shown_text(), format!("{}y", dots(3)));
    field.press(KeyCode::Backspace, cranpose_ui::Modifiers::NONE);
    assert_eq!(field.shown_text(), dots(3), "a deletion reveals nothing");
    assert_eq!(field.text(), "pwx");
}

#[test]
fn hidden_and_visible_fields_show_dots_or_the_text() {
    let mut hidden = secure("pw", TextObfuscationMode::Hidden);
    hidden.type_text("x");
    assert_eq!(hidden.shown_text(), dots(3));

    let mut visible = secure("pw", TextObfuscationMode::Visible);
    visible.type_text("x");
    assert_eq!(visible.shown_text(), "pwx");
}

#[test]
fn copy_and_cut_leave_the_secret_alone() {
    let mut field = secure("secret", TextObfuscationMode::Hidden);
    field.press(KeyCode::A, COMMAND);
    assert_eq!(field.selection(), (0, 6));
    assert_eq!(field.shell.on_copy(), None);
    assert_eq!(field.shell.on_cut(), None);
    assert_eq!(field.text(), "secret");
}

#[test]
fn accessibility_reads_dots_and_the_password_flag() {
    let mut field = secure("secret", TextObfuscationMode::Visible);
    field.shell.update();
    let (text, password) = field.shell.with_layout_tree(|tree| {
        fn find(node: &cranpose_ui::LayoutBox) -> Option<(Option<String>, bool)> {
            node.node_data
                .semantics()
                .filter(|config| config.is_editable_text)
                .map(|config| (config.text.clone(), config.password))
                .or_else(|| node.children.iter().find_map(find))
        }
        tree.and_then(|tree| find(tree.root()))
            .expect("editable field")
    });
    assert_eq!(text, Some(dots(6)), "even a visible secret reads as dots");
    assert!(password);
}

#[test]
fn the_platform_editor_asks_for_a_password_keyboard() {
    let mut field = secure("pw", TextObfuscationMode::RevealLastTyped);
    let editor = field.shell.ime_editor_state().expect("focused field");
    assert_eq!(
        editor.keyboard_options.keyboard_type,
        KeyboardType::Password
    );
    assert!(!editor.keyboard_options.corrects());
    assert!(editor.single_line);
}
