use std::sync::Arc;

use cranpose_core::{DefaultScheduler, Runtime};

use super::*;

fn with_runtime<T>(body: impl FnOnce() -> T) -> T {
    let _runtime = Runtime::new(Arc::new(DefaultScheduler));
    body()
}

#[test]
fn a_remembered_drag_state_survives_recomposition_and_takes_the_new_handler() {
    use cranpose_core::{Composition, MemoryApplier, location_key};

    let mut composition = Composition::new(MemoryApplier::new());
    let seen = Rc::new(RefCell::new(Vec::new()));
    let handles: Rc<RefCell<Vec<DraggableState>>> = Rc::new(RefCell::new(Vec::new()));
    let pass = Rc::new(Cell::new(0usize));

    let key = location_key(file!(), line!(), column!());
    for _ in 0..2 {
        let seen = Rc::clone(&seen);
        let handles = Rc::clone(&handles);
        let pass = Rc::clone(&pass);
        let mut render = move || {
            let tag = pass.get();
            pass.set(tag + 1);
            let recorder = Rc::clone(&seen);
            let state = rememberDraggableState(move |delta| {
                recorder.borrow_mut().push((tag, delta));
            });
            handles.borrow_mut().push(state);
        };
        composition.render(key, &mut render).expect("render");
    }

    let handles = handles.borrow();
    assert_eq!(handles.len(), 2);
    assert!(
        handles[0] == handles[1],
        "a remembered state must survive the slot"
    );

    handles[1].drag_by(3.0);
    assert_eq!(
        *seen.borrow(),
        vec![(1, 3.0)],
        "the delta must reach the handler the latest pass supplied"
    );
}

#[test]
fn a_drag_delta_reaches_the_current_handler() {
    with_runtime(|| {
        let seen = Rc::new(RefCell::new(Vec::new()));
        let recorder = Rc::clone(&seen);
        let state = DraggableState::new(move |delta| recorder.borrow_mut().push(delta));
        state.drag_by(4.0);
        state.drag_by(-1.5);
        assert_eq!(seen.borrow().as_slice(), [4.0, -1.5]);
        assert_eq!(state.offset(), 2.5);
    });
}

#[test]
fn replacing_the_handler_redirects_later_deltas() {
    with_runtime(|| {
        let first = Rc::new(Cell::new(0.0));
        let second = Rc::new(Cell::new(0.0));
        let recorder = Rc::clone(&first);
        let state = DraggableState::new(move |delta| recorder.set(recorder.get() + delta));
        state.drag_by(2.0);
        let recorder = Rc::clone(&second);
        state.update_handler(move |delta| recorder.set(recorder.get() + delta));
        state.drag_by(3.0);
        assert_eq!(first.get(), 2.0);
        assert_eq!(second.get(), 3.0);
    });
}

#[test]
fn a_delta_that_is_not_a_movement_is_not_delivered() {
    with_runtime(|| {
        let count = Rc::new(Cell::new(0u32));
        let recorder = Rc::clone(&count);
        let state = DraggableState::new(move |_| recorder.set(recorder.get() + 1));
        state.drag_by(0.0);
        state.drag_by(f32::NAN);
        assert_eq!(count.get(), 0);
        assert_eq!(state.offset(), 0.0);
    });
}

#[test]
fn dragging_is_observable_and_clones_share_it() {
    with_runtime(|| {
        let state = DraggableState::new(|_| {});
        let handle = state.clone();
        assert!(!handle.is_dragging());
        state.set_dragging(true);
        assert!(handle.is_dragging());
        state.set_dragging(false);
        assert!(!handle.is_dragging());
        assert!(state == handle);
    });
}
