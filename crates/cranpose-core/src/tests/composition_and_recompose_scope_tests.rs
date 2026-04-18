use super::*;

#[test]
fn composition_local_provider_scopes_values() {
    thread_local! {
        static CHILD_RECOMPOSITIONS: Cell<usize> = const { Cell::new(0) };
        static LAST_VALUE: Cell<i32> = const { Cell::new(0) };
    }

    let local_counter = compositionLocalOf(|| 0);
    let mut composition = test_composition();
    let runtime = composition.runtime_handle();
    let provided_state = MutableState::with_runtime(1, runtime.clone());

    #[composable]
    fn child(local_counter: CompositionLocal<i32>) {
        CHILD_RECOMPOSITIONS.with(|count| count.set(count.get() + 1));
        let value = local_counter.current();
        LAST_VALUE.with(|slot| slot.set(value));
    }

    #[composable]
    fn parent(local_counter: CompositionLocal<i32>, state: MutableState<i32>) {
        CompositionLocalProvider(vec![local_counter.provides(state.value())], || {
            child(local_counter.clone());
        });
    }

    composition
        .render(1, || parent(local_counter.clone(), provided_state))
        .expect("initial composition");

    assert_eq!(CHILD_RECOMPOSITIONS.with(|c| c.get()), 1);
    assert_eq!(LAST_VALUE.with(|slot| slot.get()), 1);

    provided_state.set_value(5);
    let _ = composition
        .process_invalid_scopes()
        .expect("process local change");

    assert_eq!(CHILD_RECOMPOSITIONS.with(|c| c.get()), 2);
    assert_eq!(LAST_VALUE.with(|slot| slot.get()), 5);
}

#[test]
fn composition_local_default_value_used_outside_provider() {
    thread_local! {
        static READ_VALUE: Cell<i32> = const { Cell::new(0) };
    }

    let local_counter = compositionLocalOf(|| 7);
    let mut composition = test_composition();

    #[composable]
    fn reader(local_counter: CompositionLocal<i32>) {
        let value = local_counter.current();
        READ_VALUE.with(|slot| slot.set(value));
    }

    composition
        .render(2, || reader(local_counter.clone()))
        .expect("compose reader");

    assert_eq!(READ_VALUE.with(|slot| slot.get()), 7);
}

#[test]
fn composition_local_simple_subscription_test() {
    // Simplified test to verify basic subscription behavior
    thread_local! {
        static READER_RECOMPOSITIONS: Cell<usize> = const { Cell::new(0) };
        static LAST_VALUE: Cell<i32> = const { Cell::new(-1) };
    }

    let local_value = compositionLocalOf(|| 0);
    let mut composition = test_composition();
    let runtime = composition.runtime_handle();
    let trigger = MutableState::with_runtime(10, runtime.clone());

    #[composable]
    fn reader(local_value: CompositionLocal<i32>) {
        READER_RECOMPOSITIONS.with(|c| c.set(c.get() + 1));
        let val = local_value.current();
        LAST_VALUE.with(|v| v.set(val));
    }

    #[composable]
    fn root(local_value: CompositionLocal<i32>, trigger: MutableState<i32>) {
        let val = trigger.value();
        println!("root sees trigger value {}", val);
        CompositionLocalProvider(vec![local_value.provides(val)], || {
            reader(local_value.clone());
        });
    }

    composition
        .render(1, || root(local_value.clone(), trigger))
        .expect("initial composition");

    println!(
        "initial recompositions={}, last={}",
        READER_RECOMPOSITIONS.with(|c| c.get()),
        LAST_VALUE.with(|v| v.get())
    );
    assert_eq!(READER_RECOMPOSITIONS.with(|c| c.get()), 1);
    assert_eq!(LAST_VALUE.with(|v| v.get()), 10);

    // Change trigger - should update the provided value and reader should see it
    trigger.set_value(20);
    let _ = composition.process_invalid_scopes().expect("recomposition");

    // Reader should have recomposed and seen the new value
    println!(
        "after update recompositions={}, last={}",
        READER_RECOMPOSITIONS.with(|c| c.get()),
        LAST_VALUE.with(|v| v.get())
    );
    assert_eq!(
        READER_RECOMPOSITIONS.with(|c| c.get()),
        2,
        "reader should recompose"
    );
    assert_eq!(
        LAST_VALUE.with(|v| v.get()),
        20,
        "reader should see new value"
    );
}

