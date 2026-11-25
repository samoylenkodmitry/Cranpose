use cranpose_foundation::PointerEventKind::{Cancel, Down, Move, Up};

use super::*;

type Log = Rc<RefCell<Vec<(PointerId, PointerEventKind, Option<i64>)>>>;

/// A shell whose whole surface records the id, kind and sample time of each
/// press, release, cancel and pressed move it gets (no hover), and the router
/// that feeds it fingers numbered by `u32`.
fn recording_shell() -> (AppShell<HitGraphRenderer>, TouchRouter<u32>, Log) {
    let log = Log::default();
    let sink = Rc::clone(&log);
    let mut shell = AppShell::new(
        HitGraphRenderer::default(),
        location_key(file!(), line!(), column!()),
        move || {
            let sink = Rc::clone(&sink);
            Box(
                Modifier::empty().fill_max_size().pointer_input(
                    (),
                    move |scope: PointerInputScope| {
                        let sink = Rc::clone(&sink);
                        async move {
                            scope
                                .await_pointer_event_scope(|events| async move {
                                    loop {
                                        let event = events.await_pointer_event().await;
                                        let pressed =
                                            event.buttons.contains(PointerButton::Primary);
                                        let kept = match event.kind {
                                            Down | Up | Cancel => true,
                                            Move => pressed,
                                            _ => false,
                                        };
                                        if kept {
                                            sink.borrow_mut().push((
                                                event.id,
                                                event.kind,
                                                event.time_ms,
                                            ));
                                        }
                                    }
                                })
                                .await;
                        }
                    },
                ),
                BoxSpec::default(),
                || {},
            );
        },
    );
    shell.set_viewport(400.0, 400.0);
    shell.update();
    (shell, TouchRouter::default(), log)
}

// The pointers' raw ids and kinds the log took since the last call.
fn taken(log: &Log) -> Vec<(u64, PointerEventKind)> {
    std::mem::take(&mut *log.borrow_mut())
        .into_iter()
        .map(|(id, kind, _)| (id.0, kind))
        .collect()
}

#[test]
fn each_finger_carries_its_platform_sample_time_to_the_gesture() {
    let _guard = test_guard();
    let (mut shell, mut router, log) = recording_shell();
    router.press(
        &mut shell,
        Some(11),
        TouchSample::new(100.0, 100.0).with_time_ms(Some(1_000)),
    );
    router.press(
        &mut shell,
        Some(22),
        TouchSample::new(200.0, 100.0).with_time_ms(Some(1_004)),
    );
    router.moved(
        &mut shell,
        Some(11),
        TouchSample::new(110.0, 100.0).with_time_ms(Some(1_008)),
    );
    router.moved(
        &mut shell,
        Some(22),
        TouchSample::new(210.0, 100.0).with_time_ms(Some(1_012)),
    );
    router.release(
        &mut shell,
        Some(22),
        TouchSample::new(210.0, 100.0).with_time_ms(Some(1_016)),
    );
    router.release(
        &mut shell,
        Some(11),
        TouchSample::new(110.0, 100.0).with_time_ms(Some(1_020)),
    );
    let times = std::mem::take(&mut *log.borrow_mut())
        .into_iter()
        .map(|(id, kind, time)| (id.0, kind, time))
        .collect::<Vec<_>>();
    assert_eq!(
        times,
        [
            (0, Down, Some(1_000)),
            (1, Down, Some(1_004)),
            (0, Move, Some(1_008)),
            (1, Move, Some(1_012)),
            (1, Up, Some(1_016)),
            (0, Up, Some(1_020)),
        ]
    );
}

