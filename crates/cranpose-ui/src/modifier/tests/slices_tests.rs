use std::cell::RefCell;

use super::*;

#[test]
fn local_pointer_dispatch_preserves_consumption_and_terminal_delivery() {
    let delivered = Rc::new(RefCell::new(Vec::new()));
    let first = Rc::clone(&delivered);
    let second = Rc::clone(&delivered);
    let slices = ModifierNodeSlices {
        pointer_inputs: vec![
            Rc::new(move |event| {
                first.borrow_mut().push((1, event.kind, event.position));
                event.consume();
            }),
            Rc::new(move |event| second.borrow_mut().push((2, event.kind, event.position))),
        ],
        ..Default::default()
    };
    let local = Point { x: 12.0, y: 8.0 };
    for kind in [
        PointerEventKind::Down,
        PointerEventKind::Up,
        PointerEventKind::Cancel,
    ] {
        slices.dispatch_pointer_event(PointerEvent::new(kind, local, local));
    }
    assert_eq!(
        *delivered.borrow(),
        [
            (1, PointerEventKind::Down, local),
            (1, PointerEventKind::Up, local),
            (2, PointerEventKind::Up, local),
            (1, PointerEventKind::Cancel, local),
            (2, PointerEventKind::Cancel, local),
        ]
    );
}

#[test]
fn local_pointer_dispatch_calls_click_handlers_only_for_an_unconsumed_press() {
    let clicks = Rc::new(RefCell::new(Vec::new()));
    let recorded = Rc::clone(&clicks);
    let slices = ModifierNodeSlices {
        click_handlers: vec![Rc::new(move |position| {
            recorded.borrow_mut().push(position);
        })],
        ..Default::default()
    };
    let local = Point { x: 24.0, y: 16.0 };
    slices.dispatch_pointer_event(PointerEvent::new(PointerEventKind::Down, local, local));
    slices.dispatch_pointer_event(PointerEvent::new(PointerEventKind::Up, local, local));
    let consumed = PointerEvent::new(PointerEventKind::Down, local, local);
    consumed.consume();
    slices.dispatch_pointer_event(consumed);
    assert_eq!(*clicks.borrow(), [local]);
}