#[test]
fn composition_local_unchanged_value_does_not_reinvalidate_reader() {
    thread_local! {
        static ROOT_RECOMPOSITIONS: Cell<usize> = const { Cell::new(0) };
        static READER_RECOMPOSITIONS: Cell<usize> = const { Cell::new(0) };
        static LAST_VALUE: Cell<i32> = const { Cell::new(-1) };
    }

    let local_value = compositionLocalOf(|| 7);
    let mut composition = test_composition();
    let runtime = composition.runtime_handle();
    let trigger = MutableState::with_runtime(0, runtime.clone());

    #[composable]
    fn reader(local_value: CompositionLocal<i32>) {
        READER_RECOMPOSITIONS.with(|c| c.set(c.get() + 1));
        LAST_VALUE.with(|v| v.set(local_value.current()));
    }

    #[composable]
    fn root(local_value: CompositionLocal<i32>, trigger: MutableState<i32>) {
        ROOT_RECOMPOSITIONS.with(|c| c.set(c.get() + 1));
        let _ = trigger.value();
        CompositionLocalProvider(vec![local_value.provides(7)], || {
            reader(local_value.clone());
        });
    }

    composition
        .render(1, || root(local_value.clone(), trigger))
        .expect("initial composition");

    assert_eq!(ROOT_RECOMPOSITIONS.with(|c| c.get()), 1);
    assert_eq!(READER_RECOMPOSITIONS.with(|c| c.get()), 1);
    assert_eq!(LAST_VALUE.with(|v| v.get()), 7);

    trigger.set_value(1);
    let did_recompose = composition
        .process_invalid_scopes()
        .expect("process unchanged provider value");

    assert!(did_recompose, "parent should recompose for trigger change");
    assert_eq!(ROOT_RECOMPOSITIONS.with(|c| c.get()), 2);
    assert_eq!(
        READER_RECOMPOSITIONS.with(|c| c.get()),
        1,
        "unchanged provided value must not invalidate the reader",
    );
    assert_eq!(LAST_VALUE.with(|v| v.get()), 7);
    assert!(
        !runtime.has_invalid_scopes(),
        "unchanged provider value must not leave invalid scopes behind",
    );
}

#[test]
fn composition_local_custom_policy_uses_equivalence_for_updates() {
    thread_local! {
        static ROOT_RECOMPOSITIONS: Cell<usize> = const { Cell::new(0) };
        static READER_RECOMPOSITIONS: Cell<usize> = const { Cell::new(0) };
        static LAST_VALUE: Cell<i32> = const { Cell::new(-1) };
    }

    let local_value = compositionLocalOfWithPolicy(
        || Arc::new(0),
        |current: &Arc<i32>, next: &Arc<i32>| Arc::ptr_eq(current, next),
    );
    let mut composition = test_composition();
    let runtime = composition.runtime_handle();
    let shared = Arc::new(7);
    let provided_state = MutableState::with_runtime(shared.clone(), runtime.clone());

    #[composable]
    fn reader(local_value: CompositionLocal<Arc<i32>>) {
        READER_RECOMPOSITIONS.with(|c| c.set(c.get() + 1));
        LAST_VALUE.with(|v| v.set(*local_value.current()));
    }

    #[composable]
    fn root(local_value: CompositionLocal<Arc<i32>>, provided_state: MutableState<Arc<i32>>) {
        ROOT_RECOMPOSITIONS.with(|c| c.set(c.get() + 1));
        let current = provided_state.value();
        CompositionLocalProvider(vec![local_value.provides(current.clone())], || {
            reader(local_value.clone());
        });
    }

    composition
        .render(1, || root(local_value.clone(), provided_state))
        .expect("initial composition");

    assert_eq!(ROOT_RECOMPOSITIONS.with(|c| c.get()), 1);
    assert_eq!(READER_RECOMPOSITIONS.with(|c| c.get()), 1);
    assert_eq!(LAST_VALUE.with(|v| v.get()), 7);

    provided_state.set_value(shared.clone());
    let did_recompose = composition
        .process_invalid_scopes()
        .expect("process equivalent provider value");

    assert!(
        did_recompose,
        "provider scope should recompose for state change"
    );
    assert_eq!(ROOT_RECOMPOSITIONS.with(|c| c.get()), 2);
    assert_eq!(
        READER_RECOMPOSITIONS.with(|c| c.get()),
        1,
        "equivalent pointer value must not invalidate the reader",
    );
    assert_eq!(LAST_VALUE.with(|v| v.get()), 7);
    assert!(
        !runtime.has_invalid_scopes(),
        "equivalent provider value must not leave invalid scopes behind",
    );

    provided_state.set_value(Arc::new(9));
    let did_recompose = composition
        .process_invalid_scopes()
        .expect("process distinct provider value");

    assert!(did_recompose, "distinct provider value should recompose");
    assert_eq!(ROOT_RECOMPOSITIONS.with(|c| c.get()), 3);
    assert_eq!(READER_RECOMPOSITIONS.with(|c| c.get()), 2);
    assert_eq!(LAST_VALUE.with(|v| v.get()), 9);
    assert!(
        !runtime.has_invalid_scopes(),
        "distinct provider value should settle after recomposition",
    );
}

