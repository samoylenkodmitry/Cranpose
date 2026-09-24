use cranpose_ui_graphics::Point;

use super::*;
use crate::nodes::input::PointerEventKind;

fn event(id: u64, x: f32) -> PointerEvent {
    let position = Point { x, y: 0.0 };
    let mut event = PointerEvent::new(PointerEventKind::Move, position, position);
    event.id = id;
    event
}

#[test]
fn drain_dispatches_events_in_fifo_order_and_clears_queue() {
    let mut dispatcher = PointerDispatcher::new();
    dispatcher.push(event(1, 10.0));
    dispatcher.push(event(2, 20.0));

    let mut drained = Vec::new();
    dispatcher.drain(|id, event| drained.push((id, event.position.x)));

    assert_eq!(drained, vec![(1, 10.0), (2, 20.0)]);
    assert!(dispatcher.is_empty());
}

#[test]
fn clear_drops_queued_events() {
    let mut dispatcher = PointerDispatcher::new();
    dispatcher.push(event(1, 10.0));
    assert_eq!(dispatcher.len(), 1);

    dispatcher.clear();

    assert!(dispatcher.is_empty());
}
