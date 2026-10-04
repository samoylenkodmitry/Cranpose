use std::panic::{AssertUnwindSafe, catch_unwind};

use cranpose_core::{
    Composition, MemoryApplier,
    composer_context::{current_composer, with_composer},
    location_key, mutableStateOf, run_in_mutable_snapshot,
    snapshot_v2::take_mutable_snapshot,
};

#[test]
fn nested_compositions_restore_the_outer_composer_after_return_and_unwind() {
    for panic_in_inner in [false, true] {
        let mut outer = Composition::new(MemoryApplier::new());
        let outer_runtime = outer.runtime_handle().id();
        outer
            .render(location_key(file!(), line!(), column!()), || {
                with_composer(|composer| {
                    assert_eq!(composer.runtime_handle().id(), outer_runtime);
                    let mut inner = Composition::new(MemoryApplier::new());
                    let inner_runtime = inner.runtime_handle().id();
                    assert_ne!(outer_runtime, inner_runtime);
                    let result = catch_unwind(AssertUnwindSafe(|| {
                        inner
                            .render(location_key(file!(), line!(), column!()), || {
                                with_composer(|composer| {
                                    assert_eq!(composer.runtime_handle().id(), inner_runtime);
                                });
                                assert!(!panic_in_inner, "unwind the nested composition");
                            })
                            .expect("nested render succeeds");
                    }));
                    assert_eq!(result.is_err(), panic_in_inner);
                    with_composer(|composer| {
                        assert_eq!(composer.runtime_handle().id(), outer_runtime);
                    });
                });
            })
            .expect("outer render succeeds");
        assert!(current_composer().is_none());
    }
}

#[test]
fn unwinding_a_snapshot_restores_the_surrounding_transaction() {
    let _composition = Composition::new(MemoryApplier::new());
    let state = mutableStateOf(1);
    run_in_mutable_snapshot(|| {
        state.set(2);
        let child = take_mutable_snapshot(None, None);
        let result = catch_unwind(AssertUnwindSafe(|| {
            child.enter(|| {
                state.set(3);
                assert_eq!(state.get(), 3);
                panic!("discard the nested transaction");
            });
        }));
        assert!(result.is_err());
        assert_eq!(state.get(), 2);
        child.dispose();
        state.set(4);
    })
    .expect("outer transaction applies");
    assert_eq!(state.get(), 4);
}
