use super::*;

#[test]
fn tablet_contact_is_primary_but_barrel_buttons_are_not() {
    assert!(is_primary_tablet_button(TabletToolButton::Contact));
    assert!(!is_primary_tablet_button(TabletToolButton::Barrel));
    assert!(!is_primary_tablet_button(TabletToolButton::Other(7)));
}

#[test]
fn only_the_primary_mouse_button_starts_direct_manipulation() {
    assert!(is_primary_pointer_button(&ButtonSource::Mouse(
        MouseButton::Left
    )));
    assert!(!is_primary_pointer_button(&ButtonSource::Mouse(
        MouseButton::Right
    )));
}