#[test]
fn composition_local_tracks_reads_and_recomposes_selectively() {
    // This test verifies that CompositionLocal establishes subscriptions
    // and ONLY recomposes composables that actually read .current()
    thread_local! {
        static OUTSIDE_RECOMPOSITIONS: Cell<usize> = const { Cell::new(0) };
        static NOT_CHANGING_TEXT_RECOMPOSITIONS: Cell<usize> = const { Cell::new(0) };
        static INSIDE_RECOMPOSITIONS: Cell<usize> = const { Cell::new(0) };
        static READING_TEXT_RECOMPOSITIONS: Cell<usize> = const { Cell::new(0) };
        static NON_READING_TEXT_RECOMPOSITIONS: Cell<usize> = const { Cell::new(0) };
        static INSIDE_INSIDE_RECOMPOSITIONS: Cell<usize> = const { Cell::new(0) };
        static LAST_READ_VALUE: Cell<i32> = const { Cell::new(-999) };
    }

    let local_count = compositionLocalOf(|| 0);
    let mut composition = test_composition();
    let runtime = composition.runtime_handle();
    let trigger = MutableState::with_runtime(0, runtime.clone());

    #[composable]
    fn inside_inside() {
        INSIDE_INSIDE_RECOMPOSITIONS.with(|c| c.set(c.get() + 1));
        // Does NOT read LocalCount - should NOT recompose when it changes
    }

    #[composable]
    fn inside(local_count: CompositionLocal<i32>) {
        INSIDE_RECOMPOSITIONS.with(|c| c.set(c.get() + 1));
        // Does NOT read LocalCount directly - should NOT recompose when it changes

        // This text reads the local - SHOULD recompose
        #[composable]
        fn reading_text(local_count: CompositionLocal<i32>) {
            READING_TEXT_RECOMPOSITIONS.with(|c| c.set(c.get() + 1));
            let count = local_count.current();
            LAST_READ_VALUE.with(|v| v.set(count));
        }

        reading_text(local_count.clone());

        // This text does NOT read the local - should NOT recompose
        #[composable]
        fn non_reading_text() {
            NON_READING_TEXT_RECOMPOSITIONS.with(|c| c.set(c.get() + 1));
        }

        non_reading_text();
        inside_inside();
    }

    #[composable]
    fn not_changing_text() {
        NOT_CHANGING_TEXT_RECOMPOSITIONS.with(|c| c.set(c.get() + 1));
        // Does NOT read anything reactive - should NOT recompose
    }

    #[composable]
    fn outside(local_count: CompositionLocal<i32>, trigger: MutableState<i32>) {
        OUTSIDE_RECOMPOSITIONS.with(|c| c.set(c.get() + 1));
        let count = trigger.value(); // Read trigger to establish subscription
        CompositionLocalProvider(vec![local_count.provides(count)], || {
            // Directly call reading_text without the inside() wrapper
            #[composable]
            fn reading_text(local_count: CompositionLocal<i32>) {
                READING_TEXT_RECOMPOSITIONS.with(|c| c.set(c.get() + 1));
                let count = local_count.current();
                LAST_READ_VALUE.with(|v| v.set(count));
            }

            not_changing_text();
            reading_text(local_count.clone());
        });
    }

    // Initial composition
    composition
        .render(1, || outside(local_count.clone(), trigger))
        .expect("initial composition");

    assert_eq!(OUTSIDE_RECOMPOSITIONS.with(|c| c.get()), 1);
    assert_eq!(NOT_CHANGING_TEXT_RECOMPOSITIONS.with(|c| c.get()), 1);
    assert_eq!(READING_TEXT_RECOMPOSITIONS.with(|c| c.get()), 1);
    assert_eq!(LAST_READ_VALUE.with(|v| v.get()), 0);

    // Change the trigger - this should update the provided value
    trigger.set_value(1);
    let _ = composition
        .process_invalid_scopes()
        .expect("process recomposition");

    // Expected behavior:
    // - outside: RECOMPOSES (reads trigger.value())
    // - not_changing_text: SKIPPED (no reactive reads)
    // - reading_text: RECOMPOSES (reads local_count.current())

    assert_eq!(
        OUTSIDE_RECOMPOSITIONS.with(|c| c.get()),
        2,
        "outside should recompose"
    );
    assert_eq!(
        NOT_CHANGING_TEXT_RECOMPOSITIONS.with(|c| c.get()),
        1,
        "not_changing_text should NOT recompose"
    );
    assert_eq!(
        READING_TEXT_RECOMPOSITIONS.with(|c| c.get()),
        2,
        "reading_text SHOULD recompose (reads .current())"
    );
    assert_eq!(
        LAST_READ_VALUE.with(|v| v.get()),
        1,
        "should read new value"
    );

    // Change again
    trigger.set_value(2);
    let _ = composition
        .process_invalid_scopes()
        .expect("process second recomposition");

    assert_eq!(OUTSIDE_RECOMPOSITIONS.with(|c| c.get()), 3);
    assert_eq!(NOT_CHANGING_TEXT_RECOMPOSITIONS.with(|c| c.get()), 1);
    assert_eq!(READING_TEXT_RECOMPOSITIONS.with(|c| c.get()), 3);
    assert_eq!(LAST_READ_VALUE.with(|v| v.get()), 2);
}