#[test]
fn each_finger_drives_its_own_pointer_with_a_stable_id() {
    let _guard = test_guard();
    let (mut shell, mut router, log) = recording_shell();
    router.press(&mut shell, Some(11), TouchSample::new(100.0, 100.0));
    router.press(&mut shell, Some(22), TouchSample::new(200.0, 100.0));
    router.press(&mut shell, Some(33), TouchSample::new(300.0, 100.0));
    router.moved(&mut shell, Some(22), TouchSample::new(210.0, 100.0));
    router.moved(&mut shell, Some(33), TouchSample::new(310.0, 100.0));
    router.moved(&mut shell, Some(11), TouchSample::new(110.0, 100.0));
    assert_eq!(
        taken(&log),
        [
            (0, Down),
            (1, Down),
            (2, Down),
            (1, Move),
            (2, Move),
            (0, Move)
        ]
    );
}

#[test]
fn a_finger_that_is_not_down_moves_nothing() {
    let _guard = test_guard();
    let (mut shell, mut router, log) = recording_shell();
    router.press(&mut shell, Some(11), TouchSample::new(100.0, 100.0));
    assert!(!router.moved(&mut shell, Some(99), TouchSample::new(150.0, 150.0)));
    assert_eq!(taken(&log), [(0, Down)]);
}

#[test]
fn lifting_a_secondary_finger_keeps_the_gesture() {
    let _guard = test_guard();
    let (mut shell, mut router, log) = recording_shell();
    router.press(&mut shell, Some(11), TouchSample::new(100.0, 100.0));
    router.press(&mut shell, Some(22), TouchSample::new(200.0, 100.0));
    router.release(&mut shell, Some(22), TouchSample::new(200.0, 100.0));
    router.moved(&mut shell, Some(11), TouchSample::new(120.0, 100.0));
    assert_eq!(taken(&log), [(0, Down), (1, Down), (1, Up), (0, Move)]);
}

#[test]
fn lifting_the_primary_finger_releases_the_others_first_and_ends_the_gesture() {
    let _guard = test_guard();
    let (mut shell, mut router, log) = recording_shell();
    router.press(&mut shell, Some(11), TouchSample::new(100.0, 100.0));
    router.press(&mut shell, Some(22), TouchSample::new(200.0, 100.0));
    router.press(&mut shell, Some(33), TouchSample::new(300.0, 100.0));
    router.release(&mut shell, Some(11), TouchSample::new(100.0, 100.0));
    assert!(!router.moved(&mut shell, Some(22), TouchSample::new(220.0, 100.0)));
    // The next finger down drives the primary pointer, and secondary ids
    // start over.
    router.press(&mut shell, Some(44), TouchSample::new(100.0, 200.0));
    router.press(&mut shell, Some(55), TouchSample::new(200.0, 200.0));
    assert_eq!(
        taken(&log),
        [
            (0, Down),
            (1, Down),
            (2, Down),
            (1, Up),
            (2, Up),
            (0, Up),
            (0, Down),
            (1, Down)
        ]
    );
}

#[test]
fn a_secondary_finger_that_leaves_is_released_without_ending_the_gesture() {
    let _guard = test_guard();
    let (mut shell, mut router, log) = recording_shell();
    router.press(&mut shell, Some(11), TouchSample::new(100.0, 100.0));
    router.press(&mut shell, Some(22), TouchSample::new(200.0, 100.0));
    router.cancel(&mut shell, Some(22), Some((200.0, 100.0)));
    router.moved(&mut shell, Some(11), TouchSample::new(120.0, 100.0));
    assert_eq!(taken(&log), [(0, Down), (1, Down), (1, Up), (0, Move)]);
}

#[test]
fn the_primary_finger_leaving_cancels_the_gesture() {
    let _guard = test_guard();
    let (mut shell, mut router, log) = recording_shell();
    router.press(&mut shell, Some(11), TouchSample::new(100.0, 100.0));
    router.press(&mut shell, Some(22), TouchSample::new(200.0, 100.0));
    assert!(router.cancel(&mut shell, Some(11), None));
    assert!(!router.moved(&mut shell, Some(22), TouchSample::new(220.0, 100.0)));
    assert_eq!(taken(&log), [(0, Down), (1, Down), (0, Cancel)]);
}

