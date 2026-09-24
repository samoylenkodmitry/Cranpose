use super::*;

#[test]
fn key_event_creation() {
    let event = KeyEvent::key_down(KeyCode::A, "a");
    assert_eq!(event.key_code, KeyCode::A);
    assert_eq!(event.text, "a");
    assert!(event.is_key_down());
    assert!(event.has_text());
}

#[test]
fn key_event_with_modifiers() {
    let modifiers = Modifiers {
        shift: true,
        ctrl: false,
        alt: false,
        meta: false,
    };
    let event = KeyEvent::key_down_with_modifiers(KeyCode::A, "A", modifiers);
    assert_eq!(event.text, "A");
    assert!(event.modifiers.shift);
}

#[test]
fn backspace_has_no_text() {
    let event = KeyEvent::key_down(KeyCode::Backspace, "");
    assert!(!event.has_text());
}
