// A mouse's right, middle and side buttons reach the nodes under the
// pointer as their own presses. A click, a drag or a scroll starts only from
// the primary button, so a right press never clicks a button or scrolls a
// list, and a handler that asks for another button gets its press, the
// moves of its drag and its release.

use cranpose_ui::PointerButton;

use super::*;

type Seen = Rc<RefCell<Vec<(PointerEventKind, bool, Point)>>>;

/// A 320 by 240 shell that hit tests `content`.
fn hit_shell(content: impl FnMut() + 'static) -> AppShell<HitGraphRenderer> {
    let mut shell = AppShell::new(
        HitGraphRenderer::default(),
        location_key(file!(), line!(), column!()),
        content,
    );
    shell.set_buffer_size(320, 240);
    shell.set_viewport(320.0, 240.0);
    shell
}

/// A modifier that writes down every pointer event the node receives: its
/// kind, whether the secondary button is held, and where it happened.
fn recording(seen: &Seen) -> Modifier {
    let seen = Rc::clone(seen);
    Modifier::empty().pointer_input((), move |scope: PointerInputScope| {
        let seen = Rc::clone(&seen);
        async move {
            scope
                .await_pointer_event_scope(|events| async move {
                    loop {
                        let event = events.await_pointer_event().await;
                        seen.borrow_mut().push((
                            event.kind,
                            event.buttons.contains(PointerButton::Secondary),
                            event.global_position,
                        ));
                    }
                })
                .await;
        }
    })
}

/// A shell showing a 100 by 100 box at the origin that records its pointer
/// events into `seen`, with `more` added to its modifier.
fn recording_box(
    seen: &Seen,
    more: impl Fn(Modifier) -> Modifier + 'static,
) -> AppShell<HitGraphRenderer> {
    let seen = Rc::clone(seen);
    let mut shell = hit_shell(move || {
        Box(
            more(recording(&seen)).size(Size {
                width: 100.0,
                height: 100.0,
            }),
            BoxSpec::default(),
            || {},
        );
    });
    shell.update();
    shell
}

/// Presses the right button in the box, runs `during`, and releases it
/// outside the box: the drag keeps the box until the release.
fn right_drag_out(
    shell: &mut AppShell<HitGraphRenderer>,
    during: impl FnOnce(&mut AppShell<HitGraphRenderer>),
) {
    shell.set_cursor(50.0, 50.0);
    shell
        .primary()
        .mouse_button_pressed(PointerButton::Secondary);
    during(shell);
    shell.set_cursor(200.0, 150.0);
    shell
        .primary()
        .mouse_button_released(PointerButton::Secondary);
    shell.update();
}

#[test]
fn a_right_press_reaches_a_handler_and_never_clicks() {
    let _guard = test_guard();
    let seen: Seen = Rc::default();
    let clicks = Rc::new(RefCell::new(0));
    let mut shell = recording_box(&seen, {
        let clicks = Rc::clone(&clicks);
        move |modifier| {
            let clicks = Rc::clone(&clicks);
            modifier.clickable(move |_| *clicks.borrow_mut() += 1)
        }
    });

    right_drag_out(&mut shell, |_| {});

    assert_eq!(*clicks.borrow(), 0, "a right press and release clicked");
    let kinds: Vec<_> = seen.borrow().iter().map(|(kind, ..)| *kind).collect();
    assert!(
        !kinds.contains(&PointerEventKind::Down) && !kinds.contains(&PointerEventKind::Up),
        "a right press came as the primary pointer's: {kinds:?}"
    );
    assert!(
        seen.borrow().contains(&(
            PointerEventKind::ButtonDown(PointerButton::Secondary),
            true,
            Point::new(50.0, 50.0)
        )),
        "the node missed the right press: {:?}",
        seen.borrow()
    );
    assert!(
        seen.borrow()
            .contains(&(PointerEventKind::Move, true, Point::new(200.0, 150.0))),
        "the node missed the drag's move beyond its bounds: {:?}",
        seen.borrow()
    );
    assert_eq!(
        kinds.last(),
        Some(&PointerEventKind::ButtonUp(PointerButton::Secondary)),
        "the node missed the right release"
    );

    shell.set_cursor(50.0, 50.0);
    shell.pointer_pressed();
    shell.pointer_released();
    shell.update();
    assert_eq!(*clicks.borrow(), 1, "a left click did not click");
}