#[test]
fn static_composition_local_provides_values() {
    thread_local! {
        static READ_VALUE: Cell<i32> = const { Cell::new(0) };
    }

    let local_counter = staticCompositionLocalOf(|| 0);
    let mut composition = test_composition();

    #[composable]
    fn reader(local_counter: StaticCompositionLocal<i32>) {
        let value = local_counter.current();
        READ_VALUE.with(|slot| slot.set(value));
    }

    composition
        .render(1, || {
            CompositionLocalProvider(vec![local_counter.provides(5)], || {
                reader(local_counter.clone());
            })
        })
        .expect("initial composition");

    // Verify the provided value is accessible
    assert_eq!(READ_VALUE.with(|slot| slot.get()), 5);
}

#[test]
fn static_composition_local_default_value_used_outside_provider() {
    thread_local! {
        static READ_VALUE: Cell<i32> = const { Cell::new(0) };
    }

    let local_counter = staticCompositionLocalOf(|| 7);
    let mut composition = test_composition();

    #[composable]
    fn reader(local_counter: StaticCompositionLocal<i32>) {
        let value = local_counter.current();
        READ_VALUE.with(|slot| slot.set(value));
    }

    composition
        .render(2, || reader(local_counter.clone()))
        .expect("compose reader");

    assert_eq!(READ_VALUE.with(|slot| slot.get()), 7);
}

#[test]
fn cranpose_with_reuse_skips_then_recomposes() {
    thread_local! {
        static INVOCATIONS: Cell<usize> = const { Cell::new(0) };
    }

    let mut composition = test_composition();
    let runtime = composition.runtime_handle();
    let state = MutableState::with_runtime(0, runtime.clone());
    let root_key = location_key(file!(), line!(), column!());
    let slot_key = location_key(file!(), line!(), column!());

    let mut render_with_options = |options: RecomposeOptions| {
        let state_clone = state;
        composition
            .render(root_key, || {
                let local_state = state_clone;
                with_current_composer(|composer| {
                    composer.cranpose_with_reuse(slot_key, options, |composer| {
                        let scope = composer
                            .current_recranpose_scope()
                            .expect("scope available");
                        let changed = scope.should_recompose();
                        let has_previous = composer.remember(|| false);
                        if !changed && has_previous.with(|value| *value) {
                            composer.skip_current_group();
                            return;
                        }
                        has_previous.update(|value| *value = true);
                        INVOCATIONS.with(|count| count.set(count.get() + 1));
                        let _ = local_state.value();
                    });
                });
            })
            .expect("render with options");
    };

    render_with_options(RecomposeOptions::default());

    assert_eq!(INVOCATIONS.with(|count| count.get()), 1);

    state.set_value(1);

    render_with_options(RecomposeOptions {
        force_reuse: true,
        ..Default::default()
    });

    assert_eq!(INVOCATIONS.with(|count| count.get()), 1);
}

#[test]
fn cranpose_with_reuse_forces_recomposition_when_requested() {
    thread_local! {
        static INVOCATIONS: Cell<usize> = const { Cell::new(0) };
    }

    let mut composition = test_composition();
    let runtime = composition.runtime_handle();
    let state = MutableState::with_runtime(0, runtime.clone());
    let root_key = location_key(file!(), line!(), column!());
    let slot_key = location_key(file!(), line!(), column!());

    let mut render_with_options = |options: RecomposeOptions| {
        let state_clone = state;
        composition
            .render(root_key, || {
                let local_state = state_clone;
                with_current_composer(|composer| {
                    composer.cranpose_with_reuse(slot_key, options, |composer| {
                        let scope = composer
                            .current_recranpose_scope()
                            .expect("scope available");
                        let changed = scope.should_recompose();
                        let has_previous = composer.remember(|| false);
                        if !changed && has_previous.with(|value| *value) {
                            composer.skip_current_group();
                            return;
                        }
                        has_previous.update(|value| *value = true);
                        INVOCATIONS.with(|count| count.set(count.get() + 1));
                        let _ = local_state.value();
                    });
                });
            })
            .expect("render with options");
    };

    render_with_options(RecomposeOptions::default());

    assert_eq!(INVOCATIONS.with(|count| count.get()), 1);

    render_with_options(RecomposeOptions {
        force_recompose: true,
        ..Default::default()
    });

    assert_eq!(INVOCATIONS.with(|count| count.get()), 2);
}

#[test]
fn inactive_scopes_delay_invalidation_until_reactivated() {
    thread_local! {
        static CAPTURED_SCOPE: RefCell<Option<RecomposeScope>> = const { RefCell::new(None) };
        static INVOCATIONS: Cell<usize> = const { Cell::new(0) };
    }

    let mut composition = test_composition();
    let runtime = composition.runtime_handle();
    let state = MutableState::with_runtime(0, runtime.clone());
    let root_key = location_key(file!(), line!(), column!());

    #[composable]
    fn capture_scope(state: MutableState<i32>) {
        INVOCATIONS.with(|count| count.set(count.get() + 1));
        with_current_composer(|composer| {
            let scope = composer
                .current_recranpose_scope()
                .expect("scope available");
            CAPTURED_SCOPE.with(|slot| slot.replace(Some(scope)));
        });
        let _ = state.value();
    }

    composition
        .render(root_key, || capture_scope(state))
        .expect("initial composition");

    assert_eq!(INVOCATIONS.with(|count| count.get()), 1);

    let scope = CAPTURED_SCOPE
        .with(|slot| slot.borrow().clone())
        .expect("captured scope");
    assert!(scope.is_active());

    scope.deactivate();
    state.set_value(1);

    let _ = composition
        .process_invalid_scopes()
        .expect("no recomposition while inactive");

    assert_eq!(INVOCATIONS.with(|count| count.get()), 1);

    scope.reactivate();

    let _ = composition
        .process_invalid_scopes()
        .expect("recomposition after reactivation");

    assert_eq!(INVOCATIONS.with(|count| count.get()), 2);
}

