use super::StdRuntime;
use cranpose_core::{location_key, Composition, MemoryApplier, MutableState, RuntimeScheduler};
use std::cell::{Cell, RefCell};
use std::panic::{self, AssertUnwindSafe};
use std::rc::Rc;
#[cfg(not(target_arch = "wasm32"))]
use std::sync::atomic::{AtomicBool, Ordering};
#[cfg(not(target_arch = "wasm32"))]
use std::sync::Arc;

#[test]
fn std_runtime_requests_frame_and_recomposes_on_state_change() {
    fn cranpose_counter_body(
        recompositions: &Rc<Cell<u32>>,
        state_slot: &Rc<RefCell<Option<MutableState<i32>>>>,
    ) {
        recompositions.set(recompositions.get() + 1);
        let state = cranpose_core::rememberMutableStateOf(|| 0);
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
            cranpose_core::with_current_composer(|composer| {
                let recompositions_cb = recompositions.clone();
                let state_slot_cb = state_slot.clone();
                composer.set_recompose_callback(move |_composer| {
                    cranpose_counter_body(&recompositions_cb, &state_slot_cb);
                });
            });
            cranpose_counter_body(&recompositions, &state_slot);
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
        runtime.has_frame_request(),
        "has_frame_request should observe pending frames without consuming them"
    );
    assert!(
        runtime.take_frame_request(),
        "state.set should request a frame"
    );
    assert!(
        !runtime.has_frame_request(),
        "take_frame_request should consume the pending frame"
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

#[cfg(not(target_arch = "wasm32"))]
#[test]
fn native_frame_waker_replacement_panic_does_not_poison_scheduler() {
    struct PanicOnDrop;

    impl Drop for PanicOnDrop {
        fn drop(&mut self) {
            panic!("frame waker drop panic");
        }
    }

    let runtime = StdRuntime::new();
    let drop_marker = PanicOnDrop;
    runtime.set_frame_waker(move || {
        let _ = &drop_marker;
    });

    let replacement = panic::catch_unwind(AssertUnwindSafe(|| {
        runtime.set_frame_waker(|| {});
    }));
    assert!(
        replacement.is_err(),
        "replacing the panic-on-drop waker should still surface the waker drop panic"
    );

    let woke = Arc::new(AtomicBool::new(false));
    let woke_for_waker = Arc::clone(&woke);
    runtime.set_frame_waker(move || {
        woke_for_waker.store(true, Ordering::SeqCst);
    });

    runtime.scheduler().schedule_frame();
    assert!(
        woke.load(Ordering::SeqCst),
        "scheduler should recover the frame-waker lock after a replacement panic"
    );
}

#[cfg(not(target_arch = "wasm32"))]
#[test]
fn native_frame_waker_clear_panic_does_not_poison_scheduler() {
    struct PanicOnDrop;

    impl Drop for PanicOnDrop {
        fn drop(&mut self) {
            panic!("frame waker drop panic");
        }
    }

    let runtime = StdRuntime::new();
    let drop_marker = PanicOnDrop;
    runtime.set_frame_waker(move || {
        let _ = &drop_marker;
    });

    let clear = panic::catch_unwind(AssertUnwindSafe(|| {
        runtime.clear_frame_waker();
    }));
    assert!(
        clear.is_err(),
        "clearing the panic-on-drop waker should still surface the waker drop panic"
    );

    let woke = Arc::new(AtomicBool::new(false));
    let woke_for_waker = Arc::clone(&woke);
    runtime.set_frame_waker(move || {
        woke_for_waker.store(true, Ordering::SeqCst);
    });

    runtime.scheduler().schedule_frame();
    assert!(
        woke.load(Ordering::SeqCst),
        "scheduler should recover the frame-waker lock after a clear panic"
    );
}