#[test]
fn a_right_drag_does_not_scroll_and_a_finger_does() {
    let _guard = test_guard();
    let offset = Rc::new(RefCell::new(None::<ScrollState>));
    let mut shell = hit_shell({
        let offset = Rc::clone(&offset);
        move || {
            let state = cranpose_core::remember(|| ScrollState::new(0.0)).with(|state| *state);
            *offset.borrow_mut() = Some(state);
            Column(
                Modifier::empty()
                    .size(Size {
                        width: 200.0,
                        height: 200.0,
                    })
                    .vertical_scroll(state, false),
                ColumnSpec::default(),
                || {
                    for row in 0..20 {
                        Text(
                            format!("Row {row}"),
                            Modifier::empty().size(Size {
                                width: 200.0,
                                height: 40.0,
                            }),
                            TextStyle::default(),
                        );
                    }
                },
            );
        }
    });
    shell.update();
    let state = offset.borrow().expect("the scroll composed");

    shell.set_pointer_source(PointerSource::Mouse);
    shell.set_cursor(100.0, 180.0);
    shell
        .primary()
        .mouse_button_pressed(PointerButton::Secondary);
    for step in 1..=10 {
        shell.set_cursor(100.0, 180.0 - 12.0 * step as f32);
    }
    shell
        .primary()
        .mouse_button_released(PointerButton::Secondary);
    shell.update();
    assert_eq!(state.value(), 0.0, "a right drag scrolled the list");

    shell.set_pointer_source(PointerSource::Touch);
    shell.set_cursor(100.0, 180.0);
    shell.pointer_pressed();
    for step in 1..=10 {
        shell.set_cursor(100.0, 180.0 - 12.0 * step as f32);
    }
    shell.pointer_released();
    shell.update();
    assert!(
        state.value() > 0.0,
        "a finger's drag did not scroll the list"
    );
}

#[test]
fn a_left_release_during_a_right_drag_leaves_the_drag_its_node() {
    let _guard = test_guard();
    let seen: Seen = Rc::default();
    let mut shell = recording_box(&seen, |modifier| modifier);

    right_drag_out(&mut shell, |shell| {
        shell.pointer_pressed();
        shell.pointer_released();
    });

    let seen = seen.borrow();
    assert!(
        seen.contains(&(PointerEventKind::Move, true, Point::new(200.0, 150.0))),
        "the right drag lost its node when the left button rose: {seen:?}"
    );
    assert_eq!(
        seen.last().map(|(kind, ..)| *kind),
        Some(PointerEventKind::ButtonUp(PointerButton::Secondary)),
        "the right release missed the node: {seen:?}"
    );
}

#[test]
fn a_held_right_button_is_a_gesture_but_not_a_primary_one() {
    let _guard = test_guard();
    let seen: Seen = Rc::default();
    let mut shell = recording_box(&seen, |modifier| modifier);

    shell.set_cursor(50.0, 50.0);
    shell
        .primary()
        .mouse_button_pressed(PointerButton::Secondary);
    assert!(
        shell.has_active_pointer_gesture(),
        "a held right button kept no gesture"
    );
    assert!(
        !shell.has_active_primary_gesture(),
        "a held right button counted as a held left one, which a platform that polls the left button then releases"
    );
    shell
        .primary()
        .mouse_button_released(PointerButton::Secondary);
    assert!(!shell.has_active_pointer_gesture());

    shell.pointer_pressed();
    assert!(shell.has_active_pointer_gesture());
    assert!(
        shell.has_active_primary_gesture(),
        "a held left button is not a primary gesture"
    );
    shell.pointer_released();
    assert!(!shell.has_active_primary_gesture());
}