#[test]
fn callbackless_scope_promotes_via_parent_scope_metadata() {
    thread_local! {
        static CHILD_SCOPE: RefCell<Option<RecomposeScope>> = const { RefCell::new(None) };
        static PARENT_SCOPE: RefCell<Option<RecomposeScope>> = const { RefCell::new(None) };
        static PARENT_SCOPE_ID: Cell<Option<ScopeId>> = const { Cell::new(None) };
        static PARENT_INVOCATIONS: Cell<usize> = const { Cell::new(0) };
        static OBSERVED_VALUES: RefCell<Vec<i32>> = const { RefCell::new(Vec::new()) };
    }

    let mut composition = test_composition();
    let runtime = composition.runtime_handle();
    let state = MutableState::with_runtime(0, runtime.clone());
    let root_key = location_key(file!(), line!(), column!());

    PARENT_INVOCATIONS.with(|count| count.set(0));
    CHILD_SCOPE.with(|slot| slot.borrow_mut().take());
    PARENT_SCOPE.with(|slot| slot.borrow_mut().take());
    PARENT_SCOPE_ID.with(|slot| slot.set(None));
    OBSERVED_VALUES.with(|values| values.borrow_mut().clear());

    #[composable]
    fn parent(state: MutableState<i32>) {
        PARENT_INVOCATIONS.with(|count| count.set(count.get() + 1));
        with_current_composer(|composer| {
            let scope = composer
                .current_recranpose_scope()
                .expect("parent scope available");
            PARENT_SCOPE.with(|slot| slot.replace(Some(scope.clone())));
            PARENT_SCOPE_ID.with(|slot| slot.set(Some(scope.id())));
        });

        cranpose_core::with_key(&"callbackless-child", || {
            with_current_composer(|composer| {
                let scope = composer
                    .current_recranpose_scope()
                    .expect("child scope available");
                assert!(
                    scope.group_anchor().is_valid(),
                    "callbackless scope should own a stable group anchor",
                );
                CHILD_SCOPE.with(|slot| slot.replace(Some(scope)));
            });
            OBSERVED_VALUES.with(|values| values.borrow_mut().push(state.value()));
        });
    }

    composition
        .render(root_key, || parent(state))
        .expect("initial composition");

    let child_scope = CHILD_SCOPE
        .with(|slot| slot.borrow().clone())
        .expect("captured callbackless child scope");
    let parent_scope = PARENT_SCOPE
        .with(|slot| slot.borrow().clone())
        .expect("captured parent scope");
    assert!(
        !child_scope.has_recompose_callback(),
        "with_key child should stay callbackless so promotion uses scope metadata",
    );
    assert!(
        parent_scope.has_recompose_callback(),
        "parent composable scope should own the recomposition callback",
    );
    assert_eq!(
        child_scope.parent_scope().map(|scope| scope.id()),
        PARENT_SCOPE_ID.with(|slot| slot.get()),
        "callbackless scope should point at its parent scope without slot-table scans",
    );
    assert_eq!(
        child_scope
            .callback_promotion_target()
            .map(|scope| scope.id()),
        Some(parent_scope.id()),
        "callbackless promotion should resolve directly to the callback-owning parent scope",
    );

    state.set_value(1);
    let recomposed = composition
        .process_invalid_scopes()
        .expect("process callbackless invalid scope");
    assert!(
        recomposed,
        "expected callbackless invalidation to recompose"
    );
    assert!(
        !composition.take_root_render_request(),
        "callbackless promotion should invalidate the parent scope instead of requesting a root render",
    );
    assert_eq!(PARENT_INVOCATIONS.with(|count| count.get()), 2);
    OBSERVED_VALUES.with(|values| {
        assert_eq!(values.borrow().as_slice(), &[0, 1]);
    });
}

