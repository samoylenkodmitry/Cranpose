use super::*;

fn pointer(kind: PointerEventKind, x: f32, y: f32) -> PointerEvent {
    PointerEvent::new(kind, Point { x, y }, Point { x, y })
}

#[test]
fn drag_starts_after_threshold_and_consumes_move() {
    let mut gesture = DragGesture::new();
    let down = pointer(PointerEventKind::Down, 0.0, 0.0);
    let move_inside_slop = pointer(PointerEventKind::Move, DRAG_THRESHOLD - 1.0, 0.0);
    let move_after_slop = pointer(PointerEventKind::Move, DRAG_THRESHOLD + 1.0, 0.0);

    assert_eq!(gesture.handle_event(&down), DragGestureEvent::None);
    assert_eq!(
        gesture.handle_event(&move_inside_slop),
        DragGestureEvent::None
    );
    assert!(!move_inside_slop.is_consumed());
    assert_eq!(
        gesture.handle_event(&move_after_slop),
        DragGestureEvent::Started {
            start: Point { x: 0.0, y: 0.0 },
            current: Point {
                x: DRAG_THRESHOLD + 1.0,
                y: 0.0
            },
        }
    );
    assert!(move_after_slop.is_consumed());
}

#[test]
fn drag_reports_delta_after_start() {
    let mut gesture = DragGesture::new();
    gesture.handle_event(&pointer(PointerEventKind::Down, 0.0, 0.0));
    gesture.handle_event(&pointer(PointerEventKind::Move, DRAG_THRESHOLD + 1.0, 0.0));

    assert_eq!(
        gesture.handle_event(&pointer(PointerEventKind::Move, DRAG_THRESHOLD + 3.0, 2.0)),
        DragGestureEvent::Dragged {
            delta: Point { x: 2.0, y: 2.0 },
            total: Point {
                x: DRAG_THRESHOLD + 3.0,
                y: 2.0
            },
            current: Point {
                x: DRAG_THRESHOLD + 3.0,
                y: 2.0
            },
        }
    );
}
