use super::StdRuntime;
use compose_core::{location_key, Composition, MemoryApplier, MutableState};
use std::cell::{Cell, RefCell};
use std::rc::Rc;

#[test]
fn std_runtime_requests_frame_and_recomposes_on_state_change() {
    fn compose_counter_body(
        recompositions: &Rc<Cell<u32>>,
        state_slot: &Rc<RefCell<Option<MutableState<i32>>>>,
    ) {
        recompositions.set(recompositions.get() + 1);
        let state = compose_core::useState(|| 0);
        state_slot.borrow_mut().replace(state);
        let _ = state.value();
    }

    let runtime = StdRuntime::new();
    let mut composition = Composition::with_runtime(MemoryApplier::new(), runtime.runtime());
    let root_key = location_key(file!(), line!(), column!());

    let recompositions = Rc::new(Cell::new(0u32));
    let state_slot: Rc<RefCell<Option<MutableState<i32>>>> = Rc::new(RefCell::new(None));

    let mut content = {
        let recompositions = recompositions.clone();
        let state_slot = state_slot.clone();
        move || {
            compose_core::with_current_composer(|composer| {
                let recompositions_cb = recompositions.clone();
                let state_slot_cb = state_slot.clone();
                composer.set_recompose_callback(move |_composer| {
                    compose_counter_body(&recompositions_cb, &state_slot_cb);
                });
            });
            compose_counter_body(&recompositions, &state_slot);
        }
    };

    composition
        .render(root_key, &mut content)
        .expect("initial render");
    assert_eq!(recompositions.get(), 1);

    let state = state_slot
        .borrow()
        .as_ref()
        .cloned()
        .expect("state captured during composition");

    state.set(1);

    assert!(
        runtime.take_frame_request(),
        "state.set should request a frame"
    );

    let runtime_handle = composition.runtime_handle();
    runtime_handle.drain_ui();
    composition
        .process_invalid_scopes()
        .expect("process invalid scopes after state change");

    assert_eq!(
        recompositions.get(),
        2,
        "state change should trigger recomposition"
    );
    assert_eq!(state.value(), 1);
}