#[test]
fn render_stable_reaches_fixpoint_when_internal_invalid_scope_processing_requests_root_render() {
    thread_local! {
        static ROOT_CALLBACKLESS_SCOPE: RefCell<Option<RecomposeScope>> = const { RefCell::new(None) };
        static INVALIDATED_DURING_SIDE_EFFECT: Cell<bool> = const { Cell::new(false) };
        static RENDER_COUNT: Cell<usize> = const { Cell::new(0) };
    }

    let mut composition = test_composition();
    let root_key = location_key(file!(), line!(), column!());

    ROOT_CALLBACKLESS_SCOPE.with(|slot| slot.borrow_mut().take());
    INVALIDATED_DURING_SIDE_EFFECT.with(|flag| flag.set(false));
    RENDER_COUNT.with(|count| count.set(0));

    let mut render = || {
        RENDER_COUNT.with(|count| count.set(count.get() + 1));
        cranpose_core::with_key(&"root-callbackless", || {
            with_current_composer(|composer| {
                let scope = composer
                    .current_recranpose_scope()
                    .expect("callbackless root scope available");
                ROOT_CALLBACKLESS_SCOPE.with(|slot| slot.replace(Some(scope)));
            });
        });

        cranpose_core::SideEffect(|| {
            let already_invalidated =
                INVALIDATED_DURING_SIDE_EFFECT.with(|flag| flag.replace(true));
            if already_invalidated {
                return;
            }

            ROOT_CALLBACKLESS_SCOPE.with(|slot| {
                let scope = slot
                    .borrow()
                    .clone()
                    .expect("captured root callbackless scope");
                assert!(
                    !scope.has_recompose_callback(),
                    "root-level with_key scope should stay callbackless",
                );
                assert!(
                    scope.callback_promotion_target().is_none(),
                    "root-level callbackless scope should request a root render",
                );
                scope.invalidate();
            });
        });
    };

    composition
        .render_stable(root_key, &mut render)
        .expect("initial render with side effect invalidation");

    assert!(
        !composition.take_root_render_request(),
        "render_stable() must drain root render requests raised during its internal invalid-scope pass",
    );
    assert_eq!(
        RENDER_COUNT.with(|count| count.get()),
        2,
        "render_stable() must replay the root content until the composition reaches a stable fixpoint",
    );
}

#[test]
fn reconcile_clears_suppressed_scope_flags_during_root_replay() {
    thread_local! {
        static PARENT_SCOPE: RefCell<Option<RecomposeScope>> = const { RefCell::new(None) };
        static CALLBACKLESS_SCOPE: RefCell<Option<RecomposeScope>> = const { RefCell::new(None) };
        static TRACKED_SCOPE: RefCell<Option<RecomposeScope>> = const { RefCell::new(None) };
        static INVALIDATE_TRACKED_DURING_PARENT: Cell<bool> = const { Cell::new(false) };
        static TRACKED_RENDERS: Cell<usize> = const { Cell::new(0) };
    }

    let mut composition = test_composition();
    let root_key = location_key(file!(), line!(), column!());

    PARENT_SCOPE.with(|slot| slot.borrow_mut().take());
    CALLBACKLESS_SCOPE.with(|slot| slot.borrow_mut().take());
    TRACKED_SCOPE.with(|slot| slot.borrow_mut().take());
    INVALIDATE_TRACKED_DURING_PARENT.with(|flag| flag.set(false));
    TRACKED_RENDERS.with(|count| count.set(0));

    #[composable]
    fn tracked_scope_leaf() {
        TRACKED_RENDERS.with(|count| count.set(count.get() + 1));
        with_current_composer(|composer| {
            let scope = composer
                .current_recranpose_scope()
                .expect("tracked scope available");
            TRACKED_SCOPE.with(|slot| slot.replace(Some(scope)));
        });
    }

    #[composable]
    fn parent_scope_host() {
        with_current_composer(|composer| {
            let scope = composer
                .current_recranpose_scope()
                .expect("parent scope available");
            PARENT_SCOPE.with(|slot| slot.replace(Some(scope)));
        });

        if INVALIDATE_TRACKED_DURING_PARENT.with(|flag| flag.replace(false)) {
            TRACKED_SCOPE.with(|slot| {
                slot.borrow()
                    .clone()
                    .expect("tracked scope captured")
                    .invalidate();
            });
        }

        tracked_scope_leaf();
    }

    let mut render = || {
        parent_scope_host();
        cranpose_core::with_key(&"root-callbackless", || {
            with_current_composer(|composer| {
                let scope = composer
                    .current_recranpose_scope()
                    .expect("callbackless root scope available");
                CALLBACKLESS_SCOPE.with(|slot| slot.replace(Some(scope)));
            });
        });
    };

    composition
        .render_stable(root_key, &mut render)
        .expect("initial stable render");

    let parent_scope = PARENT_SCOPE
        .with(|slot| slot.borrow().clone())
        .expect("captured parent scope");
    let callbackless_scope = CALLBACKLESS_SCOPE
        .with(|slot| slot.borrow().clone())
        .expect("captured callbackless scope");
    let tracked_scope = TRACKED_SCOPE
        .with(|slot| slot.borrow().clone())
        .expect("captured tracked scope");

    INVALIDATE_TRACKED_DURING_PARENT.with(|flag| flag.set(true));
    parent_scope.invalidate();
    callbackless_scope.invalidate();

    let changed = composition
        .reconcile(root_key, &mut render)
        .expect("reconcile mixed invalid scopes");
    assert!(changed, "expected reconcile to perform work");
    assert!(
        !tracked_scope.is_invalid(),
        "root replay must fully clear suppressed callbackful scopes so they can be invalidated again",
    );

    tracked_scope.invalidate();
    let recomposed = composition
        .process_invalid_scopes()
        .expect("process tracked scope after replay");
    assert!(recomposed, "tracked scope should recompose after replay");
    assert_eq!(
        TRACKED_RENDERS.with(|count| count.get()),
        3,
        "tracked scope should render initially, during reconcile, and once more after explicit reinvalidation",
    );
}

