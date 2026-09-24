use winit::event::MouseButton;

use super::*;

fn finger(raw: usize) -> FingerId {
    FingerId::from_raw(raw)
}

fn touch_button(raw: usize) -> ButtonSource {
    ButtonSource::Touch {
        finger_id: finger(raw),
        force: None,
    }
}

fn touch_source(raw: usize) -> WinitPointerSource {
    WinitPointerSource::Touch {
        finger_id: finger(raw),
        force: None,
    }
}

#[test]
fn two_touch_downs_with_different_finger_ids_produce_a_primary_and_a_secondary() {
    let mut router = TouchPointerRouter::default();

    assert_eq!(router.press(&touch_button(11)), TouchRoute::Primary);
    assert_eq!(router.press(&touch_button(22)), TouchRoute::Secondary(1));
}

#[test]
fn a_third_finger_gets_its_own_stable_non_zero_id() {
    let mut router = TouchPointerRouter::default();

    router.press(&touch_button(11));
    assert_eq!(router.press(&touch_button(22)), TouchRoute::Secondary(1));
    assert_eq!(router.press(&touch_button(33)), TouchRoute::Secondary(2));
    assert_eq!(
        router.moved(&touch_source(22)),
        TouchRoute::Secondary(1),
        "a finger must keep its id across moves"
    );
    assert_eq!(router.moved(&touch_source(33)), TouchRoute::Secondary(2));
}

#[test]
fn moves_route_each_finger_to_its_own_pointer() {
    let mut router = TouchPointerRouter::default();
    router.press(&touch_button(11));
    router.press(&touch_button(22));

    assert_eq!(router.moved(&touch_source(11)), TouchRoute::Primary);
    assert_eq!(router.moved(&touch_source(22)), TouchRoute::Secondary(1));
}

#[test]
fn moves_from_an_untracked_finger_do_not_steal_the_primary_pointer() {
    let mut router = TouchPointerRouter::default();
    router.press(&touch_button(11));

    assert_eq!(router.moved(&touch_source(99)), TouchRoute::Untracked);
}

#[test]
fn lifting_a_secondary_finger_leaves_the_primary_gesture_running() {
    let mut router = TouchPointerRouter::default();
    router.press(&touch_button(11));
    router.press(&touch_button(22));

    assert_eq!(
        router.release(&touch_button(22)),
        TouchRelease {
            secondaries: vec![1],
            releases_primary: false,
        }
    );
    assert_eq!(router.moved(&touch_source(11)), TouchRoute::Primary);
}

#[test]
fn lifting_the_primary_finger_closes_the_secondaries_first() {
    let mut router = TouchPointerRouter::default();
    router.press(&touch_button(11));
    router.press(&touch_button(22));
    router.press(&touch_button(33));

    assert_eq!(
        router.release(&touch_button(11)),
        TouchRelease {
            secondaries: vec![1, 2],
            releases_primary: true,
        }
    );
    assert_eq!(router.moved(&touch_source(22)), TouchRoute::Untracked);
}

#[test]
fn a_new_finger_after_the_gesture_ends_becomes_the_next_primary() {
    let mut router = TouchPointerRouter::default();
    router.press(&touch_button(11));
    router.release(&touch_button(11));

    assert_eq!(router.press(&touch_button(44)), TouchRoute::Primary);
    assert_eq!(
        router.press(&touch_button(55)),
        TouchRoute::Secondary(1),
        "secondary ids restart with each gesture"
    );
}

#[test]
fn pointer_left_for_a_secondary_finger_does_not_cancel_the_gesture() {
    let mut router = TouchPointerRouter::default();
    router.press(&touch_button(11));
    router.press(&touch_button(22));

    assert_eq!(
        router.left(&PointerKind::Touch(finger(22))),
        TouchLeave::ReleaseSecondary(1)
    );
    assert_eq!(router.moved(&touch_source(11)), TouchRoute::Primary);
}

#[test]
fn pointer_left_trailing_a_release_still_cancels_once_the_gesture_is_over() {
    let mut router = TouchPointerRouter::default();
    router.press(&touch_button(11));
    router.release(&touch_button(11));

    assert_eq!(
        router.left(&PointerKind::Touch(finger(11))),
        TouchLeave::CancelGesture,
        "the single-finger sequence must behave as it did before"
    );
}

#[test]
fn pointer_left_trailing_a_secondary_release_is_ignored_mid_gesture() {
    let mut router = TouchPointerRouter::default();
    router.press(&touch_button(11));
    router.press(&touch_button(22));
    router.release(&touch_button(22));

    assert_eq!(
        router.left(&PointerKind::Touch(finger(22))),
        TouchLeave::Ignore
    );
}

#[test]
fn pointer_left_for_the_primary_finger_cancels_the_gesture() {
    let mut router = TouchPointerRouter::default();
    router.press(&touch_button(11));
    router.press(&touch_button(22));

    assert_eq!(
        router.left(&PointerKind::Touch(finger(11))),
        TouchLeave::CancelGesture
    );
    assert!(router.is_idle());
}

#[test]
fn mouse_input_always_drives_the_primary_pointer() {
    let mut router = TouchPointerRouter::default();

    let button = ButtonSource::Mouse(MouseButton::Left);
    assert_eq!(router.press(&button), TouchRoute::Primary);
    assert_eq!(
        router.moved(&WinitPointerSource::Mouse),
        TouchRoute::Primary
    );
    assert_eq!(
        router.release(&button),
        TouchRelease {
            secondaries: Vec::new(),
            releases_primary: true,
        }
    );
    assert_eq!(router.left(&PointerKind::Mouse), TouchLeave::CancelGesture);
}
