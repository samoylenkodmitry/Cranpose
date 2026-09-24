use super::*;

fn pointer(kind: PointerEventKind, x: f32, y: f32) -> PointerEvent {
    PointerEvent::new(kind, Point { x, y }, Point { x, y })
}

#[test]
fn tap_recognizes_down_up_without_drag() {
    let mut gesture = TapGesture::new();

    assert_eq!(
        gesture.handle_event(&pointer(PointerEventKind::Down, 4.0, 5.0)),
        TapGestureEvent::Pressed(Point { x: 4.0, y: 5.0 })
    );
    assert_eq!(
        gesture.handle_event(&pointer(PointerEventKind::Up, 4.0, 5.0)),
        TapGestureEvent::Tapped(Point { x: 4.0, y: 5.0 })
    );
}

#[test]
fn tap_cancels_after_drag_threshold() {
    let mut gesture = TapGesture::new();

    gesture.handle_event(&pointer(PointerEventKind::Down, 0.0, 0.0));
    assert_eq!(
        gesture.handle_event(&pointer(PointerEventKind::Move, DRAG_THRESHOLD + 1.0, 0.0)),
        TapGestureEvent::Canceled
    );
    assert_eq!(
        gesture.handle_event(&pointer(PointerEventKind::Up, DRAG_THRESHOLD + 1.0, 0.0)),
        TapGestureEvent::Canceled
    );
}
