use super::*;

fn pointer(kind: PointerEventKind, x: f32, y: f32) -> PointerEvent {
    PointerEvent::new(kind, Point { x, y }, Point { x, y })
}

#[test]
fn fling_reports_velocity_on_release() {
    let mut gesture = FlingGesture::new();
    gesture.handle_event(&pointer(PointerEventKind::Down, 0.0, 0.0), 0);
    gesture.handle_event(&pointer(PointerEventKind::Move, 0.0, 40.0), 10);

    match gesture.handle_event(&pointer(PointerEventKind::Up, 0.0, 80.0), 20) {
        FlingGestureEvent::Fling(velocity) => {
            assert_eq!(velocity.x, 0.0);
            assert!(velocity.y > 0.0);
        }
        event => panic!("expected fling event, got {event:?}"),
    }
}

#[test]
fn fling_resets_on_cancel() {
    let mut gesture = FlingGesture::new();
    gesture.handle_event(&pointer(PointerEventKind::Down, 0.0, 0.0), 0);
    assert_eq!(
        gesture.handle_event(&pointer(PointerEventKind::Cancel, 0.0, 0.0), 1),
        FlingGestureEvent::Canceled
    );
    assert_eq!(gesture.velocity(), Point { x: 0.0, y: 0.0 });
}
