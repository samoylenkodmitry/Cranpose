use std::{cell::RefCell, rc::Rc};

use cranpose_foundation::DRAG_THRESHOLD;

use super::*;

#[derive(Default)]
struct Log {
    lines: RefCell<Vec<String>>,
}

impl Log {
    fn push(&self, line: impl Into<String>) {
        self.lines.borrow_mut().push(line.into());
    }

    fn take(&self) -> Vec<String> {
        std::mem::take(&mut *self.lines.borrow_mut())
    }
}

fn event(kind: PointerEventKind, x: f32, y: f32) -> PointerEvent {
    PointerEvent::new(kind, Point { x, y }, Point { x, y })
        .with_screen_position(Some(Point { x: x + 100.0, y }))
}

fn logged_source(log: &Rc<Log>) -> DragAndDropSource {
    let started = Rc::clone(log);
    let ended = Rc::clone(log);
    DragAndDropSource::new(7u64)
        .on_started(move |point| started.push(format!("started {:?}", point.screen)))
        .on_ended(move |outcome| ended.push(format!("ended {outcome:?}")))
}

fn logged_target(log: &Rc<Log>, accept: bool) -> DragAndDropTarget {
    let entered = Rc::clone(log);
    let moved = Rc::clone(log);
    let exited = Rc::clone(log);
    let dropped = Rc::clone(log);
    DragAndDropTarget::new()
        .on_entered(move |payload| entered.push(format!("entered {}", number(payload))))
        .on_moved(move |_, at| moved.push(format!("moved {},{}", at.x, at.y)))
        .on_exited(move |_| exited.push("exited"))
        .on_drop(move |payload, at| {
            dropped.push(format!("drop {} at {},{}", number(payload), at.x, at.y));
            accept
        })
}

fn number(payload: &DragAndDropPayload) -> u64 {
    payload.downcast_ref::<u64>().copied().unwrap_or(0)
}

fn drag(gesture: &mut SourceGesture, source: &DragAndDropSource, state: &DragAndDropState) {
    gesture.on_event(&event(PointerEventKind::Down, 0.0, 0.0), source, state);
    gesture.on_event(
        &event(PointerEventKind::Move, DRAG_THRESHOLD + 2.0, 0.0),
        source,
        state,
    );
}

#[test]
fn a_press_that_stays_within_the_threshold_starts_nothing() {
    let state = DragAndDropState::default();
    let source = DragAndDropSource::new(7u64);
    let mut gesture = SourceGesture::default();
    gesture.on_event(&event(PointerEventKind::Down, 0.0, 0.0), &source, &state);
    let small_move = event(PointerEventKind::Move, DRAG_THRESHOLD - 1.0, 0.0);
    gesture.on_event(&small_move, &source, &state);
    assert!(!state.is_active(), "no transfer below the drag threshold");
    assert!(
        !small_move.is_consumed(),
        "the press stays a press for others"
    );
    gesture.on_event(&event(PointerEventKind::Up, 3.0, 0.0), &source, &state);
    assert!(!state.route(|_| None), "nothing to route");
}

#[test]
fn a_drag_past_the_threshold_starts_moves_and_drops() {
    let log = Rc::new(Log::default());
    let state = DragAndDropState::default();
    let source = logged_source(&log);
    let mut gesture = SourceGesture::default();
    drag(&mut gesture, &source, &state);
    assert!(state.is_active(), "the transfer started");
    let moved = event(PointerEventKind::Move, 30.0, 0.0);
    gesture.on_event(&moved, &source, &state);
    assert!(moved.is_consumed(), "a running transfer owns the pointer");
    gesture.on_event(&event(PointerEventKind::Up, 40.0, 0.0), &source, &state);
    assert!(state.route(|_| None));
    assert_eq!(
        log.take(),
        vec![
            format!("started {:?}", Some(Point { x: 110.0, y: 0.0 })),
            "ended Missed".to_string()
        ]
    );
    assert!(!state.is_active(), "a release ends the transfer");
}