#[test]
fn process_invalid_scopes_preserves_later_fresh_subtree_when_earlier_scope_runs_after_it() {
    thread_local! {
        static EARLY_SCOPE: RefCell<Option<RecomposeScope>> = const { RefCell::new(None) };
        static LATE_STATE: RefCell<Option<MutableState<i32>>> = const { RefCell::new(None) };
        static EARLY_INVOCATIONS: Cell<usize> = const { Cell::new(0) };
        static LATE_INVOCATIONS: Cell<usize> = const { Cell::new(0) };
    }

    let mut composition = test_composition();
    let runtime = composition.runtime_handle();
    let show_late = MutableState::with_runtime(false, runtime.clone());
    let root_key = location_key(file!(), line!(), column!());

    #[composable]
    fn earlier_sibling() {
        EARLY_INVOCATIONS.with(|count| count.set(count.get() + 1));
        with_current_composer(|composer| {
            let scope = composer
                .current_recranpose_scope()
                .expect("scope available");
            EARLY_SCOPE.with(|slot| slot.replace(Some(scope)));
        });
    }

    #[composable]
    fn later_sibling(show_late: MutableState<bool>) {
        LATE_INVOCATIONS.with(|count| count.set(count.get() + 1));
        if show_late.value() {
            let state = useState(|| 0i32);
            LATE_STATE.with(|slot| {
                *slot.borrow_mut() = Some(state);
            });
        } else {
            LATE_STATE.with(|slot| {
                slot.borrow_mut().take();
            });
        }
    }

    #[composable]
    fn root(show_late: MutableState<bool>) {
        earlier_sibling();
        later_sibling(show_late);
    }

    composition
        .render(root_key, || root(show_late))
        .expect("initial composition");

    assert_eq!(EARLY_INVOCATIONS.with(|count| count.get()), 1);
    assert_eq!(LATE_INVOCATIONS.with(|count| count.get()), 1);
    assert!(
        LATE_STATE.with(|slot| slot.borrow().is_none()),
        "late branch should start hidden",
    );

    let earlier_scope = EARLY_SCOPE
        .with(|slot| slot.borrow().clone())
        .expect("captured earlier scope");

    show_late.set_value(true);
    earlier_scope.invalidate();

    while composition
        .process_invalid_scopes()
        .expect("process invalid scopes")
    {}

    assert_eq!(
        EARLY_INVOCATIONS.with(|count| count.get()),
        2,
        "earlier scope should have recomposed after explicit invalidation",
    );
    assert_eq!(
        LATE_INVOCATIONS.with(|count| count.get()),
        2,
        "later scope should have recomposed when its branch became visible",
    );

    let late_state = LATE_STATE
        .with(|slot| slot.borrow().as_ref().copied())
        .expect("late state registered after branch became visible");
    assert!(
        late_state.is_alive(),
        "later fresh subtree state must survive unrelated earlier-scope recomposition",
    );
    assert_eq!(late_state.get(), 0);

    late_state.set_value(1);
    while composition
        .process_invalid_scopes()
        .expect("recompose after late state update")
    {}

    let live_late_state = LATE_STATE
        .with(|slot| slot.borrow().as_ref().copied())
        .expect("late state still registered after update");
    assert!(live_late_state.is_alive());
    assert_eq!(live_late_state.get(), 1);
}

#[test]
fn gapped_scope_kept_alive_externally_stays_inactive_until_restored() {
    thread_local! {
        static CAPTURED_SCOPE: RefCell<Option<RecomposeScope>> = const { RefCell::new(None) };
        static INVOCATIONS: Cell<usize> = const { Cell::new(0) };
    }

    let mut composition = test_composition();
    let runtime = composition.runtime_handle();
    let show_branch = MutableState::with_runtime(true, runtime.clone());
    let observed = MutableState::with_runtime(0, runtime.clone());
    let root_key = location_key(file!(), line!(), column!());

    #[composable]
    fn conditional_branch(observed: MutableState<i32>) {
        INVOCATIONS.with(|count| count.set(count.get() + 1));
        with_current_composer(|composer| {
            let scope = composer
                .current_recranpose_scope()
                .expect("scope available");
            CAPTURED_SCOPE.with(|slot| slot.replace(Some(scope)));
        });
        let _ = observed.value();
    }

    #[composable]
    fn root(show_branch: MutableState<bool>, observed: MutableState<i32>) {
        if show_branch.value() {
            conditional_branch(observed);
        }
    }

    composition
        .render(root_key, || root(show_branch, observed))
        .expect("initial composition");

    assert_eq!(INVOCATIONS.with(|count| count.get()), 1);

    let scope = CAPTURED_SCOPE
        .with(|slot| slot.borrow().clone())
        .expect("captured scope");
    assert!(
        scope.is_active(),
        "visible branch scope should start active"
    );

    show_branch.set_value(false);
    let _ = composition
        .process_invalid_scopes()
        .expect("hide branch recomposition");

    assert!(
        !scope.is_active(),
        "scope retained outside the slot table must deactivate once its group is gapped; scope_id={} groups={:?} slots={:?}",
        scope.id(),
        composition.debug_dump_slot_table_groups(),
        composition.debug_dump_all_slots(),
    );

    observed.set_value(1);
    let _ = composition
        .process_invalid_scopes()
        .expect("hidden scope invalidation should be ignored");

    assert_eq!(
        INVOCATIONS.with(|count| count.get()),
        1,
        "a hidden gapped scope must not recompose just because an external clone kept it alive"
    );
}

