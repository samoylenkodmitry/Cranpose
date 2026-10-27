use super::*;

#[test]
fn key_event_creation() {
    let event = KeyEvent::key_down(Key::A, "a");
    assert_eq!(event.key(), Key::A);
    assert_eq!(event.text(), "a");
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
    let event = KeyEvent::key_down_with_modifiers(Key::A, "A", modifiers);
    assert_eq!(event.text(), "A");
    assert!(event.modifiers().shift);
}

#[test]
fn backspace_has_no_text() {
    let event = KeyEvent::key_down(Key::Backspace, "");
    assert!(!event.has_text());
}

#[test]
fn dom_codes_name_their_physical_keys() {
    assert_eq!(Key::from_dom_code("KeyA"), Key::A);
    assert_eq!(Key::from_dom_code("Digit7"), Key::Seven);
    assert_eq!(Key::from_dom_code("F12"), Key::F12);
    assert_eq!(Key::from_dom_code("ArrowLeft"), Key::DirectionLeft);
    assert_eq!(Key::from_dom_code("PageDown"), Key::PageDown);
    assert_eq!(Key::from_dom_code("ShiftRight"), Key::ShiftRight);
    assert_eq!(Key::from_dom_code("MetaLeft"), Key::MetaLeft);
    assert_eq!(Key::from_dom_code("Backquote"), Key::Grave);
}

#[test]
fn media_codes_name_remote_keys() {
    assert_eq!(Key::from_dom_code("MediaPlayPause"), Key::MediaPlayPause);
    assert_eq!(Key::from_dom_code("MediaTrackNext"), Key::MediaNext);
    assert_eq!(Key::from_dom_code("ContextMenu"), Key::Menu);
}

#[test]
fn numpad_enter_is_its_own_key_and_unmapped_codes_are_unknown() {
    assert_eq!(Key::from_dom_code("NumpadEnter"), Key::NumPadEnter);
    assert_eq!(Key::from_dom_code("Numpad5"), Key::Unknown);
    assert_eq!(Key::from_dom_code("keya"), Key::Unknown);
    assert_eq!(Key::from_dom_code(""), Key::Unknown);
}