#[test]
fn routing_tells_the_target_enter_move_exit_and_drop() {
    let log = Rc::new(Log::default());
    let state = DragAndDropState::default();
    state.register_target(5, logged_target(&log, true));
    let source = logged_source(&log);
    let mut gesture = SourceGesture::default();
    drag(&mut gesture, &source, &state);
    let over_target = |point: DragAndDropPoint| (point.local.x > 50.0).then_some((5, point.local));
    assert!(state.route(over_target));
    assert_eq!(log.take(), vec!["started Some(Point { x: 110.0, y: 0.0 })"]);

    gesture.on_event(&event(PointerEventKind::Move, 60.0, 5.0), &source, &state);
    state.route(over_target);
    assert_eq!(log.take(), vec!["entered 7", "moved 60,5"]);

    gesture.on_event(&event(PointerEventKind::Move, 20.0, 5.0), &source, &state);
    state.route(over_target);
    assert_eq!(log.take(), vec!["exited"]);

    gesture.on_event(&event(PointerEventKind::Move, 70.0, 5.0), &source, &state);
    gesture.on_event(&event(PointerEventKind::Up, 70.0, 5.0), &source, &state);
    state.route(over_target);
    assert_eq!(
        log.take(),
        vec![
            "entered 7",
            "moved 70,5",
            "moved 70,5",
            "drop 7 at 70,5",
            "ended Dropped"
        ]
    );
    assert!(!state.is_active());
}

#[test]
fn a_target_that_declines_leaves_the_source_with_a_miss() {
    let log = Rc::new(Log::default());
    let state = DragAndDropState::default();
    state.register_target(5, logged_target(&log, false));
    let source = logged_source(&log);
    let mut gesture = SourceGesture::default();
    drag(&mut gesture, &source, &state);
    gesture.on_event(&event(PointerEventKind::Up, 70.0, 5.0), &source, &state);
    state.route(|point| Some((5, point.local)));
    assert_eq!(
        log.take(),
        vec![
            "started Some(Point { x: 110.0, y: 0.0 })",
            "entered 7",
            "moved 10,0",
            "moved 70,5",
            "drop 7 at 70,5",
            "ended Missed"
        ]
    );
}

#[test]
fn a_cancelled_gesture_exits_the_target_and_ends_cancelled() {
    let log = Rc::new(Log::default());
    let state = DragAndDropState::default();
    state.register_target(5, logged_target(&log, true));
    let source = logged_source(&log);
    let mut gesture = SourceGesture::default();
    drag(&mut gesture, &source, &state);
    state.route(|point| Some((5, point.local)));
    log.take();
    gesture.on_event(&event(PointerEventKind::Cancel, 10.0, 0.0), &source, &state);
    state.route(|point| Some((5, point.local)));
    assert_eq!(log.take(), vec!["exited", "ended Cancelled"]);
    assert!(!state.is_active());
}

#[test]
fn a_target_that_leaves_the_tree_is_forgotten() {
    let state = DragAndDropState::default();
    state.register_target(5, DragAndDropTarget::new());
    assert!(state.is_target(5));
    state.unregister_target(5);
    assert!(!state.is_target(5));
}

#[test]
fn the_target_modifier_registers_its_node_while_attached() {
    let node = Rc::new(std::cell::Cell::new(None));
    let captured = Rc::clone(&node);
    let composition = crate::run_test_composition(move || {
        let id = crate::widgets::Box(
            Modifier::empty().drag_and_drop_target(DragAndDropTarget::new()),
            crate::widgets::BoxSpec::default(),
            || {},
        );
        captured.set(Some(id));
    });
    let id = node.get().expect("the target composed");
    let registered =
        current_app_context().is_some_and(|context| context.drag_and_drop().is_target(id));
    assert!(registered, "the node is a target while attached");
    drop(composition);
}
