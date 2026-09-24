use super::*;

#[test]
fn scroll_consumes_non_zero_scroll_delta() {
    let mut gesture = ScrollGesture::new();
    let event = PointerEvent::new(
        PointerEventKind::Scroll,
        Point { x: 0.0, y: 0.0 },
        Point { x: 0.0, y: 0.0 },
    )
    .with_scroll_delta(Point { x: 0.0, y: 12.0 });

    assert_eq!(
        gesture.handle_event(&event),
        ScrollGestureEvent::Scrolled(Point { x: 0.0, y: 12.0 })
    );
    assert!(event.is_consumed());
}
