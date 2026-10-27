use cranpose_app_shell::Key;
use winit::keyboard::{Key as LogicalKey, KeyCode, NamedKey, PhysicalKey};

use super::app_key;

#[test]
fn keypad_navigation_keys_follow_their_meaning() {
    for (physical, named, expected) in [
        (KeyCode::Numpad4, NamedKey::ArrowLeft, Key::DirectionLeft),
        (KeyCode::Numpad6, NamedKey::ArrowRight, Key::DirectionRight),
        (KeyCode::Numpad8, NamedKey::ArrowUp, Key::DirectionUp),
        (KeyCode::Numpad2, NamedKey::ArrowDown, Key::DirectionDown),
        (KeyCode::Numpad7, NamedKey::Home, Key::MoveHome),
        (KeyCode::Numpad1, NamedKey::End, Key::MoveEnd),
        (KeyCode::NumpadEnter, NamedKey::Enter, Key::NumPadEnter),
        (KeyCode::NumpadDecimal, NamedKey::Delete, Key::Delete),
    ] {
        assert_eq!(
            app_key(PhysicalKey::Code(physical), &LogicalKey::Named(named)),
            expected
        );
    }
}

#[test]
fn letters_keep_their_physical_position() {
    assert_eq!(
        app_key(
            PhysicalKey::Code(KeyCode::KeyC),
            &LogicalKey::Character("с".into())
        ),
        Key::C
    );
    assert_eq!(
        app_key(
            PhysicalKey::Code(KeyCode::ArrowLeft),
            &LogicalKey::Named(NamedKey::ArrowLeft)
        ),
        Key::DirectionLeft
    );
}
