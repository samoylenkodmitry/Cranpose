use cranpose_ui::{CursorIcon, PointerIcon};

use super::{a_cursor_that_is_not, offering_the_same_icon_needs_a_nudge};

#[test]
fn re_offering_the_icon_a_window_already_has_needs_a_nudge() {
    let icon = PointerIcon::System(CursorIcon::Grab);

    assert!(
        offering_the_same_icon_needs_a_nudge(Some(&icon), &icon),
        "a refresh offers the icon the window already holds, and the \
         backend drops that call along with the invalidation the platform \
         needs, so the cursor would never be re-drawn"
    );
}

#[test]
fn a_genuine_change_goes_straight_through() {
    let held = PointerIcon::System(CursorIcon::Grab);
    let wanted = PointerIcon::System(CursorIcon::Text);

    assert!(!offering_the_same_icon_needs_a_nudge(Some(&held), &wanted));
    assert!(!offering_the_same_icon_needs_a_nudge(None, &wanted));
}

#[test]
fn the_nudge_is_never_the_icon_it_is_making_room_for() {
    for icon in [
        PointerIcon::DEFAULT,
        PointerIcon::System(CursorIcon::Grab),
        PointerIcon::System(CursorIcon::Text),
    ] {
        assert_ne!(
            PointerIcon::System(a_cursor_that_is_not(&icon)),
            icon,
            "a nudge equal to the target would be dropped exactly like the \
             call it is making room for: {icon:?}"
        );
    }
}
