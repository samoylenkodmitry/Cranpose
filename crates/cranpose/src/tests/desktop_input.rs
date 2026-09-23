use cranpose_app_shell::KeyCode;
use winit::keyboard::{Key, KeyCode as WinitKeyCode, NamedKey, PhysicalKey};

use super::app_key_code;

#[test]
fn keypad_navigation_keys_follow_their_meaning() {
    for (physical, named, expected) in [
        (
            WinitKeyCode::Numpad4,
            NamedKey::ArrowLeft,
            KeyCode::ArrowLeft,
        ),
        (
            WinitKeyCode::Numpad6,
            NamedKey::ArrowRight,
            KeyCode::ArrowRight,
        ),
        (WinitKeyCode::Numpad8, NamedKey::ArrowUp, KeyCode::ArrowUp),
        (
            WinitKeyCode::Numpad2,
            NamedKey::ArrowDown,
            KeyCode::ArrowDown,
        ),
        (WinitKeyCode::Numpad7, NamedKey::Home, KeyCode::Home),
        (WinitKeyCode::Numpad1, NamedKey::End, KeyCode::End),
        (WinitKeyCode::NumpadEnter, NamedKey::Enter, KeyCode::Enter),
        (
            WinitKeyCode::NumpadDecimal,
            NamedKey::Delete,
            KeyCode::Delete,
        ),
    ] {
        assert_eq!(
            app_key_code(PhysicalKey::Code(physical), &Key::Named(named)),
            expected
        );
    }
}

#[test]
fn letters_keep_their_physical_position() {
    assert_eq!(
        app_key_code(
            PhysicalKey::Code(WinitKeyCode::KeyC),
            &Key::Character("с".into())
        ),
        KeyCode::C
    );
    assert_eq!(
        app_key_code(
            PhysicalKey::Code(WinitKeyCode::ArrowLeft),
            &Key::Named(NamedKey::ArrowLeft)
        ),
        KeyCode::ArrowLeft
    );
}
