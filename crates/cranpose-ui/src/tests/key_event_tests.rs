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

#[test]
fn dom_codes_name_their_physical_keys() {
    assert_eq!(KeyCode::from_dom_code("KeyA"), KeyCode::A);
    assert_eq!(KeyCode::from_dom_code("Digit7"), KeyCode::Digit7);
    assert_eq!(KeyCode::from_dom_code("F12"), KeyCode::F12);
    assert_eq!(KeyCode::from_dom_code("ArrowLeft"), KeyCode::ArrowLeft);
    assert_eq!(KeyCode::from_dom_code("PageDown"), KeyCode::PageDown);
    assert_eq!(KeyCode::from_dom_code("ShiftRight"), KeyCode::ShiftRight);
    assert_eq!(KeyCode::from_dom_code("MetaLeft"), KeyCode::MetaLeft);
    assert_eq!(KeyCode::from_dom_code("Backquote"), KeyCode::Backquote);
}

#[test]
fn numpad_enter_is_enter_and_unmapped_codes_are_unknown() {
    assert_eq!(KeyCode::from_dom_code("NumpadEnter"), KeyCode::Enter);
    assert_eq!(KeyCode::from_dom_code("Numpad5"), KeyCode::Unknown);
    assert_eq!(KeyCode::from_dom_code("keya"), KeyCode::Unknown);
    assert_eq!(KeyCode::from_dom_code(""), KeyCode::Unknown);
}