#[test]
fn skipped_group_reparents_root_nodes_when_moved_to_a_new_parent() {
    thread_local! {
        static ROOT_ID: Cell<Option<NodeId>> = const { Cell::new(None) };
        static PARENT_ID: Cell<Option<NodeId>> = const { Cell::new(None) };
        static CHILD_ID: Cell<Option<NodeId>> = const { Cell::new(None) };
    }

    #[composable]
    fn movable_child() -> NodeId {
        let id = cranpose_core::with_current_composer(|composer| {
            composer.emit_node(TrackingChild::default)
        });
        CHILD_ID.with(|slot| slot.set(Some(id)));
        id
    }

    #[composable]
    fn keyed_movable_child() -> NodeId {
        let node_id = Cell::new(None);
        cranpose_core::with_key(&"movable-child", || {
            node_id.set(Some(movable_child()));
        });
        node_id
            .get()
            .expect("keyed movable child should emit a node")
    }

    #[composable]
    fn movable_parent(show_child_inside: bool) -> NodeId {
        let id = cranpose_core::with_current_composer(|composer| {
            composer.emit_node(RecordingNode::default)
        });
        PARENT_ID.with(|slot| slot.set(Some(id)));
        cranpose_core::push_parent(id);
        if show_child_inside {
            keyed_movable_child();
        }
        cranpose_core::pop_parent();
        id
    }

    #[composable]
    fn movable_root(show_child_inside: MutableState<bool>) -> NodeId {
        let show_child_inside = show_child_inside.value();
        let id = cranpose_core::with_current_composer(|composer| {
            composer.emit_node(RecordingNode::default)
        });
        ROOT_ID.with(|slot| slot.set(Some(id)));
        cranpose_core::push_parent(id);
        movable_parent(show_child_inside);
        if !show_child_inside {
            keyed_movable_child();
        }
        cranpose_core::pop_parent();
        id
    }

    let mut composition = test_composition();
    let runtime = composition.runtime_handle();
    let show_child_inside = MutableState::with_runtime(true, runtime);
    let root_key = location_key(file!(), line!(), column!());

    composition
        .render(root_key, || {
            movable_root(show_child_inside);
        })
        .expect("initial render");

    let root_id = ROOT_ID.with(|slot| slot.get()).expect("root id");
    let parent_id = PARENT_ID.with(|slot| slot.get()).expect("parent id");
    let child_id = CHILD_ID.with(|slot| slot.get()).expect("child id");

    {
        let mut applier = composition.applier_mut();
        let child_parent = applier
            .with_node::<TrackingChild, _>(child_id, |node| node.parent())
            .expect("child should exist");
        let parent_children = applier
            .with_node::<RecordingNode, _>(parent_id, |node| node.children.clone())
            .expect("parent should exist");
        assert_eq!(child_parent, Some(parent_id));
        assert_eq!(parent_children, vec![child_id]);
    }

    show_child_inside.set(false);
    while composition
        .process_invalid_scopes()
        .expect("recompose after moving child")
    {}

    let current_child_id = CHILD_ID.with(|slot| slot.get()).expect("current child id");
    let mut applier = composition.applier_mut();
    let tree = applier.dump_tree(Some(root_id));
    assert_eq!(
        current_child_id, child_id,
        "moving a skipped child between parents must reuse the same node; tree=\n{tree}",
    );
    let child_parent = applier
        .with_node::<TrackingChild, _>(child_id, |node| node.parent())
        .expect("child should still exist after move");
    let root_children = applier
        .with_node::<RecordingNode, _>(root_id, |node| node.children.clone())
        .expect("root should exist");
    let parent_children = applier
        .with_node::<RecordingNode, _>(parent_id, |node| node.children.clone())
        .expect("parent should exist");

    assert_eq!(
        child_parent,
        Some(root_id),
        "a skipped moved group must reattach its root node to the new parent; tree=\n{tree}",
    );
    assert_eq!(
        root_children,
        vec![parent_id, child_id],
        "root should now own the moved child directly; tree=\n{tree}",
    );
    assert!(
        parent_children.is_empty(),
        "old parent must release the moved child when the skipped group changes parents; tree=\n{tree}",
    );
}
