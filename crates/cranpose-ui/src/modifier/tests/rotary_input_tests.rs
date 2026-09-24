use std::cell::RefCell;

use cranpose_ui_graphics::Point;

use super::*;

fn rotary_event(kind: PointerEventKind, vertical: f32) -> PointerEvent {
    PointerEvent::rotary(
        kind,
        RotaryScrollEvent::new(vertical, 0.0, 42),
        Point { x: 0.0, y: 0.0 },
    )
}

#[test]
fn bubble_node_receives_bubble_events_only() {
    let seen = Rc::new(RefCell::new(Vec::new()));
    let sink = Rc::clone(&seen);
    let node = RotaryInputModifierNode::new(
        RotaryPass::Bubble,
        Rc::new(move |event: RotaryScrollEvent| {
            sink.borrow_mut().push(event.vertical_scroll_pixels);
            false
        }),
    );
    let dispatch = node.pointer_input_handler().expect("handler");

    dispatch(rotary_event(PointerEventKind::RotaryScrollPre, -1.0));
    dispatch(rotary_event(PointerEventKind::RotaryScroll, -2.0));
    dispatch(PointerEvent::new(
        PointerEventKind::Scroll,
        Point { x: 0.0, y: 0.0 },
        Point { x: 0.0, y: 0.0 },
    ));

    assert_eq!(*seen.borrow(), vec![-2.0]);
}

#[test]
fn pre_node_receives_capture_events_only() {
    let seen = Rc::new(RefCell::new(Vec::new()));
    let sink = Rc::clone(&seen);
    let node = RotaryInputModifierNode::new(
        RotaryPass::Pre,
        Rc::new(move |event: RotaryScrollEvent| {
            sink.borrow_mut().push(event.vertical_scroll_pixels);
            false
        }),
    );
    let dispatch = node.pointer_input_handler().expect("handler");

    dispatch(rotary_event(PointerEventKind::RotaryScrollPre, -1.0));
    dispatch(rotary_event(PointerEventKind::RotaryScroll, -2.0));

    assert_eq!(*seen.borrow(), vec![-1.0]);
}

#[test]
fn returning_true_consumes_the_event() {
    let node = RotaryInputModifierNode::new(RotaryPass::Bubble, Rc::new(|_| true));
    let dispatch = node.pointer_input_handler().expect("handler");

    let event = rotary_event(PointerEventKind::RotaryScroll, -8.0);
    dispatch(event.clone());

    assert!(event.is_consumed());
}

#[test]
fn returning_false_leaves_the_event_unconsumed() {
    let node = RotaryInputModifierNode::new(RotaryPass::Bubble, Rc::new(|_| false));
    let dispatch = node.pointer_input_handler().expect("handler");

    let event = rotary_event(PointerEventKind::RotaryScroll, -8.0);
    dispatch(event.clone());

    assert!(!event.is_consumed());
}

#[test]
fn an_already_consumed_event_never_reaches_the_handler() {
    let calls = Rc::new(Cell::new(0));
    let counter = Rc::clone(&calls);
    let node = RotaryInputModifierNode::new(
        RotaryPass::Bubble,
        Rc::new(move |_| {
            counter.set(counter.get() + 1);
            false
        }),
    );
    let dispatch = node.pointer_input_handler().expect("handler");

    let event = rotary_event(PointerEventKind::RotaryScroll, -8.0);
    event.consume();
    dispatch(event);

    assert_eq!(calls.get(), 0);
}

#[test]
fn handler_sees_the_full_rotary_payload() {
    let captured = Rc::new(Cell::new(None));
    let sink = Rc::clone(&captured);
    let node = RotaryInputModifierNode::new(
        RotaryPass::Bubble,
        Rc::new(move |event: RotaryScrollEvent| {
            sink.set(Some(event));
            true
        }),
    );
    let dispatch = node.pointer_input_handler().expect("handler");

    dispatch(PointerEvent::rotary(
        PointerEventKind::RotaryScroll,
        RotaryScrollEvent::new(-64.0, 32.0, 777),
        Point { x: 0.0, y: 0.0 },
    ));

    assert_eq!(
        captured.get(),
        Some(RotaryScrollEvent::new(-64.0, 32.0, 777))
    );
}

#[test]
fn element_reuses_the_node_across_recomposition() {
    let first = RotaryInputElement::new(RotaryPass::Bubble, Rc::new(|_| false));
    let second = RotaryInputElement::new(RotaryPass::Bubble, Rc::new(|_| true));
    assert_eq!(first, second);
    assert!(first.always_update());

    let mut node = first.create();
    second.update(&mut node);

    let event = rotary_event(PointerEventKind::RotaryScroll, -1.0);
    node.pointer_input_handler().expect("handler")(event.clone());

    assert!(event.is_consumed(), "updated handler should be in effect");
}

#[test]
fn pre_and_bubble_elements_are_distinct() {
    let pre = RotaryInputElement::new(RotaryPass::Pre, Rc::new(|_| false));
    let bubble = RotaryInputElement::new(RotaryPass::Bubble, Rc::new(|_| false));

    assert_ne!(pre, bubble);
}

#[test]
fn modifier_builders_add_pointer_input_elements() {
    let modifier = Modifier::empty()
        .on_pre_rotary_scroll_event(|_| false)
        .on_rotary_scroll_event(|_| true);

    assert_eq!(modifier.elements().len(), 2);
}