#[test]
fn a_leave_after_the_last_lift_still_cancels_and_one_mid_gesture_is_ignored() {
    let _guard = test_guard();
    let (mut shell, mut router, _log) = recording_shell();
    router.press(&mut shell, Some(11), TouchSample::new(100.0, 100.0));
    router.press(&mut shell, Some(22), TouchSample::new(200.0, 100.0));
    router.release(&mut shell, Some(22), TouchSample::new(200.0, 100.0));
    assert!(
        !router.cancel(&mut shell, Some(22), None),
        "a lifted secondary finger's leave changes nothing mid-gesture"
    );
    router.release(&mut shell, Some(11), TouchSample::new(100.0, 100.0));
    assert!(
        router.cancel(&mut shell, Some(11), None),
        "with no gesture running, a leave cancels as a single finger's does"
    );
}

#[test]
fn the_mouse_always_drives_the_primary_pointer() {
    let _guard = test_guard();
    let (mut shell, mut router, log) = recording_shell();
    router.press(&mut shell, None, TouchSample::new(100.0, 100.0));
    router.moved(&mut shell, None, TouchSample::new(120.0, 100.0));
    router.release(&mut shell, None, TouchSample::new(120.0, 100.0));
    assert_eq!(taken(&log), [(0, Down), (0, Move), (0, Up)]);
    assert!(router.cancel(&mut shell, None, None));
}

#[test]
fn press_at_position_clicks_without_synthesizing_hover_enter() {
    let _guard = test_guard();
    let events = Rc::new(RefCell::new(Vec::new()));
    let clicks = Rc::new(Cell::new(0u32));
    let events_sink = Rc::clone(&events);
    let clicks_sink = Rc::clone(&clicks);
    let mut shell = AppShell::new(
        HitGraphRenderer::default(),
        location_key(file!(), line!(), column!()),
        move || {
            let events_sink = Rc::clone(&events_sink);
            let clicks_sink = Rc::clone(&clicks_sink);
            // Offset the clickable away from `(0, 0)` so a press must move the
            // cursor to `(x, y)` before hit-testing.
            Box(
                Modifier::empty().fill_max_size().padding(50.0),
                BoxSpec::default(),
                move || {
                    let events_sink = Rc::clone(&events_sink);
                    let clicks_sink = Rc::clone(&clicks_sink);
                    Box(
                        Modifier::empty()
                            .size_points(100.0, 100.0)
                            .clickable(move |_| clicks_sink.set(clicks_sink.get() + 1))
                            .pointer_input((), move |scope: PointerInputScope| {
                                let events_sink = Rc::clone(&events_sink);
                                async move {
                                    scope
                                        .await_pointer_event_scope(|pointer_events| async move {
                                            loop {
                                                let event =
                                                    pointer_events.await_pointer_event().await;
                                                events_sink.borrow_mut().push(event.kind);
                                            }
                                        })
                                        .await;
                                }
                            }),
                        BoxSpec::default(),
                        || {},
                    );
                },
            );
        },
    );
    shell.set_viewport(200.0, 200.0);
    shell.update();

    assert!(!shell.pointer_pressed_at_position(10.0, 10.0));
    shell.pointer_released_at_position(10.0, 10.0);

    for source in [PointerSource::Unknown, PointerSource::Stylus] {
        shell.set_pointer_source(source);
        assert!(shell.pointer_pressed_at_position(100.0, 100.0));
        shell.pointer_released_at_position(100.0, 100.0);
    }

    let mut router = TouchRouter::<u32>::default();
    shell.set_pointer_source(PointerSource::Unknown);
    assert!(router.press(&mut shell, Some(7), TouchSample::new(100.0, 100.0)));
    assert!(router.release(&mut shell, Some(7), TouchSample::new(100.0, 100.0)));

    assert_eq!(clicks.get(), 3);
    assert!(
        !events.borrow().contains(&PointerEventKind::Enter),
        "direct presses without hover must not synthesize Enter: {:?}",
        events.borrow()
    );
}
