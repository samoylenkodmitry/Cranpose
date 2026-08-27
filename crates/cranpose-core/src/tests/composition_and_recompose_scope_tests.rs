use super::*;

#[test]
fn failed_apply_abandons_root_slot_state() {
    let mut composition = test_composition();
    let root_key = location_key(file!(), line!(), column!());
    let child_key = location_key(file!(), line!(), column!());

    let result = composition.render(root_key, || {
        with_current_composer(|composer| {
            composer.with_group(child_key, |composer| {
                let _remembered = composer.remember(|| 17_i32);
                composer.commands_mut().push(Command::callback(|_| {
                    Err(NodeError::MissingContext {
                        id: 77,
                        reason: "forced apply failure",
                    })
                }));
            });
        });
    });

    assert_eq!(
        result,
        Err(NodeError::MissingContext {
            id: 77,
            reason: "forced apply failure",
        })
    );
    let snapshot = composition.debug_slot_snapshot();
    assert!(
        snapshot.active_groups.is_empty(),
        "failed apply must not leave committed active slot groups: {snapshot:#?}",
    );
    assert_eq!(snapshot.retained_subtree_count, 0);
    assert!(composition.root().is_none());
    assert!(composition.take_root_render_request());
}

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
fn malformed_composition_local_entry_falls_back_to_default() {
    thread_local! {
        static READ_VALUE: Cell<i32> = const { Cell::new(0) };
    }

    let local_counter = compositionLocalOf(|| 31);
    let mut composition = test_composition();

    #[composable]
    fn reader(local_counter: CompositionLocal<i32>) {
        READ_VALUE.with(|slot| slot.set(local_counter.current()));
    }

    composition
        .render(2_031, || {
            let malformed = crate::composition_locals::malformed_composition_local_for_test(
                &local_counter,
                Rc::new(String::from("wrong")) as Rc<dyn std::any::Any>,
            );
            CompositionLocalProvider(vec![malformed], || reader(local_counter.clone()));
        })
        .expect("compose malformed local provider");

    assert_eq!(READ_VALUE.with(|slot| slot.get()), 31);
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
fn malformed_static_composition_local_entry_falls_back_to_default() {
    thread_local! {
        static READ_VALUE: Cell<i32> = const { Cell::new(0) };
    }

    let local_counter = staticCompositionLocalOf(|| 37);
    let mut composition = test_composition();

    #[composable]
    fn reader(local_counter: StaticCompositionLocal<i32>) {
        READ_VALUE.with(|slot| slot.set(local_counter.current()));
    }

    composition
        .render(2_037, || {
            let malformed = crate::composition_locals::malformed_static_composition_local_for_test(
                &local_counter,
                Rc::new(String::from("wrong")) as Rc<dyn std::any::Any>,
            );
            CompositionLocalProvider(vec![malformed], || reader(local_counter.clone()));
        })
        .expect("compose malformed static local provider");

    assert_eq!(READ_VALUE.with(|slot| slot.get()), 37);
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
                        let scope = composer.current_recompose_scope().expect("scope available");
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
                        let scope = composer.current_recompose_scope().expect("scope available");
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
            let scope = composer.current_recompose_scope().expect("scope available");
            CAPTURED_SCOPE.with(|slot| slot.replace(Some(scope)));
        });
        let _ = state.value();
    }

    composition
        .render(root_key, || capture_scope(state))
        .expect("initial composition");
    assert_composition_valid(&composition);

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
    assert_composition_valid(&composition);

    assert_eq!(INVOCATIONS.with(|count| count.get()), 1);

    scope.reactivate();

    let _ = composition
        .process_invalid_scopes()
        .expect("recomposition after reactivation");
    assert_composition_valid(&composition);

    assert_eq!(INVOCATIONS.with(|count| count.get()), 2);
}

#[test]
fn scope_deactivated_during_invalidation_batch_waits_for_reactivation() {
    thread_local! {
        static CHILD_SCOPE: RefCell<Option<RecomposeScope>> = const { RefCell::new(None) };
        static DEACTIVATE_CHILD: Cell<bool> = const { Cell::new(false) };
        static CHILD_INVOCATIONS: Cell<usize> = const { Cell::new(0) };
    }

    let mut composition = test_composition();
    let runtime = composition.runtime_handle();
    let parent_trigger = MutableState::with_runtime(0, runtime.clone());
    let child_trigger = MutableState::with_runtime(0, runtime);
    let root_key = location_key(file!(), line!(), column!());

    CHILD_SCOPE.with(|slot| slot.borrow_mut().take());
    DEACTIVATE_CHILD.with(|flag| flag.set(false));
    CHILD_INVOCATIONS.with(|count| count.set(0));

    #[composable]
    fn earlier_sibling(parent_trigger: MutableState<i32>) {
        let _ = parent_trigger.value();
        if DEACTIVATE_CHILD.with(|flag| flag.replace(false)) {
            CHILD_SCOPE.with(|slot| {
                slot.borrow()
                    .as_ref()
                    .expect("child scope captured")
                    .deactivate();
            });
        }
    }

    #[composable]
    fn later_sibling(child_trigger: MutableState<i32>) {
        let _ = child_trigger.value();
        CHILD_INVOCATIONS.with(|count| count.set(count.get() + 1));
        with_current_composer(|composer| {
            CHILD_SCOPE.with(|slot| {
                slot.replace(Some(
                    composer
                        .current_recompose_scope()
                        .expect("child scope available"),
                ));
            });
        });
    }

    composition
        .render(root_key, || {
            earlier_sibling(parent_trigger);
            later_sibling(child_trigger);
        })
        .expect("initial composition");
    assert_composition_valid(&composition);
    assert_eq!(CHILD_INVOCATIONS.with(Cell::get), 1);

    DEACTIVATE_CHILD.with(|flag| flag.set(true));
    parent_trigger.set_value(1);
    child_trigger.set_value(1);

    composition
        .process_invalid_scopes()
        .expect("process batch containing deactivated child");
    assert_composition_valid(&composition);
    assert_eq!(
        CHILD_INVOCATIONS.with(Cell::get),
        1,
        "a scope deactivated by an earlier callback in the batch must not run",
    );

    let child_scope = CHILD_SCOPE
        .with(|slot| slot.borrow().clone())
        .expect("child scope retained for reactivation");
    assert!(child_scope.is_invalid());
    child_scope.reactivate();

    composition
        .process_invalid_scopes()
        .expect("process child after reactivation");
    assert_composition_valid(&composition);
    assert_eq!(CHILD_INVOCATIONS.with(Cell::get), 2);
}

#[test]
fn scope_beneath_inactive_ancestor_waits_for_tree_reactivation() {
    thread_local! {
        static PARENT_SCOPE: RefCell<Option<RecomposeScope>> = const { RefCell::new(None) };
        static CHILD_SCOPE: RefCell<Option<RecomposeScope>> = const { RefCell::new(None) };
        static CHILD_INVOCATIONS: Cell<usize> = const { Cell::new(0) };
    }

    let mut composition = test_composition();
    let runtime = composition.runtime_handle();
    let child_trigger = MutableState::with_runtime(0, runtime);
    let root_key = location_key(file!(), line!(), column!());

    PARENT_SCOPE.with(|slot| slot.borrow_mut().take());
    CHILD_SCOPE.with(|slot| slot.borrow_mut().take());
    CHILD_INVOCATIONS.with(|count| count.set(0));

    #[composable]
    fn child(child_trigger: MutableState<i32>) {
        let _ = child_trigger.value();
        CHILD_INVOCATIONS.with(|count| count.set(count.get() + 1));
        with_current_composer(|composer| {
            CHILD_SCOPE.with(|slot| {
                slot.replace(Some(
                    composer
                        .current_recompose_scope()
                        .expect("child scope available"),
                ));
            });
        });
    }

    #[composable]
    fn parent(child_trigger: MutableState<i32>) {
        with_current_composer(|composer| {
            PARENT_SCOPE.with(|slot| {
                slot.replace(Some(
                    composer
                        .current_recompose_scope()
                        .expect("parent scope available"),
                ));
            });
        });
        child(child_trigger);
    }

    composition
        .render(root_key, || parent(child_trigger))
        .expect("initial composition");
    assert_composition_valid(&composition);
    assert_eq!(CHILD_INVOCATIONS.with(Cell::get), 1);

    let parent_scope = PARENT_SCOPE
        .with(|slot| slot.borrow().clone())
        .expect("parent scope captured");
    let child_scope = CHILD_SCOPE
        .with(|slot| slot.borrow().clone())
        .expect("child scope captured");
    assert!(parent_scope.is_active());
    assert!(child_scope.is_active());

    child_trigger.set_value(1);
    parent_scope.deactivate();
    composition
        .process_invalid_scopes()
        .expect("defer child beneath inactive ancestor");
    assert_composition_valid(&composition);
    assert_eq!(
        CHILD_INVOCATIONS.with(Cell::get),
        1,
        "a locally active scope cannot run beneath an inactive ancestor",
    );
    assert!(child_scope.is_invalid());

    parent_scope.reactivate();
    child_scope.reactivate();
    composition
        .process_invalid_scopes()
        .expect("recompose child after tree reactivation");
    assert_composition_valid(&composition);
    assert_eq!(CHILD_INVOCATIONS.with(Cell::get), 2);
}

#[test]
fn subcomposition_scope_inherits_source_owner_lifetime() {
    thread_local! {
        static SECONDARY_HOST: RefCell<Option<Rc<SlotsHost>>> = const { RefCell::new(None) };
        static OWNER_STATE: RefCell<Option<MutableState<i32>>> = const { RefCell::new(None) };
        static OWNER_SCOPE: RefCell<Option<RecomposeScope>> = const { RefCell::new(None) };
        static SECONDARY_ROOT_SCOPE: RefCell<Option<RecomposeScope>> = const { RefCell::new(None) };
        static CHILD_SCOPE: RefCell<Option<RecomposeScope>> = const { RefCell::new(None) };
        static CHILD_READS: Cell<usize> = const { Cell::new(0) };
    }

    let mut composition = test_composition();
    let root_key = location_key(file!(), line!(), column!());

    SECONDARY_HOST.with(|slot| {
        slot.replace(Some(Rc::new(SlotsHost::new(SlotTable::new()))));
    });
    OWNER_STATE.with(|slot| slot.borrow_mut().take());
    OWNER_SCOPE.with(|slot| slot.borrow_mut().take());
    SECONDARY_ROOT_SCOPE.with(|slot| slot.borrow_mut().take());
    CHILD_SCOPE.with(|slot| slot.borrow_mut().take());
    CHILD_READS.with(|count| count.set(0));

    #[composable]
    fn secondary_content(state: MutableState<i32>) {
        let _ = state.value();
        CHILD_READS.with(|count| count.set(count.get() + 1));
        with_current_composer(|composer| {
            CHILD_SCOPE.with(|slot| {
                slot.replace(Some(
                    composer
                        .current_recompose_scope()
                        .expect("secondary child scope available"),
                ));
            });
        });
    }

    #[composable]
    fn owner() {
        let state = rememberMutableStateOf(|| 0_i32);
        OWNER_STATE.with(|slot| slot.replace(Some(state)));
        with_current_composer(|composer| {
            OWNER_SCOPE.with(|slot| {
                slot.replace(Some(
                    composer
                        .current_recompose_scope()
                        .expect("source owner scope available"),
                ));
            });
        });
        let context = with_current_composer(Composer::capture_composition_context);
        let host = SECONDARY_HOST.with(|slot| {
            slot.borrow()
                .as_ref()
                .cloned()
                .expect("secondary host installed")
        });
        with_current_composer(|composer| {
            composer
                .subcompose_slot_with_context(&host, None, &context, |secondary_composer| {
                    SECONDARY_ROOT_SCOPE.with(|slot| {
                        slot.replace(Some(
                            secondary_composer
                                .current_recompose_scope()
                                .expect("secondary root scope available"),
                        ));
                    });
                    secondary_content(state);
                })
                .expect("subcompose owner content");
        });
    }

    #[composable]
    fn root(show_owner: bool) {
        if show_owner {
            owner();
        }
    }

    composition
        .render(root_key, || root(true))
        .expect("initial owner composition");
    assert_composition_valid(&composition);
    assert_eq!(CHILD_READS.with(Cell::get), 1);

    let owner_scope = OWNER_SCOPE
        .with(|slot| slot.borrow().clone())
        .expect("source owner scope captured");
    let secondary_root_scope = SECONDARY_ROOT_SCOPE
        .with(|slot| slot.borrow().clone())
        .expect("secondary root scope captured");
    assert!(
        secondary_root_scope.parent_scope().is_none(),
        "cross-host lifetime ownership must not become structural recomposition ancestry",
    );
    assert_eq!(
        secondary_root_scope
            .lifetime_owner_scope()
            .map(|scope| scope.id()),
        Some(owner_scope.id()),
        "secondary root must remain weakly bound to its source owner's lifetime",
    );
    assert!(
        secondary_root_scope.callback_promotion_target().is_none(),
        "callback promotion must not cross slot-host boundaries",
    );

    let owner_state = OWNER_STATE
        .with(|slot| slot.borrow().as_ref().copied())
        .expect("owner state captured");
    let child_scope = CHILD_SCOPE
        .with(|slot| slot.borrow().clone())
        .expect("secondary child scope captured");
    owner_state.set_value(1);
    assert!(child_scope.is_invalid());

    composition
        .render(root_key, || root(false))
        .expect("remove state owner");
    assert_composition_valid(&composition);
    assert!(
        !owner_state.is_alive(),
        "removing the source group must release its owned state",
    );

    composition
        .process_invalid_scopes()
        .expect("defer invalid secondary child after source removal");
    assert_composition_valid(&composition);
    assert_eq!(
        CHILD_READS.with(Cell::get),
        1,
        "secondary content cannot execute after its source owner is removed",
    );
    assert!(child_scope.is_invalid());
    assert!(!child_scope.is_effectively_active());

    SECONDARY_HOST.with(|slot| slot.borrow_mut().take());
    OWNER_STATE.with(|slot| slot.borrow_mut().take());
    OWNER_SCOPE.with(|slot| slot.borrow_mut().take());
    SECONDARY_ROOT_SCOPE.with(|slot| slot.borrow_mut().take());
    CHILD_SCOPE.with(|slot| slot.borrow_mut().take());
}

