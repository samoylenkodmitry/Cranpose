use super::*;

fn event(kind: PointerEventKind, id: u64, x: f32, y: f32) -> PointerEvent {
    let mut event = PointerEvent::new(kind, Point { x, y }, Point { x, y });
    event.id = id;
    event
}

#[test]
fn pinch_out_reports_zoom_and_focal_centroid() {
    let mut gesture = TransformGesture::new();
    gesture.handle_event(&event(PointerEventKind::Down, 0, 100.0, 100.0));
    gesture.handle_event(&event(PointerEventKind::Down, 1, 200.0, 100.0));

    let step = gesture.handle_event(&event(PointerEventKind::Move, 1, 300.0, 100.0));
    match step {
        TransformGestureEvent::Transform {
            pan,
            zoom,
            centroid,
            pointer_count,
        } => {
            assert!((zoom - 2.0).abs() < 1e-5, "spread doubled, got zoom={zoom}");
            assert!((pan.x - 50.0).abs() < 1e-5 && pan.y.abs() < 1e-5, "{pan:?}");
            assert_eq!(centroid, Point { x: 150.0, y: 100.0 });
            assert_eq!(pointer_count, 2);
        }
        other => panic!("expected Transform, got {other:?}"),
    }
}

#[test]
fn pinch_in_reports_zoom_below_one() {
    let mut gesture = TransformGesture::new();
    gesture.handle_event(&event(PointerEventKind::Down, 0, 0.0, 0.0));
    gesture.handle_event(&event(PointerEventKind::Down, 1, 0.0, 200.0));

    let step = gesture.handle_event(&event(PointerEventKind::Move, 1, 0.0, 100.0));
    match step {
        TransformGestureEvent::Transform { zoom, .. } => {
            assert!((zoom - 0.5).abs() < 1e-5, "spread halved, got zoom={zoom}");
        }
        other => panic!("expected Transform, got {other:?}"),
    }
}

#[test]
fn two_finger_pan_steps_compose_to_pure_pan() {
    let mut gesture = TransformGesture::new();
    gesture.handle_event(&event(PointerEventKind::Down, 0, 100.0, 100.0));
    gesture.handle_event(&event(PointerEventKind::Down, 1, 200.0, 100.0));

    let mut total_pan = Point { x: 0.0, y: 0.0 };
    let mut total_zoom = 1.0;
    for step in [
        gesture.handle_event(&event(PointerEventKind::Move, 0, 110.0, 100.0)),
        gesture.handle_event(&event(PointerEventKind::Move, 1, 210.0, 100.0)),
    ] {
        if let TransformGestureEvent::Transform { pan, zoom, .. } = step {
            total_pan.x += pan.x;
            total_pan.y += pan.y;
            total_zoom *= zoom;
        }
    }

    assert!(
        (total_pan.x - 10.0).abs() < 1e-4 && total_pan.y.abs() < 1e-4,
        "steps must compose to the +10 centroid pan, got {total_pan:?}"
    );
    assert!(
        (total_zoom - 1.0).abs() < 1e-4,
        "pure pan must compose to zoom 1.0, got {total_zoom}"
    );
}

#[test]
fn single_finger_move_is_pan_only() {
    let mut gesture = TransformGesture::new();
    gesture.handle_event(&event(PointerEventKind::Down, 0, 50.0, 50.0));

    let step = gesture.handle_event(&event(PointerEventKind::Move, 0, 62.0, 45.0));
    assert_eq!(
        step,
        TransformGestureEvent::Transform {
            pan: Point { x: 12.0, y: -5.0 },
            zoom: 1.0,
            centroid: Point { x: 50.0, y: 50.0 },
            pointer_count: 1,
        }
    );
}

#[test]
fn untracked_pointer_moves_are_ignored() {
    let mut gesture = TransformGesture::new();
    gesture.handle_event(&event(PointerEventKind::Down, 0, 50.0, 50.0));

    let step = gesture.handle_event(&event(PointerEventKind::Move, 7, 500.0, 500.0));
    assert_eq!(step, TransformGestureEvent::None);
}

#[test]
fn gesture_ends_when_last_pointer_lifts() {
    let mut gesture = TransformGesture::new();
    gesture.handle_event(&event(PointerEventKind::Down, 0, 0.0, 0.0));
    gesture.handle_event(&event(PointerEventKind::Down, 1, 100.0, 0.0));

    assert_eq!(
        gesture.handle_event(&event(PointerEventKind::Up, 1, 100.0, 0.0)),
        TransformGestureEvent::None
    );
    assert_eq!(gesture.pointer_count(), 1);
    assert_eq!(
        gesture.handle_event(&event(PointerEventKind::Up, 0, 0.0, 0.0)),
        TransformGestureEvent::Ended
    );
    assert_eq!(gesture.pointer_count(), 0);
}

#[test]
fn lifting_one_finger_does_not_jump_the_pan() {
    let mut gesture = TransformGesture::new();
    gesture.handle_event(&event(PointerEventKind::Down, 0, 0.0, 0.0));
    gesture.handle_event(&event(PointerEventKind::Down, 1, 100.0, 0.0));
    gesture.handle_event(&event(PointerEventKind::Up, 1, 100.0, 0.0));

    let step = gesture.handle_event(&event(PointerEventKind::Move, 0, 5.0, 0.0));
    match step {
        TransformGestureEvent::Transform { pan, zoom, .. } => {
            assert!((pan.x - 5.0).abs() < 1e-5 && pan.y.abs() < 1e-5, "{pan:?}");
            assert_eq!(zoom, 1.0);
        }
        other => panic!("expected Transform, got {other:?}"),
    }
}
