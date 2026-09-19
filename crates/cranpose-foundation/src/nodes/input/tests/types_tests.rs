use super::*;

fn point(x: f32, y: f32) -> Point {
    Point { x, y }
}

#[test]
fn pointer_event_clones_share_consumed_state() {
    let event = PointerEvent::new(PointerEventKind::Move, point(1.0, 2.0), point(3.0, 4.0));
    let cloned = event.clone();
    assert!(!event.is_consumed());
    assert!(!cloned.is_consumed());

    cloned.consume();

    assert!(event.is_consumed());
    assert!(cloned.is_consumed());
}

#[test]
fn pointer_event_source_defaults_unknown_and_threads_through_copy() {
    let event = PointerEvent::new(PointerEventKind::Down, point(1.0, 1.0), point(1.0, 1.0));
    assert_eq!(event.source, PointerSource::Unknown);
    assert!(!PointerSource::Unknown.is_touch_like());

    let touch = event.with_source(PointerSource::Touch);
    assert_eq!(touch.source, PointerSource::Touch);
    assert!(PointerSource::Touch.is_touch_like());
    assert!(PointerSource::Stylus.is_touch_like());
    assert!(!PointerSource::Mouse.is_touch_like());

    let local = touch.copy_with_local_position(point(5.0, 5.0));
    assert_eq!(local.source, PointerSource::Touch);
}

#[test]
fn modifiers_any_is_true_when_any_field_is_set() {
    assert!(!Modifiers::NONE.any());
    assert!(!Modifiers::default().any());
    assert!(
        Modifiers {
            shift: true,
            ..Modifiers::NONE
        }
        .any()
    );
}

#[test]
fn modifiers_command_or_ctrl_reads_the_platform_appropriate_key() {
    let ctrl_only = Modifiers {
        ctrl: true,
        ..Modifiers::NONE
    };
    let meta_only = Modifiers {
        meta: true,
        ..Modifiers::NONE
    };

    #[cfg(target_os = "macos")]
    {
        assert!(!ctrl_only.command_or_ctrl());
        assert!(meta_only.command_or_ctrl());
    }
    #[cfg(not(target_os = "macos"))]
    {
        assert!(ctrl_only.command_or_ctrl());
        assert!(!meta_only.command_or_ctrl());
    }
    assert!(!Modifiers::NONE.command_or_ctrl());
}

#[test]
fn pointer_event_modifiers_default_to_unreported_and_thread_through_copy() {
    let event = PointerEvent::new(PointerEventKind::Down, point(1.0, 1.0), point(1.0, 1.0));
    assert_eq!(event.modifiers, None);

    let shift = event.with_modifiers(Modifiers {
        shift: true,
        ..Modifiers::NONE
    });
    assert_eq!(
        shift.modifiers,
        Some(Modifiers {
            shift: true,
            ..Modifiers::NONE
        })
    );

    let local = shift.copy_with_local_position(point(5.0, 5.0));
    assert_eq!(local.modifiers, shift.modifiers);
}

#[test]
fn rotary_payload_round_trips_through_pointer_event() {
    let rotary = RotaryScrollEvent::new(-64.0, 12.0, 1_234);
    let event = PointerEvent::rotary(PointerEventKind::RotaryScroll, rotary, point(5.0, 6.0));

    assert_eq!(event.phase, PointerPhase::Move);
    assert_eq!(event.scroll_delta, point(12.0, -64.0));
    assert_eq!(event.time_ms, Some(1_234));
    assert_eq!(event.rotary_scroll_event(), Some(rotary));
}

#[test]
fn rotary_payload_survives_local_position_copies() {
    let rotary = RotaryScrollEvent::new(-8.0, 0.0, 7);
    let event = PointerEvent::rotary(PointerEventKind::RotaryScrollPre, rotary, point(0.0, 0.0));

    let local = event.copy_with_local_position(point(3.0, 4.0));

    assert_eq!(local.rotary_scroll_event(), Some(rotary));
}

#[test]
fn non_rotary_events_have_no_rotary_payload() {
    let scroll = PointerEvent::new(PointerEventKind::Scroll, point(0.0, 0.0), point(0.0, 0.0))
        .with_scroll_delta(point(1.0, 2.0));

    assert_eq!(scroll.rotary_scroll_event(), None);
    assert!(!PointerEventKind::Scroll.is_rotary());
    assert!(PointerEventKind::RotaryScroll.is_rotary());
    assert!(PointerEventKind::RotaryScrollPre.is_rotary());
}

#[test]
fn pointer_event_copy_with_local_position_preserves_consumption_state() {
    let event = PointerEvent::new(PointerEventKind::Down, point(4.0, 5.0), point(4.0, 5.0))
        .with_time_ms(Some(123))
        .with_animation_time_nanos(456_000_000);
    let local = event.copy_with_local_position(point(1.0, 1.0));

    assert_eq!(local.position, point(1.0, 1.0));
    assert_eq!(local.global_position, event.global_position);
    assert_eq!(local.time_ms, Some(123));
    assert_eq!(local.animation_time_nanos, Some(456_000_000));
    assert!(!local.is_consumed());

    event.consume();

    assert!(local.is_consumed());
}

#[test]
fn deferred_post_dispatch_action_consumes_when_it_applies() {
    let event = PointerEvent::new(PointerEventKind::Move, point(0.0, 0.0), point(0.0, 0.0));
    event.defer_post_dispatch_action(|| true);

    event.finish_post_dispatch();

    assert!(event.is_consumed());
}

#[test]
fn deferred_post_dispatch_action_is_replaced_and_discarded_when_consumed() {
    let event = PointerEvent::new(PointerEventKind::Move, point(0.0, 0.0), point(0.0, 0.0));
    let first_called = Rc::new(Cell::new(false));
    let second_called = Rc::new(Cell::new(false));
    let first_called_in_action = first_called.clone();
    let second_called_in_action = second_called.clone();
    event.defer_post_dispatch_action(move || {
        first_called_in_action.set(true);
        true
    });
    event.defer_post_dispatch_action(move || {
        second_called_in_action.set(true);
        true
    });
    event.consume();

    event.finish_post_dispatch();

    assert!(!first_called.get());
    assert!(!second_called.get());
    assert!(event.is_consumed());
}