#[test]
fn invalidating_active_scope_recomposes_that_scope() {
    thread_local! {
        static CAPTURED_SCOPE: RefCell<Option<RecomposeScope>> = const { RefCell::new(None) };
        static INVOCATIONS: Cell<usize> = const { Cell::new(0) };
    }

    let mut composition = test_composition();
    let root_key = location_key(file!(), line!(), column!());

    #[composable]
    fn capture_scope() {
        INVOCATIONS.with(|count| count.set(count.get() + 1));
        with_current_composer(|composer| {
            let scope = composer.current_recompose_scope().expect("scope available");
            CAPTURED_SCOPE.with(|slot| slot.replace(Some(scope)));
        });
    }

    composition
        .render(root_key, capture_scope)
        .expect("initial composition");
    assert_composition_valid(&composition);
    assert_eq!(INVOCATIONS.with(|count| count.get()), 1);

    let scope = CAPTURED_SCOPE
        .with(|slot| slot.borrow().clone())
        .expect("captured active scope");
    assert!(scope.is_active());

    scope.invalidate();
    let recomposed = composition
        .process_invalid_scopes()
        .expect("recompose after explicit invalidation");
    assert_composition_valid(&composition);
    assert!(
        recomposed,
        "active scope invalidation must trigger recomposition"
    );
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
                .current_recompose_scope()
                .expect("parent scope available");
            PARENT_SCOPE.with(|slot| slot.replace(Some(scope.clone())));
            PARENT_SCOPE_ID.with(|slot| slot.set(Some(scope.id())));
        });

        cranpose_core::with_key(&"callbackless-child", || {
            with_current_composer(|composer| {
                let scope = composer
                    .current_recompose_scope()
                    .expect("child scope available");
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
    let initial_snapshot = composition.debug_slot_snapshot();
    assert!(
        initial_snapshot
            .scopes
            .iter()
            .any(|scope| scope.scope_id == child_scope.id()),
        "callbackless scope should be indexed by the slot table",
    );
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
                    .current_recompose_scope()
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
fn render_stable_reports_root_render_replay_limit_without_panicking() {
    let mut composition = test_composition();
    let root_key = location_key(file!(), line!(), column!());

    let mut render = || {
        let value = rememberMutableStateOf(|| 0usize);
        let current = value.value();
        SideEffect(move || {
            value.set_value(current + 1);
        });
    };

    let result = composition.render_stable(root_key, &mut render);

    assert_eq!(
        result,
        Err(NodeError::RecompositionLimitExceeded {
            operation: "root render replay",
            limit: ROOT_RENDER_REPLAY_LIMIT,
        })
    );
}

#[test]
fn reconcile_clears_scope_flags_during_root_replay() {
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
                .current_recompose_scope()
                .expect("tracked scope available");
            TRACKED_SCOPE.with(|slot| slot.replace(Some(scope)));
        });
    }

    #[composable]
    fn parent_scope_host() {
        with_current_composer(|composer| {
            let scope = composer
                .current_recompose_scope()
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
                    .current_recompose_scope()
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
        "root replay must fully clear callbackful scopes so they can be invalidated again",
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
            let scope = composer.current_recompose_scope().expect("scope available");
            EARLY_SCOPE.with(|slot| slot.replace(Some(scope)));
        });
    }

    #[composable]
    fn later_sibling(show_late: MutableState<bool>) {
        LATE_INVOCATIONS.with(|count| count.set(count.get() + 1));
        if show_late.value() {
            let state = rememberMutableStateOf(|| 0i32);
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
fn retained_scope_stays_inactive_until_restored() {
    thread_local! {
        static CAPTURED_SCOPE: RefCell<Option<RecomposeScope>> = const { RefCell::new(None) };
        static INVOCATIONS: Cell<usize> = const { Cell::new(0) };
        static OBSERVED_VALUES: RefCell<Vec<i32>> = const { RefCell::new(Vec::new()) };
    }

    let mut composition = test_composition();
    let runtime = composition.runtime_handle();
    let show_branch = MutableState::with_runtime(true, runtime.clone());
    let observed = MutableState::with_runtime(0, runtime.clone());
    let root_key = location_key(file!(), line!(), column!());
    OBSERVED_VALUES.with(|values| values.borrow_mut().clear());

    #[composable]
    fn root(show_branch: MutableState<bool>, observed: MutableState<i32>) {
        if show_branch.value() {
            let branch_key = location_key(file!(), line!(), column!());
            with_current_composer(|composer| {
                composer.cranpose_with_reuse(branch_key, RecomposeOptions::default(), |composer| {
                    INVOCATIONS.with(|count| count.set(count.get() + 1));
                    let scope = composer.current_recompose_scope().expect("scope available");
                    CAPTURED_SCOPE.with(|slot| slot.replace(Some(scope)));
                    OBSERVED_VALUES.with(|values| values.borrow_mut().push(observed.value()));
                });
            });
        }
    }

    let pass = |composition: &mut Composition<MemoryApplier>| {
        composition.render(root_key, || root(show_branch, observed))
    };
    pass(&mut composition).expect("initial composition");
    assert_composition_valid(&composition);

    let initial_snapshot = composition.debug_slot_snapshot();
    let initial_stats = composition.debug_slot_table_stats();
    assert_eq!(initial_snapshot.retained_subtree_count, 0);
    assert_eq!(initial_snapshot.retained_scope_count, 0);
    assert_eq!(initial_snapshot.retained_payload_count, 0);
    assert_eq!(initial_stats.retained_subtree_count, 0);
    assert_eq!(initial_stats.retained_group_count, 0);
    assert_eq!(initial_stats.retained_payload_count, 0);
    assert_eq!(initial_stats.retained_node_count, 0);
    assert_eq!(initial_stats.retained_scope_count, 0);
    assert_eq!(initial_stats.retained_anchor_count, 0);
    assert_eq!(initial_stats.retained_heap_bytes, 0);
    assert_eq!(initial_stats.retained_evictions_total, 0);
    assert_eq!(initial_stats.detached_anchor_count, 0);
    assert!(
        initial_snapshot.active_scope_count >= 1,
        "initial render should expose the active retained-branch scope in the slot snapshot"
    );

    assert_eq!(INVOCATIONS.with(|count| count.get()), 1);
    OBSERVED_VALUES.with(|values| {
        assert_eq!(values.borrow().as_slice(), &[0]);
    });

    let scope = CAPTURED_SCOPE
        .with(|slot| slot.borrow().clone())
        .expect("captured scope");
    assert!(
        scope.is_active(),
        "visible branch scope should start active"
    );

    show_branch.set_value(false);
    pass(&mut composition).expect("hide branch render");
    assert_composition_valid(&composition);

    let hidden_snapshot = composition.debug_slot_snapshot();
    let hidden_stats = composition.debug_slot_table_stats();
    assert_eq!(hidden_snapshot.retained_subtree_count, 1);
    assert!(
        hidden_snapshot.retained_group_count >= 1,
        "hiding a retained branch must move at least one group into retained storage",
    );
    assert_eq!(hidden_snapshot.retained_scope_count, 1);
    assert_eq!(hidden_stats.retained_subtree_count, 1);
    assert!(
        hidden_stats.retained_group_count >= 1,
        "retained groups must appear in runtime slot diagnostics after hiding the branch",
    );
    assert_eq!(hidden_stats.retained_scope_count, 1);
    assert!(
        hidden_stats.retained_anchor_count >= 1,
        "retained subtree anchors must stay visible in diagnostics while detached",
    );
    assert!(
        hidden_stats.retained_heap_bytes > 0,
        "retained subtree slot storage must report non-zero heap once hidden",
    );
    assert_eq!(hidden_stats.retained_evictions_total, 0);
    assert!(
        hidden_stats.detached_anchor_count >= hidden_stats.retained_anchor_count,
        "retained anchors should be tracked as detached in the active anchor registry",
    );
    assert_eq!(
        hidden_stats.active_anchor_count
            + hidden_stats.detached_anchor_count
            + hidden_stats.invalidated_anchor_count,
        hidden_stats.anchor_slot_count,
        "anchor diagnostics must account for every occupied registry slot",
    );
    assert_eq!(
        hidden_stats.active_payload_anchor_count
            + hidden_stats.detached_payload_anchor_count
            + hidden_stats.invalidated_payload_anchor_count,
        hidden_stats.payload_anchor_slot_count,
        "payload anchor diagnostics must account for every occupied registry slot",
    );

    assert!(
        !scope.is_active(),
        "retained scope must deactivate when its subtree leaves the active table; scope_id={} groups={:?} slots={:?}",
        scope.id(),
        composition.debug_dump_slot_table_groups(),
        composition.debug_dump_slot_entries(),
    );

    observed.set_value(1);
    let _ = composition
        .process_invalid_scopes()
        .expect("hidden scope invalidation should be ignored");
    assert_composition_valid(&composition);

    assert_eq!(
        INVOCATIONS.with(|count| count.get()),
        1,
        "a hidden retained scope must not recompose just because an external clone kept it alive"
    );

    show_branch.set_value(true);
    pass(&mut composition).expect("restore retained branch");
    assert_composition_valid(&composition);

    let restored_snapshot = composition.debug_slot_snapshot();
    let restored_stats = composition.debug_slot_table_stats();
    assert_eq!(restored_snapshot.retained_subtree_count, 0);
    assert_eq!(restored_snapshot.retained_scope_count, 0);
    assert_eq!(restored_stats.retained_subtree_count, 0);
    assert_eq!(restored_stats.retained_group_count, 0);
    assert_eq!(restored_stats.retained_payload_count, 0);
    assert_eq!(restored_stats.retained_node_count, 0);
    assert_eq!(restored_stats.retained_scope_count, 0);
    assert_eq!(restored_stats.retained_anchor_count, 0);
    assert_eq!(restored_stats.retained_heap_bytes, 0);
    assert_eq!(restored_stats.retained_evictions_total, 0);
    assert_eq!(restored_stats.detached_anchor_count, 0);

    let restored_scope = CAPTURED_SCOPE
        .with(|slot| slot.borrow().clone())
        .expect("captured scope after restore");
    assert_eq!(
        restored_scope.id(),
        scope.id(),
        "restoring a retained subtree must reactivate the same scope",
    );
    assert!(
        restored_scope.is_active(),
        "restored scope should be active"
    );
    assert_eq!(
        INVOCATIONS.with(|count| count.get()),
        2,
        "restoring the retained subtree must recompose it once",
    );
    OBSERVED_VALUES.with(|values| {
        assert_eq!(
            values.borrow().as_slice(),
            &[0, 1],
            "invalidations that happen while retained must be observed when the subtree is restored",
        );
    });
}

#[test]
fn restored_retained_scope_processes_forced_recompose() {
    thread_local! {
        static CAPTURED_SCOPE: RefCell<Option<RecomposeScope>> = const { RefCell::new(None) };
        static INVOCATIONS: Cell<usize> = const { Cell::new(0) };
    }

    const BRANCH_KEY: Key = 0x5C0_9E1;

    let mut composition = test_composition();
    let runtime = composition.runtime_handle();
    let show_branch = MutableState::with_runtime(true, runtime.clone());
    let root_key = location_key(file!(), line!(), column!());
    CAPTURED_SCOPE.with(|slot| slot.replace(None));
    INVOCATIONS.with(|count| count.set(0));

    #[composable]
    fn root(show_branch: MutableState<bool>) {
        if show_branch.value() {
            with_current_composer(|composer| {
                composer.cranpose_with_reuse(BRANCH_KEY, RecomposeOptions::default(), |composer| {
                    INVOCATIONS.with(|count| count.set(count.get() + 1));
                    let scope = composer
                        .current_recompose_scope()
                        .expect("retained branch scope available");
                    CAPTURED_SCOPE.with(|slot| slot.replace(Some(scope)));
                });
            });
        }
    }

    let pass = |composition: &mut Composition<MemoryApplier>| {
        composition.render(root_key, || root(show_branch))
    };
    pass(&mut composition).expect("initial composition");
    assert_composition_valid(&composition);
    assert_eq!(INVOCATIONS.with(|count| count.get()), 1);

    let initial_scope = CAPTURED_SCOPE
        .with(|slot| slot.borrow().clone())
        .expect("initial retained scope");
    assert!(initial_scope.is_active());

    show_branch.set_value(false);
    pass(&mut composition).expect("hide retained branch");
    assert_composition_valid(&composition);
    assert!(!initial_scope.is_active());
    assert_eq!(composition.debug_slot_snapshot().retained_scope_count, 1);

    show_branch.set_value(true);
    pass(&mut composition).expect("restore retained branch");
    assert_composition_valid(&composition);

    let restored_scope = CAPTURED_SCOPE
        .with(|slot| slot.borrow().clone())
        .expect("restored retained scope");
    assert_eq!(restored_scope.id(), initial_scope.id());
    assert!(restored_scope.is_active());
    assert_eq!(composition.debug_slot_snapshot().retained_scope_count, 0);

    let invocations_after_restore = INVOCATIONS.with(|count| count.get());
    restored_scope.force_recompose();
    restored_scope.invalidate();
    let recomposed = composition
        .process_invalid_scopes()
        .expect("forced recomposition after restore");
    assert_composition_valid(&composition);

    assert!(
        recomposed,
        "restored retained scope must resolve through ScopeIndex for forced recomposition",
    );
    assert_eq!(
        INVOCATIONS.with(|count| count.get()),
        invocations_after_restore + 1,
        "forced recomposition must execute the restored retained scope",
    );
}

#[test]
fn changing_root_key_does_not_leak_root_scopes() {
    let mut composition = test_composition();

    composition.render(11, || {}).expect("initial root render");
    assert_eq!(
        composition
            .debug_slot_snapshot()
            .runtime_scope_registry_count
            .expect("composition snapshot should include runtime scope registry count"),
        1,
        "initial render should register exactly one root scope"
    );

    composition.render(22, || {}).expect("second root render");
    assert_eq!(
        composition
            .debug_slot_snapshot()
            .runtime_scope_registry_count
            .expect("composition snapshot should include runtime scope registry count"),
        1,
        "changing the root key must dispose the previous root scope instead of leaking it",
    );

    composition.render(33, || {}).expect("third root render");
    assert_eq!(
        composition
            .debug_slot_snapshot()
            .runtime_scope_registry_count
            .expect("composition snapshot should include runtime scope registry count"),
        1,
        "repeated root-key churn must keep the scope registry bounded",
    );
}

#[test]
fn remember_survives_normal_recomposition() {
    thread_local! {
        static INVOCATIONS: Cell<usize> = const { Cell::new(0) };
    }

    let mut composition = test_composition();
    let runtime = composition.runtime_handle();
    let observed = MutableState::with_runtime(0, runtime.clone());
    let root_key = location_key(file!(), line!(), column!());
    let group_key = location_key(file!(), line!(), column!());
    let remembered = Rc::new(RefCell::new(None::<Owned<i32>>));

    let mut render = || {
        let remembered = Rc::clone(&remembered);
        composition
            .render(root_key, || {
                let _ = observed.value();
                with_current_composer(|composer| {
                    composer.with_group(group_key, |composer| {
                        INVOCATIONS.with(|count| count.set(count.get() + 1));
                        let slot = composer.remember(|| 7_i32);
                        remembered.replace(Some(slot));
                    });
                });
            })
            .expect("render remembered branch");
    };

    render();
    let first = remembered
        .borrow()
        .clone()
        .expect("initial remembered slot");
    first.replace(41);

    observed.set_value(1);
    render();

    let restored = remembered
        .borrow()
        .clone()
        .expect("remembered slot after recomposition");
    assert_eq!(
        restored.with(|value| *value),
        41,
        "normal recomposition must preserve remembered state",
    );
    assert_eq!(INVOCATIONS.with(|count| count.get()), 2);
}

#[test]
fn conditional_branch_without_retention_resets_remembered_state() {
    let mut composition = test_composition();
    let runtime = composition.runtime_handle();
    let show_branch = MutableState::with_runtime(true, runtime.clone());
    let root_key = location_key(file!(), line!(), column!());
    let branch_key = location_key(file!(), line!(), column!());
    let remembered = Rc::new(RefCell::new(None::<Owned<i32>>));

    let mut render = || {
        let remembered = Rc::clone(&remembered);
        composition
            .render(root_key, || {
                remembered.replace(None);
                if show_branch.value() {
                    with_current_composer(|composer| {
                        composer.with_group(branch_key, |composer| {
                            let slot = composer.remember(|| 7_i32);
                            remembered.replace(Some(slot));
                        });
                    });
                }
            })
            .expect("render conditional branch");
    };

    render();
    let first = remembered
        .borrow()
        .clone()
        .expect("initial remembered state");
    first.replace(41);

    show_branch.set_value(false);
    render();
    assert!(
        remembered.borrow().is_none(),
        "hidden branches must not leave an active remembered slot behind"
    );

    show_branch.set_value(true);
    render();
    let restored = remembered
        .borrow()
        .clone()
        .expect("remembered state after restoring the branch");
    assert_eq!(
        restored.with(|value| *value),
        7,
        "without retention, removing a conditional branch must reset its remembered state",
    );
}

#[test]
fn retained_conditional_branch_restores_remembered_state() {
    let mut composition = test_composition();
    let runtime = composition.runtime_handle();
    let show_branch = MutableState::with_runtime(true, runtime.clone());
    let root_key = location_key(file!(), line!(), column!());
    let branch_key = location_key(file!(), line!(), column!());
    let remembered = Rc::new(RefCell::new(None::<Owned<i32>>));

    fn render(
        composition: &mut Composition<MemoryApplier>,
        show_branch: &MutableState<bool>,
        root_key: Key,
        branch_key: Key,
        remembered: &Rc<RefCell<Option<Owned<i32>>>>,
    ) {
        let remembered = Rc::clone(remembered);
        composition
            .render(root_key, || {
                remembered.replace(None);
                if show_branch.value() {
                    with_current_composer(|composer| {
                        composer.cranpose_with_reuse(
                            branch_key,
                            RecomposeOptions::default(),
                            |composer| {
                                let slot = composer.remember(|| 7_i32);
                                remembered.replace(Some(slot));
                            },
                        );
                    });
                }
            })
            .expect("render retained conditional branch");
        assert_composition_valid(composition);
    }

    render(
        &mut composition,
        &show_branch,
        root_key,
        branch_key,
        &remembered,
    );
    let first = remembered
        .borrow()
        .clone()
        .expect("initial retained remembered state");
    first.replace(41);
    assert_eq!(composition.debug_slot_snapshot().retained_subtree_count, 0);

    show_branch.set_value(false);
    render(
        &mut composition,
        &show_branch,
        root_key,
        branch_key,
        &remembered,
    );
    assert!(
        remembered.borrow().is_none(),
        "hidden retained branches must not expose an active remembered slot"
    );
    let hidden_snapshot = composition.debug_slot_snapshot();
    assert_eq!(hidden_snapshot.retained_subtree_count, 1);
    assert_eq!(hidden_snapshot.retained_payload_count, 1);
    assert_eq!(
        composition.debug_slot_table_stats().retained_payload_count,
        1
    );

    show_branch.set_value(true);
    render(
        &mut composition,
        &show_branch,
        root_key,
        branch_key,
        &remembered,
    );
    let restored = remembered
        .borrow()
        .clone()
        .expect("remembered state after retained restore");
    assert_eq!(
        restored.with(|value| *value),
        41,
        "retention must restore remembered state after the conditional branch returns",
    );
    assert_eq!(composition.debug_slot_snapshot().retained_subtree_count, 0);
}

#[test]
fn retained_branch_preserves_node_payload_and_scope_lifecycle_until_restore() {
    const BRANCH_KEY: Key = 0xCA1;

    let mut composition = test_composition();
    let runtime = composition.runtime_handle();
    let show_branch = MutableState::with_runtime(true, runtime);
    let root_key = location_key(file!(), line!(), column!());
    let remembered = Rc::new(RefCell::new(None::<Owned<i32>>));
    let emitted_node = Rc::new(Cell::new(None::<NodeId>));
    let payload_drops = Rc::new(Cell::new(0));
    let node_unmounts = Rc::new(Cell::new(0));
    let branch_invocations = Rc::new(Cell::new(0));

    let render = |composition: &mut Composition<MemoryApplier>| {
        let remembered = Rc::clone(&remembered);
        let emitted_node = Rc::clone(&emitted_node);
        let payload_drops = Rc::clone(&payload_drops);
        let node_unmounts = Rc::clone(&node_unmounts);
        let branch_invocations = Rc::clone(&branch_invocations);
        composition
            .render(root_key, || {
                remembered.replace(None);
                emitted_node.set(None);
                if show_branch.value() {
                    with_current_composer(|composer| {
                        composer.cranpose_with_reuse(
                            BRANCH_KEY,
                            RecomposeOptions::default(),
                            |composer| {
                                branch_invocations.set(branch_invocations.get() + 1);
                                let slot = composer.remember(|| 7_i32);
                                remembered.replace(Some(slot));
                                let _drop_payload = composer.remember(|| {
                                    ReentrantDropState::new(1, Rc::clone(&payload_drops), false)
                                });
                                let node_id = composer.emit_node(|| {
                                    UnmountTrackingNode::new(Rc::clone(&node_unmounts))
                                });
                                emitted_node.set(Some(node_id));
                            },
                        );
                    });
                }
            })
            .expect("render retained branch lifecycle");
        assert_composition_valid(composition);
    };

    render(&mut composition);
    let first_slot = remembered
        .borrow()
        .clone()
        .expect("initial retained payload");
    first_slot.replace(41);
    let first_node = emitted_node.get().expect("initial retained node");
    let active_scope_count = composition
        .debug_slot_snapshot()
        .runtime_scope_registry_count
        .expect("composition snapshot should include runtime scope registry count");
    let active_scope_index_count = composition.debug_slot_table_stats().scope_index_count;
    assert_eq!(branch_invocations.get(), 1);

    show_branch.set_value(false);
    render(&mut composition);

    assert!(
        remembered.borrow().is_none(),
        "inactive retained branch must not expose an active payload handle"
    );
    assert_eq!(emitted_node.get(), None);
    assert_eq!(payload_drops.get(), 0, "retained payload must not drop");
    assert_eq!(node_unmounts.get(), 0, "retained node must not unmount");
    assert!(
        composition.applier_mut().get_mut(first_node).is_ok(),
        "retained detached node must stay live in the applier"
    );
    let hidden_snapshot = composition.debug_slot_snapshot();
    let hidden_stats = composition.debug_slot_table_stats();
    assert_eq!(hidden_snapshot.retained_subtree_count, 1);
    assert_eq!(hidden_snapshot.retained_scope_count, 1);
    assert_eq!(hidden_snapshot.retained_payload_count, 2);
    assert_eq!(
        hidden_snapshot
            .runtime_scope_registry_count
            .expect("composition snapshot should include runtime scope registry count"),
        active_scope_count
    );
    assert_eq!(hidden_stats.retained_payload_count, 2);
    assert_eq!(hidden_stats.retained_node_count, 1);

    show_branch.set_value(true);
    render(&mut composition);

    let restored_slot = remembered
        .borrow()
        .clone()
        .expect("restored retained payload");
    assert_eq!(restored_slot.with(|value| *value), 41);
    assert_eq!(emitted_node.get(), Some(first_node));
    assert_eq!(payload_drops.get(), 0);
    assert_eq!(node_unmounts.get(), 0);
    assert_eq!(branch_invocations.get(), 2);
    assert_eq!(
        composition.debug_slot_table_stats().scope_index_count,
        active_scope_index_count,
        "restored retained scope must re-enter the active scope index"
    );
    assert_eq!(composition.debug_slot_snapshot().retained_subtree_count, 0);
    assert_composition_valid(&composition);
}

#[test]
fn retention_budget_evicts_least_recently_detached_subtree() {
    let mut composition = test_composition();
    composition.set_retention_policy(RetentionPolicy {
        budget: RetentionBudget {
            max_retained_subtrees: Some(1),
            ..Default::default()
        },
        eviction: RetentionEvictionPolicy::LeastRecentlyDetached,
    });
    let runtime = composition.runtime_handle();
    let show_first = MutableState::with_runtime(true, runtime.clone());
    let show_second = MutableState::with_runtime(true, runtime);
    let root_key = location_key(file!(), line!(), column!());
    let first_key = location_key(file!(), line!(), column!());
    let second_key = location_key(file!(), line!(), column!());
    let first_slot = Rc::new(RefCell::new(None::<Owned<i32>>));
    let second_slot = Rc::new(RefCell::new(None::<Owned<i32>>));

    let render = |composition: &mut Composition<MemoryApplier>| {
        let first_slot = Rc::clone(&first_slot);
        let second_slot = Rc::clone(&second_slot);
        composition
            .render(root_key, || {
                first_slot.replace(None);
                second_slot.replace(None);
                with_current_composer(|composer| {
                    if show_first.value() {
                        composer.cranpose_with_reuse(
                            first_key,
                            RecomposeOptions::default(),
                            |composer| {
                                let slot = composer.remember(|| 10_i32);
                                first_slot.replace(Some(slot));
                            },
                        );
                    }
                    if show_second.value() {
                        composer.cranpose_with_reuse(
                            second_key,
                            RecomposeOptions::default(),
                            |composer| {
                                let slot = composer.remember(|| 20_i32);
                                second_slot.replace(Some(slot));
                            },
                        );
                    }
                });
            })
            .expect("render retained budget branches");
        assert_composition_valid(composition);
    };

    render(&mut composition);
    let first = first_slot
        .borrow()
        .clone()
        .expect("initial first retained slot");
    let second = second_slot
        .borrow()
        .clone()
        .expect("initial second retained slot");
    first.replace(101);
    second.replace(202);

    show_first.set_value(false);
    show_second.set_value(false);
    render(&mut composition);
    let hidden_stats = composition.debug_slot_table_stats();
    assert_eq!(hidden_stats.retained_subtree_count, 1);
    assert_eq!(hidden_stats.retained_evictions_total, 1);

    show_second.set_value(true);
    render(&mut composition);
    let restored_second = second_slot
        .borrow()
        .clone()
        .expect("second retained slot after restore");
    assert_eq!(restored_second.with(|value| *value), 202);
    assert_eq!(
        composition
            .debug_slot_table_stats()
            .retained_evictions_total,
        1
    );

    show_second.set_value(false);
    show_first.set_value(true);
    render(&mut composition);
    let recreated_first = first_slot
        .borrow()
        .clone()
        .expect("first slot after eviction");
    assert_eq!(
        recreated_first.with(|value| *value),
        10,
        "the oldest retained subtree should be evicted instead of restoring stale remembered state",
    );
}

#[test]
fn retention_budget_eviction_disposes_nodes_payloads_anchors_and_scopes() {
    const FIRST_KEY: Key = 0xE71;
    const SECOND_KEY: Key = 0xE72;

    let mut composition = test_composition();
    composition.set_retention_policy(RetentionPolicy {
        budget: RetentionBudget {
            max_retained_subtrees: Some(1),
            ..Default::default()
        },
        eviction: RetentionEvictionPolicy::LeastRecentlyDetached,
    });
    let runtime = composition.runtime_handle();
    let show_first = MutableState::with_runtime(true, runtime.clone());
    let show_second = MutableState::with_runtime(true, runtime);
    let root_key = location_key(file!(), line!(), column!());
    let first_slot = Rc::new(RefCell::new(None::<Owned<i32>>));
    let second_slot = Rc::new(RefCell::new(None::<Owned<i32>>));
    let first_node = Rc::new(Cell::new(None::<NodeId>));
    let second_node = Rc::new(Cell::new(None::<NodeId>));
    let first_payload_drops = Rc::new(Cell::new(0));
    let second_payload_drops = Rc::new(Cell::new(0));
    let first_node_unmounts = Rc::new(Cell::new(0));
    let second_node_unmounts = Rc::new(Cell::new(0));

    let render = |composition: &mut Composition<MemoryApplier>| {
        let first_slot = Rc::clone(&first_slot);
        let second_slot = Rc::clone(&second_slot);
        let first_node = Rc::clone(&first_node);
        let second_node = Rc::clone(&second_node);
        let first_payload_drops = Rc::clone(&first_payload_drops);
        let second_payload_drops = Rc::clone(&second_payload_drops);
        let first_node_unmounts = Rc::clone(&first_node_unmounts);
        let second_node_unmounts = Rc::clone(&second_node_unmounts);
        composition
            .render(root_key, || {
                first_slot.replace(None);
                second_slot.replace(None);
                first_node.set(None);
                second_node.set(None);
                with_current_composer(|composer| {
                    if show_first.value() {
                        composer.cranpose_with_reuse(
                            FIRST_KEY,
                            RecomposeOptions::default(),
                            |composer| {
                                first_slot.replace(Some(composer.remember(|| 10_i32)));
                                let _drop_payload = composer.remember(|| {
                                    ReentrantDropState::new(
                                        1,
                                        Rc::clone(&first_payload_drops),
                                        false,
                                    )
                                });
                                first_node.set(Some(composer.emit_node(|| {
                                    UnmountTrackingNode::new(Rc::clone(&first_node_unmounts))
                                })));
                            },
                        );
                    }
                    if show_second.value() {
                        composer.cranpose_with_reuse(
                            SECOND_KEY,
                            RecomposeOptions::default(),
                            |composer| {
                                second_slot.replace(Some(composer.remember(|| 20_i32)));
                                let _drop_payload = composer.remember(|| {
                                    ReentrantDropState::new(
                                        2,
                                        Rc::clone(&second_payload_drops),
                                        false,
                                    )
                                });
                                second_node.set(Some(composer.emit_node(|| {
                                    UnmountTrackingNode::new(Rc::clone(&second_node_unmounts))
                                })));
                            },
                        );
                    }
                });
            })
            .expect("render retained eviction branches");
        assert_composition_valid(composition);
    };

    render(&mut composition);
    let first_initial_slot = first_slot
        .borrow()
        .clone()
        .expect("initial first retained slot");
    let second_initial_slot = second_slot
        .borrow()
        .clone()
        .expect("initial second retained slot");
    first_initial_slot.replace(101);
    second_initial_slot.replace(202);
    let first_initial_node = first_node.get().expect("initial first retained node");
    let second_initial_node = second_node.get().expect("initial second retained node");

    show_first.set_value(false);
    show_second.set_value(false);
    render(&mut composition);

    let hidden_stats = composition.debug_slot_table_stats();
    assert_eq!(hidden_stats.retained_subtree_count, 1);
    assert_eq!(hidden_stats.retained_node_count, 1);
    assert_eq!(hidden_stats.retained_scope_count, 1);
    assert_eq!(hidden_stats.retained_evictions_total, 1);
    assert!(
        hidden_stats.free_anchor_count > 0,
        "evicted group anchors must be invalidated for reuse"
    );
    assert!(
        hidden_stats.free_payload_anchor_count > 0,
        "evicted payload anchors must be invalidated for reuse"
    );
    assert_eq!(first_payload_drops.get(), 1);
    assert_eq!(first_node_unmounts.get(), 1);
    assert_eq!(second_payload_drops.get(), 0);
    assert_eq!(second_node_unmounts.get(), 0);
    assert!(
        composition
            .applier_mut()
            .get_mut(first_initial_node)
            .is_err(),
        "evicted retained node must be removed from the applier"
    );
    assert!(
        composition
            .applier_mut()
            .get_mut(second_initial_node)
            .is_ok(),
        "the retained subtree still inside the budget must keep its node live"
    );

    show_first.set_value(true);
    render(&mut composition);
    let recreated_first_slot = first_slot
        .borrow()
        .clone()
        .expect("first branch after eviction");
    assert_eq!(
        recreated_first_slot.with(|value| *value),
        10,
        "evicted payload state must not be restored"
    );
    assert_ne!(
        first_node.get(),
        Some(first_initial_node),
        "evicted node identity must not be reused as a retained node"
    );
    assert_composition_valid(&composition);
}

#[test]
fn secondary_host_reset_disposes_retained_subtrees_before_clearing_ownership() {
    const BRANCH_KEY: Key = 0x5EC0;

    let (handle, _runtime) = runtime_handle();
    let mut slots = SlotTable::default();
    let mut applier = MemoryApplier::new();
    let (composer, slots_host, applier_host) =
        setup_composer(&mut slots, &mut applier, handle, None);
    composer.enter_phase(Phase::Measure);

    let secondary_host = Rc::new(SlotsHost::new(SlotTable::default()));
    let emitted_node = Rc::new(Cell::new(None::<NodeId>));
    let payload_drops = Rc::new(Cell::new(0));
    let node_unmounts = Rc::new(Cell::new(0));

    let subcompose = |show_branch: bool| {
        let emitted_node = Rc::clone(&emitted_node);
        let payload_drops = Rc::clone(&payload_drops);
        let node_unmounts = Rc::clone(&node_unmounts);
        composer
            .subcompose_slot(&secondary_host, None, |composer| {
                emitted_node.set(None);
                if show_branch {
                    composer.cranpose_with_reuse(
                        BRANCH_KEY,
                        RecomposeOptions::default(),
                        |composer| {
                            let _payload = composer.remember(|| {
                                ReentrantDropState::new(7, Rc::clone(&payload_drops), false)
                            });
                            let node_id = composer
                                .emit_node(|| UnmountTrackingNode::new(Rc::clone(&node_unmounts)));
                            emitted_node.set(Some(node_id));
                        },
                    );
                }
            })
            .expect("subcompose retained branch");
        secondary_host
            .runtime_state()
            .expect("secondary host must bind to composer runtime")
            .validate_host_retention(&secondary_host, &secondary_host.borrow())
            .expect("secondary host retention must validate");
    };

    subcompose(true);
    let retained_node = emitted_node
        .get()
        .expect("secondary host retained node must be emitted");
    let retained_host_key = secondary_host.storage_key();
    assert!(
        composer
            .core
            .shared_state
            .host_for_storage_key(retained_host_key)
            .is_some(),
        "secondary host must be registered before reset"
    );

    subcompose(false);
    let retained_stats = secondary_host.debug_stats();
    assert_eq!(retained_stats.retained_subtree_count, 1);
    assert_eq!(retained_stats.retained_payload_count, 1);
    assert_eq!(retained_stats.retained_node_count, 1);
    assert_eq!(retained_stats.retained_scope_count, 1);
    assert_eq!(payload_drops.get(), 0);
    assert_eq!(node_unmounts.get(), 0);
    assert!(
        applier_host.borrow_typed().get_mut(retained_node).is_ok(),
        "retained secondary-host node must stay live before reset"
    );

    secondary_host.reset().expect("secondary host reset");

    assert_eq!(payload_drops.get(), 1);
    assert_eq!(node_unmounts.get(), 1);
    assert!(
        applier_host.borrow_typed().get_mut(retained_node).is_err(),
        "secondary host reset must dispose retained nodes before clearing ownership"
    );
    assert!(
        composer
            .core
            .shared_state
            .host_for_storage_key(retained_host_key)
            .is_none(),
        "secondary host reset must clear the old host storage key"
    );
    assert_eq!(
        composer.core.shared_state.scope_registry_len(),
        0,
        "secondary host reset must remove retained scopes from the runtime registry"
    );
    assert_eq!(secondary_host.debug_stats().retained_subtree_count, 0);
    assert_eq!(secondary_host.borrow().validate(), Ok(()));

    drop(composer);
    drop(secondary_host);
    teardown_composer(&mut slots, &mut applier, slots_host, applier_host);
}

struct FailingRetainedDisposalApplier {
    fail_id: NodeId,
    inner: MemoryApplier,
}

impl Applier for FailingRetainedDisposalApplier {
    fn create(&mut self, node: Box<dyn Node>) -> NodeId {
        self.inner.create(node)
    }

    fn get_mut(&mut self, id: NodeId) -> Result<&mut dyn Node, NodeError> {
        if id == self.fail_id {
            Err(NodeError::TypeMismatch {
                id,
                expected: "retained disposal failure",
            })
        } else {
            self.inner.get_mut(id)
        }
    }

    fn remove(&mut self, id: NodeId) -> Result<(), NodeError> {
        if id == self.fail_id {
            Err(NodeError::TypeMismatch {
                id,
                expected: "retained disposal failure",
            })
        } else {
            self.inner.remove(id)
        }
    }

    fn node_generation(&self, id: NodeId) -> u32 {
        self.inner.node_generation(id)
    }

    fn insert_with_id(&mut self, id: NodeId, node: Box<dyn Node>) -> Result<(), NodeError> {
        self.inner.insert_with_id(id, node)
    }
}

#[test]
fn failed_secondary_host_reset_keeps_retained_state_owned() {
    const BRANCH_KEY: Key = 0x5EC1;

    let (handle, _runtime) = runtime_handle();
    let mut slots = SlotTable::default();
    let mut applier = MemoryApplier::new();
    let (composer, slots_host, applier_host) =
        setup_composer(&mut slots, &mut applier, handle, None);
    composer.enter_phase(Phase::Measure);

    let secondary_host = Rc::new(SlotsHost::new(SlotTable::default()));
    let emitted_node = Rc::new(Cell::new(None::<NodeId>));

    let subcompose = |show_branch: bool| {
        let emitted_node = Rc::clone(&emitted_node);
        composer
            .subcompose_slot(&secondary_host, None, |composer| {
                emitted_node.set(None);
                if show_branch {
                    composer.cranpose_with_reuse(
                        BRANCH_KEY,
                        RecomposeOptions::default(),
                        |composer| {
                            let node_id = composer.emit_node(TrackingChild::default);
                            emitted_node.set(Some(node_id));
                        },
                    );
                }
            })
            .expect("subcompose retained branch");
    };

    subcompose(true);
    let retained_node = emitted_node
        .get()
        .expect("secondary host retained node must be emitted");
    let retained_host_key = secondary_host.storage_key();
    subcompose(false);
    assert_eq!(secondary_host.debug_stats().retained_subtree_count, 1);

    let failing_host: Rc<dyn ApplierHost> =
        Rc::new(ConcreteApplierHost::new(FailingRetainedDisposalApplier {
            fail_id: retained_node,
            inner: MemoryApplier::new(),
        }));
    composer.core.shared_state.bind_applier_host(&failing_host);

    assert_eq!(
        secondary_host.reset(),
        Err(NodeError::TypeMismatch {
            id: retained_node,
            expected: "retained disposal failure",
        })
    );
    assert_eq!(
        secondary_host.debug_stats().retained_subtree_count,
        1,
        "failed retained disposal must keep subtree state available for retry"
    );
    assert!(
        composer
            .core
            .shared_state
            .host_for_storage_key(retained_host_key)
            .is_some(),
        "failed retained disposal must not clear live host ownership"
    );

    let original_host: Rc<dyn ApplierHost> = applier_host.clone();
    composer.core.shared_state.bind_applier_host(&original_host);
    secondary_host.reset().expect("secondary host reset retry");

    assert_eq!(secondary_host.debug_stats().retained_subtree_count, 0);
    assert!(
        composer
            .core
            .shared_state
            .host_for_storage_key(retained_host_key)
            .is_none(),
        "successful retry must clear live host ownership"
    );

    drop(composer);
    drop(secondary_host);
    drop(original_host);
    drop(failing_host);
    teardown_composer(&mut slots, &mut applier, slots_host, applier_host);
}

#[test]
fn retention_budget_evicts_subtree_after_max_age_passes() {
    let mut composition = test_composition();
    composition.set_retention_policy(RetentionPolicy {
        budget: RetentionBudget {
            max_age_passes: Some(0),
            ..Default::default()
        },
        eviction: RetentionEvictionPolicy::LeastRecentlyDetached,
    });
    let runtime = composition.runtime_handle();
    let show_branch = MutableState::with_runtime(true, runtime);
    let root_key = location_key(file!(), line!(), column!());
    let branch_key = location_key(file!(), line!(), column!());
    let remembered = Rc::new(RefCell::new(None::<Owned<i32>>));

    let render = |composition: &mut Composition<MemoryApplier>| {
        let remembered = Rc::clone(&remembered);
        composition
            .render(root_key, || {
                remembered.replace(None);
                if show_branch.value() {
                    with_current_composer(|composer| {
                        composer.cranpose_with_reuse(
                            branch_key,
                            RecomposeOptions::default(),
                            |composer| {
                                let slot = composer.remember(|| 30_i32);
                                remembered.replace(Some(slot));
                            },
                        );
                    });
                }
            })
            .expect("render retained age budget branch");
        assert_composition_valid(composition);
    };

    render(&mut composition);
    let first = remembered
        .borrow()
        .clone()
        .expect("initial retained age-budget slot");
    first.replace(303);

    show_branch.set_value(false);
    render(&mut composition);
    let hidden_stats = composition.debug_slot_table_stats();
    assert_eq!(hidden_stats.retained_subtree_count, 0);
    assert_eq!(hidden_stats.retained_evictions_total, 1);

    show_branch.set_value(true);
    render(&mut composition);
    let recreated = remembered
        .borrow()
        .clone()
        .expect("age-evicted branch should compose again");
    assert_eq!(recreated.with(|value| *value), 30);
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct SlotMemoryPlateau {
    group_count: usize,
    group_capacity: usize,
    group_heap_bytes: usize,
    payload_count: usize,
    payload_capacity: usize,
    active_payload_anchor_count: usize,
    payload_anchor_slot_count: usize,
    detached_payload_anchor_count: usize,
    invalidated_payload_anchor_count: usize,
    free_payload_anchor_count: usize,
    payload_anchor_capacity: usize,
    payload_anchor_heap_bytes: usize,
    node_count: usize,
    node_capacity: usize,
    pending_drop_count: usize,
    pending_drop_capacity: usize,
    active_anchor_count: usize,
    anchor_slot_count: usize,
    anchor_sparse_count: usize,
    detached_anchor_count: usize,
    invalidated_anchor_count: usize,
    free_anchor_count: usize,
    anchor_capacity: usize,
    anchor_heap_bytes: usize,
    scope_index_count: usize,
    scope_index_capacity: usize,
    retained_subtree_count: usize,
    retained_group_count: usize,
    retained_payload_count: usize,
    retained_node_count: usize,
    retained_scope_count: usize,
    retained_anchor_count: usize,
    retained_heap_bytes: usize,
    retained_evictions_total: usize,
}

type CapturedRetainedSlots = Rc<RefCell<Vec<(usize, Owned<usize>)>>>;

impl From<SlotTableDebugStats> for SlotMemoryPlateau {
    fn from(stats: SlotTableDebugStats) -> Self {
        Self {
            group_count: stats.group_count,
            group_capacity: stats.group_capacity,
            group_heap_bytes: stats.group_heap_bytes,
            payload_count: stats.payload_count,
            payload_capacity: stats.payload_capacity,
            active_payload_anchor_count: stats.active_payload_anchor_count,
            payload_anchor_slot_count: stats.payload_anchor_slot_count,
            detached_payload_anchor_count: stats.detached_payload_anchor_count,
            invalidated_payload_anchor_count: stats.invalidated_payload_anchor_count,
            free_payload_anchor_count: stats.free_payload_anchor_count,
            payload_anchor_capacity: stats.payload_anchor_capacity,
            payload_anchor_heap_bytes: stats.payload_anchor_heap_bytes,
            node_count: stats.node_count,
            node_capacity: stats.node_capacity,
            pending_drop_count: stats.pending_drop_count,
            pending_drop_capacity: stats.pending_drop_capacity,
            active_anchor_count: stats.active_anchor_count,
            anchor_slot_count: stats.anchor_slot_count,
            anchor_sparse_count: stats.anchor_sparse_count,
            detached_anchor_count: stats.detached_anchor_count,
            invalidated_anchor_count: stats.invalidated_anchor_count,
            free_anchor_count: stats.free_anchor_count,
            anchor_capacity: stats.anchor_capacity,
            anchor_heap_bytes: stats.anchor_heap_bytes,
            scope_index_count: stats.scope_index_count,
            scope_index_capacity: stats.scope_index_capacity,
            retained_subtree_count: stats.retained_subtree_count,
            retained_group_count: stats.retained_group_count,
            retained_payload_count: stats.retained_payload_count,
            retained_node_count: stats.retained_node_count,
            retained_scope_count: stats.retained_scope_count,
            retained_anchor_count: stats.retained_anchor_count,
            retained_heap_bytes: stats.retained_heap_bytes,
            retained_evictions_total: stats.retained_evictions_total,
        }
    }
}

fn assert_slot_memory_plateau(
    label: &str,
    baseline: SlotMemoryPlateau,
    current: SlotMemoryPlateau,
    cycle: usize,
) {
    assert_eq!(
        current, baseline,
        "{label} retained memory diagnostics changed after warmup cycle {cycle}: baseline={baseline:?} current={current:?}",
    );
}

#[test]
fn retained_tab_cycles_plateau_memory_and_anchors() {
    const TAB_KEYS: [Key; 4] = [0x7101, 0x7102, 0x7103, 0x7104];

    let mut composition = test_composition();
    let runtime = composition.runtime_handle();
    let active_tab = MutableState::with_runtime(0usize, runtime);
    let root_key = location_key(file!(), line!(), column!());
    let captured: CapturedRetainedSlots = Rc::new(RefCell::new(Vec::new()));

    fn render_tab_host(
        composition: &mut Composition<MemoryApplier>,
        active_tab: MutableState<usize>,
        root_key: Key,
        captured: &CapturedRetainedSlots,
    ) {
        let captured = Rc::clone(captured);
        composition
            .render(root_key, || {
                captured.borrow_mut().clear();
                let tab = active_tab.value();
                with_current_composer(|composer| {
                    composer.cranpose_with_reuse(
                        TAB_KEYS[tab],
                        RecomposeOptions::default(),
                        |composer| {
                            let slot = composer.remember(|| tab);
                            captured.borrow_mut().push((tab, slot));
                        },
                    );
                });
            })
            .expect("render retained tab host");
        assert_composition_valid(composition);
    }

    for tab in 0..TAB_KEYS.len() {
        active_tab.set_value(tab);
        render_tab_host(&mut composition, active_tab, root_key, &captured);
        captured.borrow()[0].1.replace(tab + 100);
    }
    active_tab.set_value(0);
    render_tab_host(&mut composition, active_tab, root_key, &captured);

    let baseline = SlotMemoryPlateau::from(composition.debug_slot_table_stats());
    assert_eq!(baseline.retained_subtree_count, TAB_KEYS.len() - 1);
    assert!(
        baseline.retained_heap_bytes > 0,
        "retained tabs should report retained heap bytes after warmup",
    );

    for cycle in 0..25 {
        for tab in 1..TAB_KEYS.len() {
            active_tab.set_value(tab);
            render_tab_host(&mut composition, active_tab, root_key, &captured);
            let restored = captured.borrow()[0].1.with(|value| *value);
            assert_eq!(restored, tab + 100);
        }
        active_tab.set_value(0);
        render_tab_host(&mut composition, active_tab, root_key, &captured);
        assert_eq!(captured.borrow()[0].1.with(|value| *value), 100);

        assert_slot_memory_plateau(
            "retained tab cycles",
            baseline,
            SlotMemoryPlateau::from(composition.debug_slot_table_stats()),
            cycle,
        );
    }
}

#[test]
fn retained_keyed_list_window_plateaus_memory_and_anchors() {
    const ITEM_COUNT: usize = 12;
    const WINDOW_SIZE: usize = 4;
    const ITEM_KEYS: [Key; ITEM_COUNT] = [
        0x7150, 0x7151, 0x7152, 0x7153, 0x7154, 0x7155, 0x7156, 0x7157, 0x7158, 0x7159, 0x715A,
        0x715B,
    ];

    let mut composition = test_composition();
    let runtime = composition.runtime_handle();
    let window_start = MutableState::with_runtime(0usize, runtime);
    let root_key = location_key(file!(), line!(), column!());
    let captured: CapturedRetainedSlots = Rc::new(RefCell::new(Vec::new()));

    fn render_list_window(
        composition: &mut Composition<MemoryApplier>,
        window_start: MutableState<usize>,
        root_key: Key,
        captured: &CapturedRetainedSlots,
    ) {
        let captured = Rc::clone(captured);
        composition
            .render(root_key, || {
                captured.borrow_mut().clear();
                let start = window_start.value();
                for offset in 0..WINDOW_SIZE {
                    let item = (start + offset) % ITEM_COUNT;
                    with_current_composer(|composer| {
                        composer.cranpose_with_reuse(
                            ITEM_KEYS[item],
                            RecomposeOptions::default(),
                            |composer| {
                                let slot = composer.remember(|| item);
                                captured.borrow_mut().push((item, slot));
                            },
                        );
                    });
                }
            })
            .expect("render retained keyed list window");
        assert_composition_valid(composition);
    }

    for start in 0..ITEM_COUNT {
        window_start.set_value(start);
        render_list_window(&mut composition, window_start, root_key, &captured);
        for (item, slot) in captured.borrow().iter() {
            slot.replace(item + 1_000);
        }
    }
    window_start.set_value(0);
    render_list_window(&mut composition, window_start, root_key, &captured);

    let baseline = SlotMemoryPlateau::from(composition.debug_slot_table_stats());
    assert_eq!(
        baseline.retained_subtree_count,
        ITEM_COUNT - WINDOW_SIZE,
        "warmup should retain exactly the inactive list items",
    );
    assert!(
        baseline.retained_heap_bytes > 0,
        "retained list window should report retained heap bytes after warmup",
    );

    for cycle in 0..20 {
        for start in 1..ITEM_COUNT {
            window_start.set_value(start);
            render_list_window(&mut composition, window_start, root_key, &captured);
            for (item, slot) in captured.borrow().iter() {
                assert_eq!(slot.with(|value| *value), item + 1_000);
            }
        }
        window_start.set_value(0);
        render_list_window(&mut composition, window_start, root_key, &captured);
        for (item, slot) in captured.borrow().iter() {
            assert_eq!(slot.with(|value| *value), item + 1_000);
        }

        assert_slot_memory_plateau(
            "retained keyed list window",
            baseline,
            SlotMemoryPlateau::from(composition.debug_slot_table_stats()),
            cycle,
        );
    }
}

#[test]
fn subcompose_retained_root_slots_plateau_memory_and_anchors() {
    const SLOT_KEYS: [Key; 4] = [0x7201, 0x7202, 0x7203, 0x7204];

    let (handle, _runtime) = runtime_handle();
    let mut slots = SlotTable::default();
    let mut applier = test_applier();
    let (composer, slots_host, applier_host) =
        setup_composer(&mut slots, &mut applier, handle.clone(), None);
    let subcompose_slots = Rc::new(SlotsHost::new(SlotTable::new()));
    let captured: CapturedRetainedSlots = Rc::new(RefCell::new(Vec::new()));

    let render = |active_slot: usize| {
        let captured = Rc::clone(&captured);
        composer
            .subcompose_in(&subcompose_slots, None, |composer| {
                captured.borrow_mut().clear();
                composer.cranpose_with_reuse(
                    SLOT_KEYS[active_slot],
                    RecomposeOptions::default(),
                    |composer| {
                        let slot = composer.remember(|| active_slot);
                        captured.borrow_mut().push((active_slot, slot));
                    },
                );
            })
            .expect("subcompose retained root slot render");
        subcompose_slots
            .borrow()
            .validate()
            .expect("subcompose slot table must validate");
    };

    for slot in 0..SLOT_KEYS.len() {
        render(slot);
        captured.borrow()[0].1.replace(slot + 2_000);
    }
    render(0);

    let baseline = SlotMemoryPlateau::from(subcompose_slots.debug_stats());
    assert_eq!(baseline.retained_subtree_count, SLOT_KEYS.len() - 1);
    assert!(
        baseline.retained_heap_bytes > 0,
        "retained subcompose slots should report retained heap bytes after warmup",
    );

    for cycle in 0..25 {
        for slot in 1..SLOT_KEYS.len() {
            render(slot);
            assert_eq!(captured.borrow()[0].1.with(|value| *value), slot + 2_000);
        }
        render(0);
        assert_eq!(captured.borrow()[0].1.with(|value| *value), 2_000);

        assert_slot_memory_plateau(
            "subcompose retained root slots",
            baseline,
            SlotMemoryPlateau::from(subcompose_slots.debug_stats()),
            cycle,
        );
    }

    drop(composer);
    teardown_composer(&mut slots, &mut applier, slots_host, applier_host);
}

#[test]
fn retained_branch_hides_without_running_disposable_effect_cleanup() {
    let mut composition = test_composition();
    let runtime = composition.runtime_handle();
    let show_branch = MutableState::with_runtime(true, runtime.clone());
    let cleanup_calls = Rc::new(Cell::new(0usize));
    let root_key = location_key(file!(), line!(), column!());
    let branch_key = location_key(file!(), line!(), column!());

    let mut render = || {
        let cleanup_calls = Rc::clone(&cleanup_calls);
        composition
            .render(root_key, || {
                if show_branch.value() {
                    with_current_composer(|composer| {
                        let cleanup_calls = Rc::clone(&cleanup_calls);
                        composer.cranpose_with_reuse(
                            branch_key,
                            RecomposeOptions::default(),
                            move |_composer| {
                                let cleanup_calls = Rc::clone(&cleanup_calls);
                                DisposableEffect!((), move |_| {
                                    let cleanup_calls = Rc::clone(&cleanup_calls);
                                    DisposableEffectResult::new(move || {
                                        cleanup_calls.set(cleanup_calls.get() + 1);
                                    })
                                });
                            },
                        );
                    });
                }
            })
            .expect("render retained disposable branch");
        assert_composition_valid(&composition);
    };

    render();
    assert_eq!(cleanup_calls.get(), 0);

    show_branch.set_value(false);
    render();
    assert_eq!(
        cleanup_calls.get(),
        0,
        "retained branches must not run DisposableEffect cleanup merely because they went inactive",
    );

    show_branch.set_value(true);
    render();
    assert_eq!(
        cleanup_calls.get(),
        0,
        "restoring a retained branch must not trigger disposal cleanup for the preserved effect",
    );
}

#[test]
fn conditional_branch_without_retention_runs_disposable_effect_cleanup() {
    let mut composition = test_composition();
    let runtime = composition.runtime_handle();
    let show_branch = MutableState::with_runtime(true, runtime.clone());
    let cleanup_calls = Rc::new(Cell::new(0usize));
    let root_key = location_key(file!(), line!(), column!());
    let branch_key = location_key(file!(), line!(), column!());

    let mut render = || {
        let cleanup_calls = Rc::clone(&cleanup_calls);
        composition
            .render(root_key, || {
                if show_branch.value() {
                    with_current_composer(|composer| {
                        composer.with_group(branch_key, |_composer| {
                            let cleanup_calls = Rc::clone(&cleanup_calls);
                            DisposableEffect!((), move |_| {
                                let cleanup_calls = Rc::clone(&cleanup_calls);
                                DisposableEffectResult::new(move || {
                                    cleanup_calls.set(cleanup_calls.get() + 1);
                                })
                            });
                        });
                    });
                }
            })
            .expect("render disposable conditional branch");
        assert_composition_valid(&composition);
    };

    render();
    assert_eq!(cleanup_calls.get(), 0);

    show_branch.set_value(false);
    render();
    assert_eq!(
        cleanup_calls.get(),
        1,
        "removing a non-retained branch must run DisposableEffect cleanup",
    );

    show_branch.set_value(true);
    render();
    assert_eq!(
        cleanup_calls.get(),
        1,
        "recreating a disposed branch must not run cleanup until it is removed again",
    );
}

#[test]
fn retained_branch_restores_the_same_node_id() {
    const BRANCH_KEY: Key = 0xD01;

    let mut composition = test_composition();
    let runtime = composition.runtime_handle();
    let show_branch = MutableState::with_runtime(true, runtime.clone());
    let root_key = location_key(file!(), line!(), column!());
    let captured = Rc::new(RefCell::new(Vec::<NodeId>::new()));

    let mut render = || {
        let captured = Rc::clone(&captured);
        composition
            .render(root_key, || {
                if show_branch.value() {
                    with_current_composer(|composer| {
                        let captured = Rc::clone(&captured);
                        composer.cranpose_with_reuse(
                            BRANCH_KEY,
                            RecomposeOptions::default(),
                            move |_composer| {
                                let node_id = cranpose_test_node(TestTextNode::default);
                                captured.borrow_mut().push(node_id);
                            },
                        );
                    });
                }
            })
            .expect("render retained node branch");
        assert_composition_valid(&composition);
    };

    render();
    let first = *captured.borrow().last().expect("initial retained node id");

    show_branch.set_value(false);
    render();

    show_branch.set_value(true);
    render();
    let restored = *captured.borrow().last().expect("restored retained node id");

    assert_eq!(
        restored, first,
        "retained branches must restore the same node id instead of recreating the subtree",
    );
}

#[test]
fn retained_branch_detaches_child_from_parent_while_hidden_and_restores_same_node_id() {
    const BRANCH_KEY: Key = 0xD04;

    let mut composition = test_composition();
    let runtime = composition.runtime_handle();
    let show_branch = MutableState::with_runtime(true, runtime.clone());
    let root_key = location_key(file!(), line!(), column!());
    let parent_id = Rc::new(Cell::new(None::<NodeId>));
    let captured = Rc::new(RefCell::new(Vec::<NodeId>::new()));

    let render = |composition: &mut Composition<MemoryApplier>,
                  show_branch: MutableState<bool>,
                  parent_id: &Rc<Cell<Option<NodeId>>>,
                  captured: &Rc<RefCell<Vec<NodeId>>>| {
        let parent_id = Rc::clone(parent_id);
        let captured = Rc::clone(captured);
        composition
            .render(root_key, || {
                let container =
                    with_current_composer(|composer| composer.emit_node(RecordingNode::default));
                parent_id.set(Some(container));
                cranpose_core::push_parent(container);
                if show_branch.value() {
                    with_current_composer(|composer| {
                        let captured = Rc::clone(&captured);
                        composer.cranpose_with_reuse(
                            BRANCH_KEY,
                            RecomposeOptions::default(),
                            move |_composer| {
                                let node_id = cranpose_test_node(TrackingChild::default);
                                captured.borrow_mut().push(node_id);
                            },
                        );
                    });
                }
                cranpose_core::pop_parent();
            })
            .expect("render retained child branch");
        assert_composition_valid(composition);
    };

    render(&mut composition, show_branch, &parent_id, &captured);
    let parent = parent_id.get().expect("parent id");
    let first = *captured
        .borrow()
        .last()
        .expect("initial retained child node id");

    {
        let mut applier = composition.applier_mut();
        let child_parent = applier
            .with_node::<TrackingChild, _>(first, |node| node.parent())
            .expect("retained child should exist");
        let parent_children = applier
            .with_node::<RecordingNode, _>(parent, |node| node.children.clone())
            .expect("parent should exist");
        assert_eq!(child_parent, Some(parent));
        assert_eq!(parent_children, vec![first]);
    }

    show_branch.set_value(false);
    render(&mut composition, show_branch, &parent_id, &captured);

    {
        let mut applier = composition.applier_mut();
        let child_parent = applier
            .with_node::<TrackingChild, _>(first, |node| node.parent())
            .expect("hidden retained child must stay live");
        let parent_children = applier
            .with_node::<RecordingNode, _>(parent, |node| node.children.clone())
            .expect("parent should still exist");
        assert_eq!(
            child_parent, None,
            "hidden retained children must detach from the live parent tree",
        );
        assert!(
            parent_children.is_empty(),
            "retained children must leave the active parent child list while hidden",
        );
    }

    show_branch.set_value(true);
    render(&mut composition, show_branch, &parent_id, &captured);
    let restored = *captured
        .borrow()
        .last()
        .expect("restored retained child node id");

    assert_eq!(
        restored, first,
        "restoring a retained nested branch must reuse the original child node id",
    );

    {
        let mut applier = composition.applier_mut();
        let child_parent = applier
            .with_node::<TrackingChild, _>(restored, |node| node.parent())
            .expect("restored retained child should exist");
        let parent_children = applier
            .with_node::<RecordingNode, _>(parent, |node| node.children.clone())
            .expect("parent should still exist");
        assert_eq!(child_parent, Some(parent));
        assert_eq!(parent_children, vec![restored]);
    }
}

#[test]
fn disposed_branch_recreates_node_id_without_explicit_reuse() {
    const BRANCH_KEY: Key = 0xD02;

    let mut composition = test_composition();
    let runtime = composition.runtime_handle();
    let show_branch = MutableState::with_runtime(true, runtime.clone());
    let root_key = location_key(file!(), line!(), column!());
    let captured = Rc::new(RefCell::new(Vec::<NodeId>::new()));

    let mut render = || {
        let captured = Rc::clone(&captured);
        composition
            .render(root_key, || {
                if show_branch.value() {
                    with_current_composer(|composer| {
                        let captured = Rc::clone(&captured);
                        composer.with_group(BRANCH_KEY, |_composer| {
                            let node_id = cranpose_test_node(TestTextNode::default);
                            captured.borrow_mut().push(node_id);
                        });
                    });
                }
            })
            .expect("render disposable node branch");
        assert_composition_valid(&composition);
    };

    render();
    let first = *captured
        .borrow()
        .last()
        .expect("initial disposable node id");

    show_branch.set_value(false);
    render();

    show_branch.set_value(true);
    render();
    let restored = *captured
        .borrow()
        .last()
        .expect("restored disposable node id");

    assert_ne!(
        restored, first,
        "without explicit retention or applier-level reuse, removing a branch must recreate its node id",
    );
}

#[test]
fn conditional_branch_without_retention_disposes_removed_node() {
    const BRANCH_KEY: Key = 0xD03;

    let mut composition = test_composition();
    let runtime = composition.runtime_handle();
    let show_branch = MutableState::with_runtime(true, runtime.clone());
    let root_key = location_key(file!(), line!(), column!());
    let captured = Rc::new(RefCell::new(Vec::<NodeId>::new()));

    let mut render = || {
        let captured = Rc::clone(&captured);
        composition
            .render(root_key, || {
                if show_branch.value() {
                    with_current_composer(|composer| {
                        let captured = Rc::clone(&captured);
                        composer.with_group(BRANCH_KEY, |_composer| {
                            let node_id = cranpose_test_node(TestTextNode::default);
                            captured.borrow_mut().push(node_id);
                        });
                    });
                }
            })
            .expect("render disposable branch");
        assert_composition_valid(&composition);
    };

    render();
    let first = *captured
        .borrow()
        .last()
        .expect("initial disposable branch node id");

    show_branch.set_value(false);
    render();

    assert!(
        composition.applier_mut().get_mut(first).is_err(),
        "default-retained branches must remove disposed nodes from the applier when hidden",
    );
}

#[test]
fn switching_tabs_with_retention_preserves_each_tab_state() {
    const TAB_A_KEY: Key = 0xA11;
    const TAB_B_KEY: Key = 0xB22;

    let mut composition = test_composition();
    let runtime = composition.runtime_handle();
    let active_tab = MutableState::with_runtime(0usize, runtime.clone());
    let root_key = location_key(file!(), line!(), column!());
    let tab_a = Rc::new(RefCell::new(None::<Owned<i32>>));
    let tab_b = Rc::new(RefCell::new(None::<Owned<i32>>));

    let mut render = || {
        let tab_a = Rc::clone(&tab_a);
        let tab_b = Rc::clone(&tab_b);
        composition
            .render(root_key, || {
                tab_a.replace(None);
                tab_b.replace(None);
                with_current_composer(|composer| match active_tab.value() {
                    0 => composer.cranpose_with_reuse(
                        TAB_A_KEY,
                        RecomposeOptions::default(),
                        |composer| {
                            let slot = composer.remember(|| 10_i32);
                            tab_a.replace(Some(slot));
                        },
                    ),
                    1 => composer.cranpose_with_reuse(
                        TAB_B_KEY,
                        RecomposeOptions::default(),
                        |composer| {
                            let slot = composer.remember(|| 20_i32);
                            tab_b.replace(Some(slot));
                        },
                    ),
                    _ => unreachable!("only two tabs are expected"),
                });
            })
            .expect("render retained tabs");
        assert_composition_valid(&composition);
    };

    render();
    let first_a = tab_a
        .borrow()
        .clone()
        .expect("tab A slot after first render");
    first_a.replace(101);

    active_tab.set_value(1);
    render();
    let first_b = tab_b.borrow().clone().expect("tab B slot after switch");
    first_b.replace(202);

    active_tab.set_value(0);
    render();
    let restored_a = tab_a.borrow().clone().expect("tab A slot after restore");
    assert_eq!(
        restored_a.with(|value| *value),
        101,
        "retained tabs must restore previously remembered state when switched back in",
    );

    active_tab.set_value(1);
    render();
    let restored_b = tab_b
        .borrow()
        .clone()
        .expect("tab B slot after second restore");
    assert_eq!(
        restored_b.with(|value| *value),
        202,
        "each retained tab must preserve its own remembered state independently",
    );
}

#[test]
fn switching_tabs_without_retention_resets_inactive_tab_state() {
    const TAB_A_KEY: Key = 0xA33;
    const TAB_B_KEY: Key = 0xB44;

    let mut composition = test_composition();
    let runtime = composition.runtime_handle();
    let active_tab = MutableState::with_runtime(0usize, runtime.clone());
    let root_key = location_key(file!(), line!(), column!());
    let tab_a = Rc::new(RefCell::new(None::<Owned<i32>>));
    let tab_b = Rc::new(RefCell::new(None::<Owned<i32>>));

    let mut render = || {
        let tab_a = Rc::clone(&tab_a);
        let tab_b = Rc::clone(&tab_b);
        composition
            .render(root_key, || {
                tab_a.replace(None);
                tab_b.replace(None);
                with_current_composer(|composer| match active_tab.value() {
                    0 => composer.with_group(TAB_A_KEY, |composer| {
                        let slot = composer.remember(|| 10_i32);
                        tab_a.replace(Some(slot));
                    }),
                    1 => composer.with_group(TAB_B_KEY, |composer| {
                        let slot = composer.remember(|| 20_i32);
                        tab_b.replace(Some(slot));
                    }),
                    _ => unreachable!("only two tabs are expected"),
                });
            })
            .expect("render non-retained tabs");
        assert_composition_valid(&composition);
    };

    render();
    let first_a = tab_a
        .borrow()
        .clone()
        .expect("tab A slot after first render");
    first_a.replace(101);

    active_tab.set_value(1);
    render();
    let first_b = tab_b.borrow().clone().expect("tab B slot after switch");
    first_b.replace(202);

    active_tab.set_value(0);
    render();
    let restored_a = tab_a.borrow().clone().expect("tab A slot after restore");
    assert_eq!(
        restored_a.with(|value| *value),
        10,
        "without retention, switching away from a tab must dispose its remembered state",
    );

    active_tab.set_value(1);
    render();
    let restored_b = tab_b
        .borrow()
        .clone()
        .expect("tab B slot after second restore");
    assert_eq!(
        restored_b.with(|value| *value),
        20,
        "without retention, each tab must reinitialize when it becomes active again",
    );
}

#[test]
fn list_item_reorder_with_explicit_keys_preserves_item_state() {
    let mut composition = test_composition();
    let root_key = location_key(file!(), line!(), column!());
    let order = Rc::new(RefCell::new(vec![1_i32, 2, 3]));
    let captured = Rc::new(RefCell::new(Vec::<(i32, Owned<i32>)>::new()));

    let mut render = || {
        let order = Rc::clone(&order);
        let captured = Rc::clone(&captured);
        composition
            .render(root_key, || {
                captured.replace(Vec::new());
                let current_order = order.borrow().clone();
                for item in current_order {
                    cranpose_core::with_key(&item, || {
                        let slot =
                            with_current_composer(|composer| composer.remember(|| item * 10));
                        captured.borrow_mut().push((item, slot));
                    });
                }
            })
            .expect("render keyed list");
        assert_composition_valid(&composition);
    };

    render();
    {
        let slots = captured.borrow();
        for (item, value) in [(1, 101), (2, 202), (3, 303)] {
            let slot = slots
                .iter()
                .find(|(captured_item, _)| *captured_item == item)
                .map(|(_, slot)| slot.clone())
                .expect("captured keyed slot");
            slot.replace(value);
        }
    }

    order.replace(vec![3, 1, 2]);
    render();

    let slots = captured.borrow();
    for (item, expected) in [(3, 303), (1, 101), (2, 202)] {
        let slot = slots
            .iter()
            .find(|(captured_item, _)| *captured_item == item)
            .map(|(_, slot)| slot.clone())
            .expect("captured reordered keyed slot");
        assert_eq!(
            slot.with(|value| *value),
            expected,
            "explicit keys must preserve remembered state for item {item}",
        );
    }
}

#[test]
fn list_item_reorder_without_explicit_keys_follows_positional_identity() {
    const ITEM_GROUP_KEY: Key = 0x51A7;

    let mut composition = test_composition();
    let root_key = location_key(file!(), line!(), column!());
    let order = Rc::new(RefCell::new(vec![1_i32, 2, 3]));
    let captured = Rc::new(RefCell::new(Vec::<(i32, Owned<i32>)>::new()));

    let mut render = || {
        let order = Rc::clone(&order);
        let captured = Rc::clone(&captured);
        composition
            .render(root_key, || {
                captured.replace(Vec::new());
                let current_order = order.borrow().clone();
                with_current_composer(|composer| {
                    for item in current_order {
                        composer.with_group(ITEM_GROUP_KEY, |composer| {
                            let slot = composer.remember(|| item * 10);
                            captured.borrow_mut().push((item, slot));
                        });
                    }
                });
            })
            .expect("render unkeyed list");
        assert_composition_valid(&composition);
    };

    render();
    {
        let slots = captured.borrow();
        for (item, value) in [(1, 101), (2, 202), (3, 303)] {
            let slot = slots
                .iter()
                .find(|(captured_item, _)| *captured_item == item)
                .map(|(_, slot)| slot.clone())
                .expect("captured unkeyed slot");
            slot.replace(value);
        }
    }

    order.replace(vec![3, 1, 2]);
    render();

    let slots = captured.borrow();
    for (item, expected) in [(3, 101), (1, 202), (2, 303)] {
        let slot = slots
            .iter()
            .find(|(captured_item, _)| *captured_item == item)
            .map(|(_, slot)| slot.clone())
            .expect("captured reordered unkeyed slot");
        assert_eq!(
            slot.with(|value| *value),
            expected,
            "without explicit keys, remembered state must follow position for item {item}",
        );
    }
}

#[test]
fn subcompose_in_retains_root_level_groups() {
    let (handle, _runtime) = runtime_handle();
    let mut slots = SlotTable::default();
    let mut applier = test_applier();
    let (composer, slots_host, applier_host) =
        setup_composer(&mut slots, &mut applier, handle.clone(), None);
    let subcompose_slots = Rc::new(SlotsHost::new(SlotTable::new()));
    let remembered = Rc::new(RefCell::new(None::<Owned<i32>>));
    let group_key = location_key(file!(), line!(), column!());

    let render = |show_branch: bool| {
        let remembered = Rc::clone(&remembered);
        composer
            .subcompose_in(&subcompose_slots, None, |composer| {
                if show_branch {
                    composer.cranpose_with_reuse(
                        group_key,
                        RecomposeOptions::default(),
                        |composer| {
                            let slot = composer.remember(|| 7_i32);
                            remembered.replace(Some(slot));
                        },
                    );
                }
            })
            .expect("subcompose_in render");
    };

    render(true);
    let slot = remembered
        .borrow()
        .clone()
        .expect("captured retained root-level slot");
    slot.replace(41);

    render(false);
    let hidden_snapshot = subcompose_slots.debug_snapshot();
    assert_eq!(hidden_snapshot.retained_subtree_count, 1);
    assert_eq!(hidden_snapshot.retained_scope_count, 1);

    render(true);
    let restored = remembered
        .borrow()
        .clone()
        .expect("restored retained root-level slot");
    assert_eq!(
        restored.with(|value| *value),
        41,
        "root-level retained groups in subcompose_in must restore remembered state"
    );
    assert_eq!(subcompose_slots.debug_snapshot().retained_subtree_count, 0);

    drop(composer);
    teardown_composer(&mut slots, &mut applier, slots_host, applier_host);
}

#[test]
fn subcompose_slot_root_level_scopes_keep_container_parent_hint() {
    let (handle, _runtime) = runtime_handle();
    let mut slots = SlotTable::default();
    let mut applier = test_applier();
    let container_id = applier.create(Box::<RecordingNode>::default());
    let (composer, slots_host, applier_host) =
        setup_composer(&mut slots, &mut applier, handle, None);
    let subcompose_slots = Rc::new(SlotsHost::new(SlotTable::new()));
    let group_key = location_key(file!(), line!(), column!());

    let (_, scopes) = composer
        .subcompose_slot(&subcompose_slots, Some(container_id), |composer| {
            composer.with_group(group_key, |_| {
                cranpose_test_node(TrackingChild::default);
            });
        })
        .expect("subcompose_slot render");

    assert!(
        !scopes.is_empty(),
        "subcompose_slot must record scopes for later scoped recomposition"
    );
    for scope in &scopes {
        assert_eq!(
            scope.parent_hint(),
            Some(container_id),
            "top-level subcompose scopes must reattach through their real container",
        );
    }

    drop(composer);
    teardown_composer(&mut slots, &mut applier, slots_host, applier_host);
}

#[test]
fn secondary_host_reset_deactivates_all_owned_scopes() {
    let (handle, _runtime) = runtime_handle();
    let mut slots = SlotTable::default();
    let mut applier = test_applier();
    let (composer, slots_host, applier_host) =
        setup_composer(&mut slots, &mut applier, handle, None);
    let secondary_host = Rc::new(SlotsHost::new(SlotTable::new()));
    let child_key = location_key(file!(), line!(), column!());

    let (_, scopes) = composer
        .subcompose_slot(&secondary_host, None, |composer| {
            composer.with_group(child_key, |_| {});
        })
        .expect("subcompose secondary host");
    assert!(!scopes.is_empty());
    for scope in &scopes {
        assert!(scope.is_active());
        scope.invalidate();
    }

    secondary_host.reset().expect("reset secondary host");

    for scope in &scopes {
        assert!(
            !scope.is_active(),
            "host reset must deactivate scopes that survive through queued references",
        );
    }

    drop(composer);
    drop(secondary_host);
    teardown_composer(&mut slots, &mut applier, slots_host, applier_host);
}

#[test]
fn subcompose_in_retained_root_level_nodes_stay_live_while_hidden_and_restore_same_id() {
    let (handle, _runtime) = runtime_handle();
    let mut slots = SlotTable::default();
    let mut applier = test_applier();
    let (composer, slots_host, applier_host) =
        setup_composer(&mut slots, &mut applier, handle.clone(), None);
    let subcompose_slots = Rc::new(SlotsHost::new(SlotTable::new()));
    let captured = Rc::new(RefCell::new(Vec::<NodeId>::new()));
    let group_key = location_key(file!(), line!(), column!());

    let render = |show_branch: bool| {
        let captured = Rc::clone(&captured);
        composer
            .subcompose_in(&subcompose_slots, None, |composer| {
                if show_branch {
                    composer.cranpose_with_reuse(
                        group_key,
                        RecomposeOptions::default(),
                        move |_composer| {
                            let node_id = cranpose_test_node(TrackingChild::default);
                            captured.borrow_mut().push(node_id);
                        },
                    );
                }
            })
            .expect("subcompose_in retained root node render");
    };

    render(true);
    let first = *captured
        .borrow()
        .last()
        .expect("captured retained root-level node");

    render(false);
    let hidden_snapshot = subcompose_slots.debug_snapshot();
    assert_eq!(hidden_snapshot.retained_subtree_count, 1);
    assert_eq!(hidden_snapshot.retained_scope_count, 1);
    {
        let mut applier = applier_host.borrow_typed();
        let hidden_parent = applier
            .with_node::<TrackingChild, _>(first, |node| node.parent())
            .expect("hidden retained root-level node must stay live");
        assert_eq!(
            hidden_parent, None,
            "root-level retained nodes must stay detached while hidden",
        );
    }

    render(true);
    let restored = *captured
        .borrow()
        .last()
        .expect("restored retained root-level node");
    assert_eq!(
        restored, first,
        "root-level retained groups in subcompose_in must restore the same node id",
    );
    assert_eq!(subcompose_slots.debug_snapshot().retained_subtree_count, 0);

    drop(composer);
    teardown_composer(&mut slots, &mut applier, slots_host, applier_host);
}

#[test]
fn dropping_subcompose_host_disposes_hidden_retained_nodes() {
    let (handle, _runtime) = runtime_handle();
    let mut slots = SlotTable::default();
    let mut applier = test_applier();
    let (composer, slots_host, applier_host) =
        setup_composer(&mut slots, &mut applier, handle.clone(), None);
    let subcompose_slots = Rc::new(SlotsHost::new(SlotTable::new()));
    let captured = Rc::new(RefCell::new(Vec::<NodeId>::new()));
    let group_key = location_key(file!(), line!(), column!());

    let render = |show_branch: bool| {
        let captured = Rc::clone(&captured);
        composer
            .subcompose_in(&subcompose_slots, None, |composer| {
                if show_branch {
                    composer.cranpose_with_reuse(
                        group_key,
                        RecomposeOptions::default(),
                        move |_composer| {
                            let node_id = cranpose_test_node(TrackingChild::default);
                            captured.borrow_mut().push(node_id);
                        },
                    );
                }
            })
            .expect("subcompose_in retained root node render");
    };

    render(true);
    let retained_node = *captured
        .borrow()
        .last()
        .expect("captured retained root-level node");

    render(false);
    let hidden_snapshot = subcompose_slots.debug_snapshot();
    assert_eq!(hidden_snapshot.retained_subtree_count, 1);
    assert_eq!(hidden_snapshot.retained_scope_count, 1);
    {
        let mut applier = applier_host.borrow_typed();
        let hidden_parent = applier
            .with_node::<TrackingChild, _>(retained_node, |node| node.parent())
            .expect("hidden retained root-level node must stay live");
        assert_eq!(
            hidden_parent, None,
            "hidden retained root-level node must stay detached before host disposal",
        );
    }

    drop(subcompose_slots);
    {
        let mut applier = applier_host.borrow_typed();
        assert!(
            matches!(
                applier.get_mut(retained_node),
                Err(NodeError::Missing { .. })
            ),
            "dropping a SlotsHost must dispose retained hidden nodes instead of leaking them",
        );
    }

    drop(composer);
    teardown_composer(&mut slots, &mut applier, slots_host, applier_host);
}

#[test]
fn with_key_keeps_callsite_identity_when_user_key_repeats() {
    thread_local! {
        static SECOND_SLOT: RefCell<Option<Owned<i32>>> = const { RefCell::new(None) };
    }

    let mut composition = test_composition();
    let runtime = composition.runtime_handle();
    let show_first = MutableState::with_runtime(true, runtime.clone());
    let root_key = location_key(file!(), line!(), column!());

    #[composable]
    fn root(show_first: MutableState<bool>) {
        if show_first.value() {
            cranpose_core::with_key(&"shared-user-key", || {
                let slot = with_current_composer(|composer| composer.remember(|| 10));
                let _ = slot;
            });
        }

        cranpose_core::with_key(&"shared-user-key", || {
            let slot = with_current_composer(|composer| composer.remember(|| 20));
            SECOND_SLOT.with(|cell| cell.replace(Some(slot)));
        });
    }

    let pass = |composition: &mut Composition<MemoryApplier>| {
        composition.render(root_key, || root(show_first))
    };
    pass(&mut composition).expect("initial keyed composition");

    let second = SECOND_SLOT
        .with(|cell| cell.borrow().clone())
        .expect("second keyed slot captured");
    second.replace(99);

    show_first.set_value(false);
    pass(&mut composition).expect("hide first keyed branch");

    let second_after = SECOND_SLOT
        .with(|cell| cell.borrow().clone())
        .expect("second keyed slot recaptured");
    assert_eq!(
        second_after.with(|value| *value),
        99,
        "keyed identity must include the callsite so later siblings do not inherit earlier state",
    );
}

#[test]
fn retained_siblings_with_same_raw_key_keep_distinct_state() {
    thread_local! {
        static CAPTURED: RefCell<Vec<Owned<i32>>> = const { RefCell::new(Vec::new()) };
    }

    const SHARED_KEY: Key = 0xCAFE_BABE;

    let mut composition = test_composition();
    let runtime = composition.runtime_handle();
    let show_children = MutableState::with_runtime(true, runtime.clone());
    let root_key = location_key(file!(), line!(), column!());

    #[composable]
    fn root(show_children: MutableState<bool>) {
        CAPTURED.with(|slots| slots.borrow_mut().clear());
        if !show_children.value() {
            return;
        }

        with_current_composer(|composer| {
            for initial in [10, 20] {
                composer.cranpose_with_reuse(SHARED_KEY, RecomposeOptions::default(), |composer| {
                    let slot = composer.remember(|| initial);
                    CAPTURED.with(|slots| slots.borrow_mut().push(slot));
                });
            }
        });
    }

    let pass = |composition: &mut Composition<MemoryApplier>| {
        composition.render(root_key, || root(show_children))
    };
    pass(&mut composition).expect("initial retained sibling composition");

    CAPTURED.with(|slots| {
        let slots = slots.borrow();
        assert_eq!(slots.len(), 2, "expected two retained siblings");
        slots[0].replace(101);
        slots[1].replace(202);
    });

    show_children.set_value(false);
    pass(&mut composition).expect("hide retained siblings");

    show_children.set_value(true);
    pass(&mut composition).expect("restore retained siblings");

    let restored = CAPTURED.with(|slots| {
        slots
            .borrow()
            .iter()
            .map(|slot| slot.with(|value| *value))
            .collect::<Vec<_>>()
    });
    assert_eq!(
        restored,
        vec![101, 202],
        "retention must key preserved siblings by full structural identity, not raw key alone",
    );
}

#[test]
fn keyed_child_moved_between_parents_rebuilds_without_stale_attachment() {
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
    let child_parent = applier
        .with_node::<TrackingChild, _>(current_child_id, |node| node.parent())
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
        "the moved child must attach under the new parent; tree=\n{tree}",
    );
    assert_eq!(
        root_children,
        vec![parent_id, current_child_id],
        "root should now own the moved child directly; tree=\n{tree}",
    );
    assert!(
        parent_children.is_empty(),
        "old parent must release the moved child when it changes parents; tree=\n{tree}",
    );
    if current_child_id != child_id {
        assert!(
            applier.get_mut(child_id).is_err(),
            "without explicit retention, the old child node must be disposed when rebuilt; tree=\n{tree}",
        );
    }
}

#[test]
fn conditional_nested_child_recompose_keeps_parent_order() {
    thread_local! {
        static ROOT_ID: Cell<Option<NodeId>> = const { Cell::new(None) };
        static QUEUE_ROW_ID: Cell<Option<NodeId>> = const { Cell::new(None) };
        static CURRENT_ROW_ID: Cell<Option<NodeId>> = const { Cell::new(None) };
        static STATUS_ROW_ID: Cell<Option<NodeId>> = const { Cell::new(None) };
        static FIELD_ID: Cell<Option<NodeId>> = const { Cell::new(None) };
    }

    #[allow(non_snake_case)]
    #[composable]
    fn Leaf(label: &'static str) -> NodeId {
        cranpose_core::with_current_composer(|composer| {
            composer.emit_node(|| TrackingChild {
                label: label.to_string(),
                ..TrackingChild::default()
            })
        })
    }

    #[allow(non_snake_case)]
    #[composable]
    fn QueueRow() -> NodeId {
        let id = cranpose_core::with_current_composer(|composer| {
            composer.emit_node(RecordingNode::default)
        });
        QUEUE_ROW_ID.with(|slot| slot.set(Some(id)));
        cranpose_core::push_parent(id);
        Leaf("queue");
        cranpose_core::pop_parent();
        id
    }

    #[allow(non_snake_case)]
    #[composable]
    fn CurrentField(field_text: MutableState<&'static str>) -> NodeId {
        let _ = field_text.value();
        let id = Leaf("field");
        FIELD_ID.with(|slot| slot.set(Some(id)));
        id
    }

    #[allow(non_snake_case)]
    #[composable]
    fn CurrentRow(field_text: MutableState<&'static str>) -> NodeId {
        let id = cranpose_core::with_current_composer(|composer| {
            composer.emit_node(RecordingNode::default)
        });
        CURRENT_ROW_ID.with(|slot| slot.set(Some(id)));
        cranpose_core::push_parent(id);
        CurrentField(field_text);
        cranpose_core::pop_parent();
        id
    }

    #[allow(non_snake_case)]
    #[composable]
    fn StatusRow() -> NodeId {
        let id = cranpose_core::with_current_composer(|composer| {
            composer.emit_node(RecordingNode::default)
        });
        STATUS_ROW_ID.with(|slot| slot.set(Some(id)));
        cranpose_core::push_parent(id);
        Leaf("status");
        cranpose_core::pop_parent();
        id
    }

    #[allow(non_snake_case)]
    #[composable]
    fn Root(show_current: MutableState<bool>, field_text: MutableState<&'static str>) -> NodeId {
        let show_current = show_current.value();
        let id = cranpose_core::with_current_composer(|composer| {
            composer.emit_node(RecordingNode::default)
        });
        ROOT_ID.with(|slot| slot.set(Some(id)));
        cranpose_core::push_parent(id);
        QueueRow();
        if show_current {
            CurrentRow(field_text);
        }
        StatusRow();
        cranpose_core::pop_parent();
        id
    }

    fn drain(composition: &mut Composition<MemoryApplier>, context: &'static str) {
        while composition.process_invalid_scopes().expect(context) {}
    }

    fn captured_id(slot: &'static std::thread::LocalKey<Cell<Option<NodeId>>>) -> NodeId {
        slot.with(|cell| cell.get())
            .expect("node id should be captured")
    }

    fn assert_row_order(
        composition: &mut Composition<MemoryApplier>,
        root_id: NodeId,
        queue_id: NodeId,
        current_id: NodeId,
        status_id: NodeId,
        field_id: NodeId,
    ) {
        let mut applier = composition.applier_mut();
        let tree = applier.dump_tree(Some(root_id));
        let root_children = applier
            .with_node::<RecordingNode, _>(root_id, |node| node.children.clone())
            .expect("root node should exist");
        let current_children = applier
            .with_node::<RecordingNode, _>(current_id, |node| node.children.clone())
            .expect("current row should exist");
        let field_parent = applier
            .with_node::<TrackingChild, _>(field_id, |node| node.parent())
            .expect("field node should exist");

        assert_eq!(
            root_children,
            vec![queue_id, current_id, status_id],
            "conditional row must stay before the status sibling; tree=\n{tree}",
        );
        assert_eq!(
            current_children,
            vec![field_id],
            "field node must remain inside the conditional row; tree=\n{tree}",
        );
        assert_eq!(
            field_parent,
            Some(current_id),
            "field parent link must stay on the conditional row; tree=\n{tree}",
        );
    }

    let mut composition = test_composition();
    let runtime = composition.runtime_handle();
    let show_current = MutableState::with_runtime(false, runtime.clone());
    let field_text = MutableState::with_runtime("title", runtime);
    let root_key = location_key(file!(), line!(), column!());

    composition
        .render(root_key, || {
            Root(show_current, field_text);
        })
        .expect("initial render");

    show_current.set(true);
    drain(&mut composition, "insert current row");

    let root_id = captured_id(&ROOT_ID);
    let queue_id = captured_id(&QUEUE_ROW_ID);
    let current_id = captured_id(&CURRENT_ROW_ID);
    let status_id = captured_id(&STATUS_ROW_ID);
    let field_id = captured_id(&FIELD_ID);
    assert_row_order(
        &mut composition,
        root_id,
        queue_id,
        current_id,
        status_id,
        field_id,
    );

    field_text.set("edited");
    drain(&mut composition, "edit field text");

    assert_row_order(
        &mut composition,
        root_id,
        queue_id,
        current_id,
        status_id,
        field_id,
    );
}

#[test]
fn scoped_recompose_after_root_replay_does_not_self_parent_root() {
    thread_local! {
        static ROOT_ID: Cell<Option<NodeId>> = const { Cell::new(None) };
    }

    #[derive(Default)]
    struct RootReplayNode {
        id: Option<NodeId>,
        raw_parent: Option<NodeId>,
        children: Vec<NodeId>,
    }

    impl Node for RootReplayNode {
        fn set_node_id(&mut self, id: NodeId) {
            self.id = Some(id);
        }

        fn parent(&self) -> Option<NodeId> {
            match (self.raw_parent, self.id) {
                (Some(parent), Some(id)) if parent == id => None,
                (parent, _) => parent,
            }
        }

        fn on_attached_to_parent(&mut self, parent: NodeId) {
            self.raw_parent = Some(parent);
        }

        fn on_removed_from_parent(&mut self) {
            self.raw_parent = None;
        }

        fn insert_child(&mut self, child: NodeId) {
            if !self.children.contains(&child) {
                self.children.push(child);
            }
        }

        fn remove_child(&mut self, child: NodeId) {
            self.children.retain(|&id| id != child);
        }

        fn children(&self) -> Vec<NodeId> {
            self.children.clone()
        }
    }

    #[allow(non_snake_case)]
    #[composable]
    fn Child(value: i32) -> NodeId {
        cranpose_core::with_current_composer(|composer| {
            composer.emit_node(|| TrackingChild {
                label: value.to_string(),
                ..TrackingChild::default()
            })
        })
    }

    #[allow(non_snake_case)]
    #[composable]
    fn Root(value: MutableState<i32>) -> NodeId {
        let value = value.value();
        let id = cranpose_core::with_current_composer(|composer| {
            composer.emit_node(RootReplayNode::default)
        });
        ROOT_ID.with(|slot| slot.set(Some(id)));
        cranpose_core::push_parent(id);
        Child(value);
        cranpose_core::pop_parent();
        id
    }

    fn root_raw_parent(
        composition: &mut Composition<MemoryApplier>,
        root_id: NodeId,
    ) -> Option<NodeId> {
        composition
            .applier_mut()
            .with_node::<RootReplayNode, _>(root_id, |node| node.raw_parent)
            .expect("root node should exist")
    }

    let mut composition = test_composition();
    let runtime = composition.runtime_handle();
    let value = MutableState::with_runtime(0, runtime);
    let root_key = location_key(file!(), line!(), column!());

    let mut content = || {
        Root(value);
    };
    composition
        .render(root_key, &mut content)
        .expect("initial render");
    let root_id = ROOT_ID.with(|slot| slot.get()).expect("root id");

    composition.request_root_render();
    composition
        .reconcile(root_key, &mut content)
        .expect("forced root replay");
    assert_eq!(root_raw_parent(&mut composition, root_id), None);

    value.set(1);
    while composition
        .process_invalid_scopes()
        .expect("scoped root recomposition")
    {}

    assert_eq!(
        root_raw_parent(&mut composition, root_id),
        None,
        "a root scope replay must not attach its root node as its own child",
    );
}

#[test]
fn a_surviving_keyed_remember_keeps_its_slot_when_a_same_statement_neighbor_leaves() {
    thread_local! {
        static KEYED_INITS: Cell<usize> = const { Cell::new(0) };
        static KEYED_SEEN: Cell<i32> = const { Cell::new(0) };
    }

    fn keyed_through_helper(key: u32) -> i32 {
        rememberKeyed(key, |key| {
            KEYED_INITS.with(|count| count.set(count.get() + 1));
            100 + *key as i32
        })
    }

    #[composable]
    fn keyed_pair(with_first: bool) {
        let (_first, second) = (
            with_first.then_some(1u32).map(keyed_through_helper),
            rememberKeyed(9u32, |_| {
                KEYED_INITS.with(|count| count.set(count.get() + 1));
                200
            }),
        );
        KEYED_SEEN.with(|slot| slot.set(second));
    }

    let mut composition = test_composition();
    let pass = |composition: &mut Composition<MemoryApplier>, with_first: bool| {
        composition.render(861, || keyed_pair(with_first))
    };
    pass(&mut composition, true).expect("initial composition");
    assert_eq!(KEYED_INITS.with(Cell::get), 2);
    assert_eq!(KEYED_SEEN.with(Cell::get), 200);

    pass(&mut composition, false).expect("drop the first keyed remember");
    assert_eq!(
        (KEYED_INITS.with(Cell::get), KEYED_SEEN.with(Cell::get)),
        (2, 200),
        "the surviving call's key did not change, so its initializer must not rerun; \
         adopting the vanished neighbor's slot forces a spurious key mismatch"
    );
}

#[test]
fn a_surviving_coroutine_scope_keeps_its_identity_when_a_neighbor_leaves() {
    thread_local! {
        static SECOND_SCOPE: Cell<usize> = const { Cell::new(0) };
    }

    fn scope_through_helper(_slot: u32) -> CoroutineScope {
        rememberCoroutineScope()
    }

    #[composable]
    fn scope_pair(with_first: bool) {
        let (_first, second) = (
            with_first.then_some(1u32).map(scope_through_helper),
            rememberCoroutineScope(),
        );
        SECOND_SCOPE.with(|slot| slot.set(second.probe_identity()));
    }

    let mut composition = test_composition();
    let pass = |composition: &mut Composition<MemoryApplier>, with_first: bool| {
        composition.render(862, || scope_pair(with_first))
    };
    pass(&mut composition, true).expect("initial composition");
    let original = SECOND_SCOPE.with(Cell::get);

    pass(&mut composition, false).expect("drop the first scope");
    assert_eq!(
        SECOND_SCOPE.with(Cell::get),
        original,
        "the surviving call must keep its own scope; adopting the vanished \
         neighbor's scope silently rebinds every task the survivor launched"
    );
}

#[test]
fn a_surviving_provider_keeps_its_entry_when_a_same_typed_neighbor_leaves() {
    thread_local! {
        static SECOND_READ: Cell<i32> = const { Cell::new(0) };
    }

    let first = compositionLocalOf(|| 0);
    let second = compositionLocalOf(|| 0);
    let mut composition = test_composition();
    let runtime = composition.runtime_handle();
    let with_first = MutableState::with_runtime(true, runtime.clone());
    let second_value = MutableState::with_runtime(20, runtime.clone());

    #[composable]
    fn reader(second: CompositionLocal<i32>) {
        let value = second.current();
        SECOND_READ.with(|slot| slot.set(value));
    }

    #[composable]
    fn tree(
        first: CompositionLocal<i32>,
        second: CompositionLocal<i32>,
        with_first: MutableState<bool>,
        second_value: MutableState<i32>,
    ) {
        let mut provided = Vec::new();
        if with_first.value() {
            provided.push(first.provides(10));
        }
        provided.push(second.provides(second_value.value()));
        CompositionLocalProvider(provided, || reader(second.clone()));
    }

    composition
        .render(863, || {
            tree(first.clone(), second.clone(), with_first, second_value)
        })
        .expect("initial composition");
    assert_eq!(SECOND_READ.with(Cell::get), 20);

    with_first.set_value(false);
    while composition
        .process_invalid_scopes()
        .expect("drop the first provider")
    {}

    second_value.set_value(30);
    while composition
        .process_invalid_scopes()
        .expect("change the surviving provider's value")
    {}
    assert_eq!(
        SECOND_READ.with(Cell::get),
        30,
        "the surviving provider must keep its own entry; adopting the vanished \
         neighbor's entry orphans the reader's subscription"
    );
}
