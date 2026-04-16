use super::*;

#[test]
#[should_panic(expected = "subcompose() may only be called during measure or layout")]
fn subcompose_panics_outside_measure_or_layout() {
    let (handle, _runtime) = runtime_handle();
    let mut slots = SlotTable::default();
    let mut applier = MemoryApplier::new();
    let (composer, slots_host, applier_host) =
        setup_composer(&mut slots, &mut applier, handle, None);
    let mut state = SubcomposeState::default();
    let _ = composer.subcompose(&mut state, SlotId::new(1), |_| {});
    drop(composer);
    teardown_composer(&mut slots, &mut applier, slots_host, applier_host);
}

#[test]
fn subcompose_reuses_nodes_across_calls() {
    let (handle, _runtime) = runtime_handle();
    let mut slots = SlotTable::default();
    let mut applier = MemoryApplier::new();
    let mut state = SubcomposeState::default();
    let first_id;

    {
        let (composer, slots_host, applier_host) =
            setup_composer(&mut slots, &mut applier, handle.clone(), None);
        composer.set_phase(Phase::Measure);
        let (_, first_nodes) = composer.subcompose(&mut state, SlotId::new(7), |composer| {
            composer.emit_node(|| TestDummyNode)
        });
        assert_eq!(first_nodes.len(), 1);
        first_id = first_nodes[0];
        drop(composer);
        teardown_composer(&mut slots, &mut applier, slots_host, applier_host);
    }

    slots.reset();

    {
        let (composer, slots_host, applier_host) =
            setup_composer(&mut slots, &mut applier, handle.clone(), None);
        composer.set_phase(Phase::Measure);
        let (_, second_nodes) = composer.subcompose(&mut state, SlotId::new(7), |composer| {
            composer.emit_node(|| TestDummyNode)
        });
        assert_eq!(second_nodes.len(), 1);
        assert_eq!(second_nodes[0], first_id);
        drop(composer);
        teardown_composer(&mut slots, &mut applier, slots_host, applier_host);
    }
}

#[test]
fn apply_pending_commands_makes_subcomposed_nodes_available() {
    let (handle, _runtime) = runtime_handle();
    let mut slots = SlotTable::default();
    let mut applier = MemoryApplier::new();
    let mut state = SubcomposeState::default();
    let mounted = Rc::new(Cell::new(0));

    let (composer, slots_host, applier_host) =
        setup_composer(&mut slots, &mut applier, handle, None);
    composer.set_phase(Phase::Measure);

    let (_, nodes) = composer.subcompose(&mut state, SlotId::new(1), |composer| {
        let mounted = Rc::clone(&mounted);
        composer.emit_node(|| MountTrackingNode { mounted })
    });
    assert_eq!(nodes.len(), 1);
    assert_eq!(mounted.get(), 0);

    composer
        .apply_pending_commands()
        .expect("apply_pending_commands failed");

    assert_eq!(mounted.get(), 1);

    drop(composer);
    teardown_composer(&mut slots, &mut applier, slots_host, applier_host);
}

#[test]
fn with_slot_value_reads_and_updates() {
    let (handle, _runtime) = runtime_handle();
    let mut slots = SlotTable::default();
    let mut applier = MemoryApplier::new();
    let (composer, slots_host, applier_host) =
        setup_composer(&mut slots, &mut applier, handle, None);
    let key = location_key(file!(), line!(), column!());

    composer.with_group(key, |composer| {
        let slot_id = composer.use_value_slot(|| 10i32);
        let initial = composer.with_slot_value::<i32, _>(slot_id, |value| *value);
        assert_eq!(initial, 10);
        composer.with_slot_value_mut::<i32, _>(slot_id, |value| {
            *value = 42;
        });
        let updated = composer.with_slot_value::<i32, _>(slot_id, |value| *value);
        assert_eq!(updated, 42);
    });

    drop(composer);
    teardown_composer(&mut slots, &mut applier, slots_host, applier_host);
}

#[test]
fn mutable_state_exposes_pending_value_while_borrowed() {
    let (runtime_handle, _runtime) = runtime_handle();
    let state = MutableState::with_runtime(0, runtime_handle);
    let observed = Cell::new(0);

    state.with(|value| {
        assert_eq!(*value, 0);
        state.set(1);
        observed.set(state.get());
    });

    assert_eq!(observed.get(), 1);
    state.with(|value| assert_eq!(*value, 1));
}

#[test]
fn mutable_state_reads_during_update_return_previous_value() {
    let (runtime_handle, _runtime) = runtime_handle();
    let state = MutableState::with_runtime(0, runtime_handle);
    let before = Cell::new(-1);
    let after = Cell::new(-1);

    state.update(|value| {
        before.set(state.get());
        *value = 7;
        after.set(state.get());
    });

    assert_eq!(before.get(), 0);
    assert_eq!(after.get(), 0);
    assert_eq!(state.get(), 7);
}

#[test]
fn snapshot_state_list_basic_operations() {
    let (runtime_handle, _runtime) = runtime_handle();
    let list = SnapshotStateList::with_runtime([1, 2], runtime_handle.clone());

    assert_eq!(list.len(), 2);
    assert_eq!(list.first(), Some(1));
    assert_eq!(list.get(1), 2);

    list.push(3);
    list.insert(1, 9);
    assert_eq!(list.to_vec(), vec![1, 9, 2, 3]);

    let previous = list.set(2, 7);
    assert_eq!(previous, 2);
    assert_eq!(list.to_vec(), vec![1, 9, 7, 3]);

    let removed = list.remove(1);
    assert_eq!(removed, 9);
    assert_eq!(list.to_vec(), vec![1, 7, 3]);

    let popped = list.pop();
    assert_eq!(popped, Some(3));

    list.extend([4, 5]);
    assert_eq!(list.to_vec(), vec![1, 7, 4, 5]);

    list.retain(|value| *value % 2 == 1);
    assert_eq!(list.to_vec(), vec![1, 7, 5]);

    list.clear();
    assert!(list.is_empty());
}

#[test]
fn snapshot_state_list_commits_snapshot_mutations() {
    let _guard = reset_snapshot_runtime();
    let (runtime_handle, _runtime) = runtime_handle();
    let list = SnapshotStateList::with_runtime([10], runtime_handle.clone());

    let snapshot = take_mutable_snapshot(None, None);
    snapshot.enter(|| {
        list.insert(0, 5);
        list.push(15);
    });
    snapshot.apply().check();

    assert_eq!(list.to_vec(), vec![5, 10, 15]);
}

#[test]
fn snapshot_state_map_basic_operations() {
    let (runtime_handle, _runtime) = runtime_handle();
    let map = SnapshotStateMap::with_runtime([(1, 10), (2, 20)], runtime_handle.clone());

    assert_eq!(map.len(), 2);
    assert!(map.contains_key(&1));
    assert_eq!(map.get(&2), Some(20));

    let previous = map.insert(2, 25);
    assert_eq!(previous, Some(20));
    assert_eq!(map.get(&2), Some(25));

    map.extend([(3, 30)]);
    assert_eq!(map.to_hash_map().get(&3), Some(&30));

    let removed = map.remove(&1);
    assert_eq!(removed, Some(10));
    assert!(!map.contains_key(&1));

    map.retain(|_, value| {
        *value += 1;
        *value % 2 == 0
    });
    let snapshot = map.to_hash_map();
    assert_eq!(snapshot.len(), 1);
    assert_eq!(snapshot.get(&2), Some(&26));

    map.clear();
    assert!(map.is_empty());
}

#[test]
fn snapshot_state_map_commits_snapshot_mutations() {
    let _guard = reset_snapshot_runtime();
    let (runtime_handle, _runtime) = runtime_handle();
    let map = SnapshotStateMap::with_runtime([(1, 1)], runtime_handle.clone());

    let snapshot = take_mutable_snapshot(None, None);
    snapshot.enter(|| {
        map.insert(2, 2);
        map.insert(1, 3);
    });
    snapshot.apply().check();

    let snapshot = map.to_hash_map();
    assert_eq!(snapshot.len(), 2);
    assert_eq!(snapshot.get(&1), Some(&3));
    assert_eq!(snapshot.get(&2), Some(&2));
}

#[test]
fn mutable_state_snapshot_handles_reentrant_drop_reads() {
    let (runtime_handle, _runtime) = runtime_handle();
    let drops = Rc::new(Cell::new(0));
    let state = MutableState::with_runtime(
        ReentrantDropState::new(0, Rc::clone(&drops), true),
        runtime_handle,
    );

    DROP_REENTRY_STATE.with(|slot| {
        *slot.borrow_mut() = Some(state);
    });
    DROP_REENTRY_LAST_VALUE.with(|last| last.set(None));

    state.update(|_| {
        state.set(ReentrantDropState::new(1, Rc::clone(&drops), false));
    });

    let current = state.value();
    assert_eq!(current.id, 1);
    drop(current);

    DROP_REENTRY_STATE.with(|slot| {
        slot.borrow_mut().take();
    });

    assert!(drops.get() >= 1);
    DROP_REENTRY_LAST_VALUE.with(|last| {
        assert_eq!(last.get(), Some(1));
    });
}

#[test]
fn state_arena_reuses_slot_after_last_owner_drops() {
    let (handle, _runtime) = runtime_handle();
    let first = OwnedMutableState::with_runtime(1i32, handle.clone());
    let first_handle = first.handle();
    let first_id = first_handle.state_id_for_test();

    assert_eq!(handle.state_arena_stats(), (1, 0));

    let _: MutableState<i32> = first_handle;
    assert_eq!(handle.state_arena_stats(), (1, 0));
    drop(first);
    assert_eq!(handle.state_arena_stats(), (1, 1));

    let second = OwnedMutableState::with_runtime(2i32, handle.clone());
    let second_id = second.handle().state_id_for_test();

    assert_eq!(second.value(), 2);
    assert_eq!(second_id.slot(), first_id.slot());
    assert_ne!(second_id, first_id);
    assert_eq!(handle.state_arena_stats(), (1, 0));
}

#[test]
fn stale_state_observer_task_does_not_invalidate_reused_slot() {
    let runtime = TestRuntime::new();
    let handle = runtime.handle();
    let stale = OwnedMutableState::with_runtime(0i32, handle.clone());
    let stale_handle = stale.handle();
    let stale_id = stale_handle.state_id_for_test();

    stale_handle.set_value(1);
    drop(stale);
    assert_eq!(handle.state_arena_stats(), (1, 1));

    let fresh = OwnedMutableState::with_runtime(2i32, handle.clone());
    let fresh_handle = fresh.handle();
    let fresh_id = fresh_handle.state_id_for_test();
    let scope = RecomposeScope::new_for_test(handle.clone());

    assert_eq!(fresh_id.slot(), stale_id.slot());
    assert_ne!(fresh_id, stale_id);

    fresh_handle.subscribe_scope_for_test(&scope);
    assert!(!scope.is_invalid());

    handle.drain_ui();

    assert!(!scope.is_invalid());
    assert_eq!(fresh_handle.watcher_count(), 1);
}

#[test]
fn persistent_state_handles_stay_valid_without_explicit_owner() {
    let (handle, _runtime) = runtime_handle();
    let state = MutableState::with_runtime(7i32, handle.clone());
    let _: MutableState<i32> = state;

    assert_eq!(state.value(), 7);
    assert_eq!(handle.state_arena_stats(), (1, 0));

    let retained = state.retain();
    assert_eq!(retained.value(), 7);
    drop(retained);
    assert_eq!(handle.state_arena_stats(), (1, 0));
}

#[test]
fn runtime_registry_stays_alive_until_last_runtime_clone_drops() {
    let runtime = Runtime::new(Arc::new(TestScheduler));
    let runtime_clone = runtime.clone();
    let handle = runtime.handle();
    let state = MutableState::with_runtime(11i32, handle.clone());

    drop(runtime);

    assert_eq!(state.value(), 11);
    assert!(crate::runtime::runtime_handle_by_id(handle.id()).is_some());

    drop(runtime_clone);

    assert!(crate::runtime::runtime_handle_by_id(handle.id()).is_none());
}

#[test]
fn disposable_effect_cleanup_can_update_use_state_during_group_removal() {
    let mut composition = Composition::new(MemoryApplier::new());
    let runtime = composition.runtime_handle();
    let show = MutableState::with_runtime(true, runtime);
    let cleanup_calls = Rc::new(Cell::new(0usize));
    let key = location_key(file!(), line!(), column!());

    let render = |composition: &mut Composition<MemoryApplier>| {
        let cleanup_calls = Rc::clone(&cleanup_calls);
        composition
            .render(key, move || {
                if show.get() {
                    let local = useState(|| 0usize);
                    let cleanup_calls = Rc::clone(&cleanup_calls);
                    DisposableEffect!((), move |_| {
                        let cleanup_calls = Rc::clone(&cleanup_calls);
                        DisposableEffectResult::new(move || {
                            local.set(local.get() + 1);
                            cleanup_calls.set(cleanup_calls.get() + local.get());
                        })
                    });
                } else {
                    cranpose_test_node(TestTextNode::default);
                }
            })
            .expect("render succeeds");
    };

    render(&mut composition);
    show.set(false);
    render(&mut composition);

    assert_eq!(cleanup_calls.get(), 1);
}

#[test]
fn launched_effect_runs_and_cancels() {
    let mut composition = Composition::new(MemoryApplier::new());
    let runtime = composition.runtime_handle();
    let state = MutableState::with_runtime(0i32, runtime.clone());
    let runs = Arc::new(AtomicUsize::new(0));
    let captured_scopes: Rc<RefCell<Vec<LaunchedEffectScope>>> = Rc::new(RefCell::new(Vec::new()));

    let render = |composition: &mut Composition<MemoryApplier>, key_state: &MutableState<i32>| {
        let runs = Arc::clone(&runs);
        let scopes_for_render = Rc::clone(&captured_scopes);
        let state = *key_state;
        composition
            .render(0, move || {
                let key = state.value();
                let runs = Arc::clone(&runs);
                let captured_scopes = Rc::clone(&scopes_for_render);
                LaunchedEffect!(key, move |scope| {
                    runs.fetch_add(1, Ordering::SeqCst);
                    captured_scopes.borrow_mut().push(scope);
                });
            })
            .expect("render succeeds");
    };

    render(&mut composition, &state);
    assert_eq!(runs.load(Ordering::SeqCst), 1);
    {
        let scopes = captured_scopes.borrow();
        assert_eq!(scopes.len(), 1);
        assert!(scopes[0].is_active());
    }

    state.set_value(1);
    render(&mut composition, &state);
    assert_eq!(runs.load(Ordering::SeqCst), 2);
    {
        let scopes = captured_scopes.borrow();
        assert_eq!(scopes.len(), 2);
        assert!(!scopes[0].is_active(), "previous scope should be cancelled");
        assert!(scopes[1].is_active(), "latest scope remains active");
    }

    drop(composition);
    {
        let scopes = captured_scopes.borrow();
        assert!(!scopes.last().expect("scope available").is_active());
    }
}

#[test]
fn launched_effect_runs_side_effect_body() {
    let mut composition = Composition::new(MemoryApplier::new());
    let runtime = composition.runtime_handle();
    let state = MutableState::with_runtime(0i32, runtime);
    let (tx, rx) = std::sync::mpsc::channel();
    let captured_scopes: Rc<RefCell<Vec<LaunchedEffectScope>>> = Rc::new(RefCell::new(Vec::new()));

    {
        let captured_scopes = Rc::clone(&captured_scopes);
        composition
            .render(0, move || {
                let key = state.value();
                let tx = tx.clone();
                let captured_scopes = Rc::clone(&captured_scopes);
                LaunchedEffect!(key, move |scope| {
                    let _ = tx.send("start");
                    captured_scopes.borrow_mut().push(scope);
                });
            })
            .expect("render succeeds");
    }

    assert_eq!(rx.recv_timeout(Duration::from_secs(1)).unwrap(), "start");
    {
        let scopes = captured_scopes.borrow();
        assert_eq!(scopes.len(), 1);
        assert!(scopes[0].is_active());
    }

    drop(composition);
    {
        let scopes = captured_scopes.borrow();
        assert!(!scopes.last().expect("scope available").is_active());
    }
}

#[test]
fn launched_effect_background_updates_ui() {
    let mut composition = Composition::new(MemoryApplier::new());
    let runtime = composition.runtime_handle();
    let state = MutableState::with_runtime(0i32, runtime.clone());
    let (tx, rx) = std::sync::mpsc::channel::<i32>();
    let receiver = Rc::new(RefCell::new(Some(rx)));

    {
        let receiver = Rc::clone(&receiver);
        composition
            .render(0, move || {
                let receiver = Rc::clone(&receiver);
                LaunchedEffect!((), move |scope| {
                    if let Some(rx) = receiver.borrow_mut().take() {
                        scope.launch_background(
                            move |_| async move { rx.recv().expect("value available") },
                            move |value| state.set_value(value),
                        );
                    }
                });
            })
            .expect("render succeeds");
    }

    tx.send(27).expect("send succeeds");
    for _ in 0..5 {
        let _ = composition
            .process_invalid_scopes()
            .expect("process succeeds");
        if state.value() == 27 {
            break;
        }
        std::thread::sleep(Duration::from_millis(10));
    }
    assert_eq!(state.value(), 27);
}

#[test]
fn launched_effect_background_async_updates_ui() {
    let mut composition = Composition::new(MemoryApplier::new());
    let runtime = composition.runtime_handle();
    let state = MutableState::with_runtime(0i32, runtime.clone());
    let (tx, rx) = std::sync::mpsc::channel::<i32>();
    let receiver = Rc::new(RefCell::new(Some(rx)));

    {
        let receiver = Rc::clone(&receiver);
        composition
            .render(0, move || {
                let receiver = Rc::clone(&receiver);
                LaunchedEffect!((), move |scope| {
                    if let Some(rx) = receiver.borrow_mut().take() {
                        scope.launch_background(
                            move |_| async move { rx.recv().expect("value available") },
                            move |value| state.set_value(value),
                        );
                    }
                });
            })
            .expect("render succeeds");
    }

    tx.send(42).expect("send succeeds");
    for _ in 0..5 {
        let _ = composition
            .process_invalid_scopes()
            .expect("process succeeds");
        if state.value() == 42 {
            break;
        }
        std::thread::sleep(Duration::from_millis(10));
    }
    assert_eq!(state.value(), 42);
}

#[test]
fn launched_effect_background_ignores_late_result_after_cancel() {
    let mut composition = Composition::new(MemoryApplier::new());
    let runtime = composition.runtime_handle();
    let key_state = MutableState::with_runtime(0i32, runtime.clone());
    let result_state = MutableState::with_runtime(0i32, runtime.clone());
    let (tx, rx) = std::sync::mpsc::channel::<i32>();
    let receiver = Rc::new(RefCell::new(Some(rx)));

    {
        let receiver = Rc::clone(&receiver);
        composition
            .render(0, move || {
                let key = key_state.value();
                let receiver = Rc::clone(&receiver);
                LaunchedEffect!(key, move |scope| {
                    if key == 0 {
                        if let Some(rx) = receiver.borrow_mut().take() {
                            scope.launch_background(
                                move |_| async move { rx.recv().expect("value available") },
                                move |value| result_state.set_value(value),
                            );
                        }
                    }
                });
            })
            .expect("render succeeds");
    }

    key_state.set_value(1);

    {
        let receiver = Rc::clone(&receiver);
        composition
            .render(0, move || {
                let key = key_state.value();
                let receiver = Rc::clone(&receiver);
                LaunchedEffect!(key, move |scope| {
                    if key == 0 {
                        if let Some(rx) = receiver.borrow_mut().take() {
                            scope.launch_background(
                                move |_| async move { rx.recv().expect("value available") },
                                move |value| result_state.set_value(value),
                            );
                        }
                    }
                });
            })
            .expect("render succeeds");
    }

    tx.send(99).expect("send succeeds");
    for _ in 0..5 {
        let _ = composition
            .process_invalid_scopes()
            .expect("process succeeds");
        std::thread::sleep(Duration::from_millis(10));
    }
    assert_eq!(result_state.value(), 0);
}

#[test]
fn launched_effect_relaunches_on_branch_change() {
    // Test that LaunchedEffect with same key relaunches when switching if/else branches
    // This matches Jetpack Compose behavior
    let mut composition = Composition::new(MemoryApplier::new());
    let runtime = composition.runtime_handle();
    let _state = MutableState::with_runtime(false, runtime.clone());
    let runs = Arc::new(AtomicUsize::new(0));
    let recorded_scopes: Rc<RefCell<Vec<(bool, LaunchedEffectScope)>>> =
        Rc::new(RefCell::new(Vec::new()));

    let render = |composition: &mut Composition<MemoryApplier>, show_first: bool| {
        let runs = Arc::clone(&runs);
        let recorded_scopes = Rc::clone(&recorded_scopes);
        composition
            .render(0, move || {
                let runs = Arc::clone(&runs);
                let recorded_scopes = Rc::clone(&recorded_scopes);
                if show_first {
                    // Branch A with LaunchedEffect("") - macro captures call site location
                    LaunchedEffect!("", move |scope| {
                        runs.fetch_add(1, Ordering::SeqCst);
                        recorded_scopes.borrow_mut().push((true, scope));
                    });
                } else {
                    // Branch B with LaunchedEffect("") - different call site, separate group
                    LaunchedEffect!("", move |scope| {
                        runs.fetch_add(1, Ordering::SeqCst);
                        recorded_scopes.borrow_mut().push((false, scope));
                    });
                }
            })
            .expect("render succeeds");
    };

    // First render - branch A
    render(&mut composition, true);
    assert_eq!(runs.load(Ordering::SeqCst), 1, "First effect should run");
    {
        let scopes = recorded_scopes.borrow();
        assert_eq!(scopes.len(), 1);
        assert!(scopes[0].0, "first entry should come from branch A");
        assert!(scopes[0].1.is_active());
    }

    // Switch to branch B - should relaunch even with same key
    render(&mut composition, false);
    assert_eq!(
        runs.load(Ordering::SeqCst),
        2,
        "Second effect should run after branch switch"
    );
    {
        let scopes = recorded_scopes.borrow();
        assert_eq!(scopes.len(), 2);
        assert!(scopes[0].0);
        assert!(
            !scopes[0].1.is_active(),
            "branch A scope should be cancelled"
        );
        assert!(!scopes[1].0);
        assert!(
            scopes[1].1.is_active(),
            "branch B scope should remain active"
        );
    }

    drop(composition);
    {
        let scopes = recorded_scopes.borrow();
        assert!(!scopes.last().expect("branch B scope").1.is_active());
    }
}

#[test]
fn anchor_survives_conditional_removal() {
    let mut composition = Composition::new(MemoryApplier::new());
    let runtime = composition.runtime_handle();
    let toggle = MutableState::with_runtime(true, runtime.clone());
    let runs = Arc::new(AtomicUsize::new(0));
    let captured_scope: Rc<RefCell<Option<LaunchedEffectScope>>> = Rc::new(RefCell::new(None));

    let render = |composition: &mut Composition<MemoryApplier>| {
        let runs = Arc::clone(&runs);
        let captured_scope = Rc::clone(&captured_scope);
        composition
            .render(0, move || {
                if toggle.value() {
                    cranpose_core::with_current_composer(|composer| {
                        composer.emit_node(|| TestDummyNode);
                    });
                }

                let runs_for_effect = Arc::clone(&runs);
                let scope_slot = Rc::clone(&captured_scope);
                LaunchedEffect!((), move |scope| {
                    runs_for_effect.fetch_add(1, Ordering::SeqCst);
                    scope_slot.borrow_mut().replace(scope);
                });
            })
            .expect("render succeeds");
    };

    render(&mut composition);
    assert_eq!(
        runs.load(Ordering::SeqCst),
        1,
        "effect should run exactly once on first composition"
    );
    {
        let scope_ref = captured_scope.borrow();
        let scope = scope_ref.as_ref().expect("scope captured on first run");
        assert!(scope.is_active(), "scope stays active after first run");
    }

    toggle.set_value(false);
    render(&mut composition);
    assert_eq!(
        runs.load(Ordering::SeqCst),
        1,
        "effect should not rerun while conditional is absent"
    );
    assert_eq!(
        runs.load(Ordering::SeqCst),
        1,
        "effect run count should remain stable after conditional removal"
    );
    {
        let scope_ref = captured_scope.borrow();
        let scope = scope_ref
            .as_ref()
            .expect("scope retained after conditional removal");
        assert!(
            scope.is_active(),
            "anchor should keep effect alive when slots ahead disappear"
        );
    }

    toggle.set_value(true);
    render(&mut composition);
    assert!(
        runs.load(Ordering::SeqCst) >= 1,
        "effect should remain launched after conditional restoration"
    );
    {
        let scope_ref = captured_scope.borrow();
        let scope = scope_ref
            .as_ref()
            .expect("scope retained after conditional restoration");
        assert!(
            scope.is_active(),
            "scope should remain active after conditional restoration"
        );
    }

    drop(composition);
    {
        let scope_ref = captured_scope.borrow();
        let scope = scope_ref.as_ref().expect("scope retained for final check");
        assert!(
            !scope.is_active(),
            "dropping the composition should cancel the effect"
        );
    }
}

#[test]
fn launched_effect_async_survives_conditional_cycle() {
    let mut composition = Composition::new(MemoryApplier::new());
    let runtime_handle = composition.runtime_handle();
    let gate = MutableState::with_runtime(true, runtime_handle.clone());
    let log: Rc<RefCell<Vec<u32>>> = Rc::new(RefCell::new(Vec::new()));
    let spawns = Arc::new(AtomicUsize::new(0));

    let mut render = {
        let log = log.clone();
        let spawns = Arc::clone(&spawns);
        move || {
            if gate.value() {
                cranpose_core::with_current_composer(|composer| {
                    composer.emit_node(|| TestDummyNode);
                });
            }

            let log = log.clone();
            let spawns = Arc::clone(&spawns);
            cranpose_core::LaunchedEffectAsync!((), move |scope| {
                spawns.fetch_add(1, Ordering::SeqCst);
                let log = log.clone();
                Box::pin(async move {
                    let clock = scope.runtime().frame_clock();
                    while scope.is_active() {
                        clock.next_frame().await;
                        if !scope.is_active() {
                            break;
                        }
                        log.borrow_mut().push(1);
                    }
                })
            });
        }
    };

    let key = location_key(file!(), line!(), column!());
    composition
        .render(key, &mut render)
        .expect("initial render");
    runtime_handle.drain_ui();
    runtime_handle.drain_frame_callbacks(1);
    runtime_handle.drain_ui();

    let initial_spawns = spawns.load(Ordering::SeqCst);
    assert!(initial_spawns >= 1, "effect should launch initially");
    {
        let log = log.borrow();
        assert!(
            !log.is_empty(),
            "effect should produce entries after initial frame callback"
        );
    }

    gate.set_value(false);
    composition
        .render(key, &mut render)
        .expect("render with gate disabled");
    runtime_handle.drain_ui();
    let entries_before_pause = log.borrow().len();
    runtime_handle.drain_frame_callbacks(2);
    runtime_handle.drain_ui();
    runtime_handle.drain_frame_callbacks(3);
    runtime_handle.drain_ui();
    {
        let log = log.borrow();
        assert!(
            log.len() > entries_before_pause,
            "effect should keep running while conditional content is absent"
        );
    }
    assert_eq!(
        spawns.load(Ordering::SeqCst),
        initial_spawns,
        "effect should not relaunch when conditional collapses"
    );

    gate.set_value(true);
    composition
        .render(key, &mut render)
        .expect("render with gate restored");
    runtime_handle.drain_ui();
    let entries_before_restore = log.borrow().len();
    runtime_handle.drain_frame_callbacks(4);
    runtime_handle.drain_ui();
    {
        let log = log.borrow();
        assert!(
            log.len() > entries_before_restore,
            "effect should continue running after conditional is restored"
        );
    }
    assert!(
        spawns.load(Ordering::SeqCst) >= initial_spawns,
        "effect should remain launched after conditional restoration"
    );

    let entries_before_drop = log.borrow().len();
    drop(composition);
    runtime_handle.drain_frame_callbacks(5);
    runtime_handle.drain_ui();
    {
        let log = log.borrow();
        assert_eq!(
            log.len(),
            entries_before_drop,
            "effect should stop producing entries after composition is dropped"
        );
    }
}

#[test]
fn launched_effect_async_keeps_frames_after_backward_forward_flip() {
    #[derive(Clone, Copy, Debug)]
    struct TestAnimation {
        progress: f32,
        direction: f32,
    }

    impl Default for TestAnimation {
        fn default() -> Self {
            Self {
                progress: 0.0,
                direction: 1.0,
            }
        }
    }

    #[derive(Clone, Copy, Debug)]
    struct TestFrameStats {
        frames: u32,
        last_frame_ms: f32,
    }

    impl Default for TestFrameStats {
        fn default() -> Self {
            Self {
                frames: 0,
                last_frame_ms: 0.0,
            }
        }
    }

    let mut composition = Composition::new(MemoryApplier::new());
    let runtime = composition.runtime_handle();
    let animation = MutableState::with_runtime(TestAnimation::default(), runtime.clone());
    let stats = MutableState::with_runtime(TestFrameStats::default(), runtime.clone());

    let mut render = {
        move || {
            cranpose_core::LaunchedEffectAsync!((), move |scope| {
                Box::pin(async move {
                    let clock = scope.runtime().frame_clock();
                    let mut last_time: Option<u64> = None;
                    while scope.is_active() {
                        let nanos = clock.next_frame().await;
                        if !scope.is_active() {
                            break;
                        }

                        if let Some(previous) = last_time {
                            let mut delta = nanos.saturating_sub(previous);
                            if delta == 0 {
                                delta = 16_666_667;
                            }
                            let dt_ms = delta as f32 / 1_000_000.0;
                            stats.update(|state| {
                                state.frames = state.frames.wrapping_add(1);
                                state.last_frame_ms = dt_ms;
                            });
                            animation.update(|anim| {
                                let next = anim.progress + 0.1 * anim.direction * (dt_ms / 600.0);
                                if next >= 1.0 {
                                    anim.progress = 1.0;
                                    anim.direction = -1.0;
                                } else if next <= 0.0 {
                                    anim.progress = 0.0;
                                    anim.direction = 1.0;
                                } else {
                                    anim.progress = next;
                                }
                            });
                        }

                        last_time = Some(nanos);
                    }
                })
            });

            let snapshot = animation.value();
            if snapshot.progress > 0.0 {
                cranpose_core::with_current_composer(|composer| {
                    composer.emit_node(|| TestDummyNode);
                });
            }

            // Touch stats to subscribe the current scope.
            let _stats = stats.value();
        }
    };

    let key = location_key(file!(), line!(), column!());
    composition
        .render(key, &mut render)
        .expect("initial render");
    runtime.drain_ui();
    let _ = composition
        .process_invalid_scopes()
        .expect("initial recomposition");

    let mut last_direction = animation.value().direction;
    assert_eq!(last_direction, 1.0, "animation starts moving forward");

    let mut forward_flip_observed = false;
    let mut time = 0u64;
    for _step in 0..32 {
        time += 1_000_000_000;
        runtime.drain_frame_callbacks(time);
        let _ = composition
            .process_invalid_scopes()
            .expect("process recompositions");

        let anim = animation.value();
        let frames = stats.value().frames;

        if last_direction < 0.0 && anim.direction > 0.0 {
            forward_flip_observed = true;
            let frames_before = frames;

            for _ in 0..3 {
                time += 1_000_000_000;
                runtime.drain_frame_callbacks(time);
                let _ = composition
                    .process_invalid_scopes()
                    .expect("process recompositions after flip");
            }

            let frames_after = stats.value().frames;
            assert!(
                frames_after > frames_before,
                "frames should continue increasing after backward->forward flip (before {}, after {})",
                frames_before,
                frames_after
            );
            break;
        }

        last_direction = anim.direction;
    }

    assert!(
        forward_flip_observed,
        "animation should experience a backward->forward transition"
    );

    drop(composition);
    runtime.drain_frame_callbacks(time.saturating_add(1));
    runtime.drain_ui();
}

#[test]
fn stats_scope_survives_conditional_gap() {
    #[derive(Clone, Copy, Debug, Default)]
    struct SimpleStats {
        frames: u32,
    }

    let mut composition = Composition::new(MemoryApplier::new());
    let runtime = composition.runtime_handle();
    let animation = MutableState::with_runtime(0.0f32, runtime.clone());
    let stats = MutableState::with_runtime(SimpleStats::default(), runtime.clone());
    let log: Rc<RefCell<Vec<String>>> = Rc::new(RefCell::new(Vec::new()));

    #[composable]
    fn runtime_demo(
        animation: MutableState<f32>,
        stats: MutableState<SimpleStats>,
        log: Rc<RefCell<Vec<String>>>,
    ) {
        let progress = animation.value();
        let stats_snapshot = stats.value();

        with_current_composer(|composer| {
            composer.with_group(location_key(file!(), line!(), column!()), |composer| {
                let progress_for_slot = progress;
                composer.with_group(location_key(file!(), line!(), column!()), |composer| {
                    if progress_for_slot > 0.0 {
                        let id = composer.emit_node(|| TestDummyNode);
                        log.borrow_mut()
                            .push(format!("dummy {}", progress_for_slot));
                        composer
                            .with_node_mut(id, |_: &mut TestDummyNode| {})
                            .expect("dummy node exists");
                    }
                });

                composer.with_group(location_key(file!(), line!(), column!()), |composer| {
                    let id = composer.emit_node(TestTextNode::default);
                    log.borrow_mut()
                        .push(format!("frames {}", stats_snapshot.frames));
                    composer
                        .with_node_mut(id, |node: &mut TestTextNode| {
                            node.text = format!("{}", stats_snapshot.frames);
                        })
                        .expect("update text node");
                });
            });
        });
    }

    let mut render = {
        let log = Rc::clone(&log);
        move || runtime_demo(animation, stats, Rc::clone(&log))
    };

    let key = location_key(file!(), line!(), column!());
    composition
        .render(key, &mut render)
        .expect("initial render");

    fn drain_all<A: Applier + 'static>(composition: &mut Composition<A>) -> Result<(), NodeError> {
        loop {
            if !composition.process_invalid_scopes()? {
                break;
            }
        }
        Ok(())
    }

    drain_all(&mut composition).expect("initial drain");
    {
        let entries = log.borrow();
        assert_eq!(
            entries.as_slice(),
            ["frames 0"],
            "initial composition should render frames text once"
        );
    }
    log.borrow_mut().clear();

    animation.set_value(1.0);
    drain_all(&mut composition).expect("recompose at progress 1.0");
    {
        let entries = log.borrow();
        assert!(
            entries.iter().any(|entry| entry.starts_with("dummy")),
            "progress > 0 should render dummy node"
        );
        assert!(
            entries.iter().any(|entry| entry == "frames 0"),
            "frames text should render after progress increases"
        );
    }
    log.borrow_mut().clear();

    animation.set_value(0.0);
    drain_all(&mut composition).expect("recompose at progress 0.0");
    {
        let entries = log.borrow();
        assert!(
            entries.iter().all(|entry| entry.starts_with("frames")),
            "only frames text should render when progress is zero"
        );
    }
    log.borrow_mut().clear();

    stats.update(|value| value.frames = value.frames.wrapping_add(1));
    drain_all(&mut composition).expect("recompose after stats update");
    {
        let entries = log.borrow();
        assert!(
            entries.iter().any(|entry| entry == "frames 1"),
            "frames text should re-render after stats change even after a gap"
        );
    }
}

#[test]
fn slot_table_remember_replaces_mismatched_type() {
    let mut slots = SlotTable::new();

    {
        let value = slots.remember(|| 42i32);
        assert_eq!(value.with(|value| *value), 42);
    }

    slots.reset();

    {
        let value = slots.remember(|| "updated");
        assert_eq!(value.with(|&value| value), "updated");
    }

    slots.reset();

    {
        let value = slots.remember(|| "should not run");
        assert_eq!(value.with(|&value| value), "updated");
    }
}

#[test]
fn frame_callbacks_fire_in_registration_order() {
    let runtime = Runtime::new(Arc::new(TestScheduler));
    let handle = runtime.handle();
    let clock = runtime.frame_clock();
    let events: Rc<RefCell<Vec<&'static str>>> = Rc::new(RefCell::new(Vec::new()));
    let mut guards = Vec::new();
    {
        let events = events.clone();
        guards.push(clock.with_frame_nanos(move |_| {
            events.borrow_mut().push("first");
        }));
    }
    {
        let events = events.clone();
        guards.push(clock.with_frame_nanos(move |_| {
            events.borrow_mut().push("second");
        }));
    }

    handle.drain_frame_callbacks(42);
    drop(guards);

    let events = events.borrow();
    assert_eq!(events.as_slice(), ["first", "second"]);
    assert!(!runtime.needs_frame());
}

#[test]
fn next_frame_future_resolves_after_callback() {
    let runtime = Runtime::new(Arc::new(TestScheduler));
    let handle = runtime.handle();
    let clock = runtime.frame_clock();
    let state = MutableState::with_runtime(0u64, handle.clone());

    {
        let clock = clock.clone();
        handle
            .spawn_ui(async move {
                let first = clock.next_frame().await;
                state.update(|value| *value = first);
                let second = clock.next_frame().await;
                state.update(|value| *value = second);
            })
            .expect("spawn_ui returns handle");
    }

    handle.drain_ui();
    assert_eq!(state.value(), 0);

    handle.drain_frame_callbacks(100);
    handle.drain_ui();
    assert_eq!(state.value(), 100);

    handle.drain_frame_callbacks(200);
    handle.drain_ui();
    assert_eq!(state.value(), 200);
}

#[test]
fn cancelling_frame_callback_prevents_execution() {
    let runtime = Runtime::new(Arc::new(TestScheduler));
    let handle = runtime.handle();
    let clock = runtime.frame_clock();
    let events: Rc<RefCell<Vec<&'static str>>> = Rc::new(RefCell::new(Vec::new()));

    let registration = {
        let events = events.clone();
        clock.with_frame_nanos(move |_| {
            events.borrow_mut().push("fired");
        })
    };

    assert!(runtime.needs_frame());
    drop(registration);
    handle.drain_frame_callbacks(84);
    assert!(events.borrow().is_empty());
    assert!(!runtime.needs_frame());
}

#[test]
fn launched_effect_async_restarts_on_key_change() {
    let mut composition = Composition::new(MemoryApplier::new());
    let runtime_handle = composition.runtime_handle();
    let key_state = MutableState::with_runtime(0i32, runtime_handle.clone());
    let log: Rc<RefCell<Vec<i32>>> = Rc::new(RefCell::new(Vec::new()));

    let mut render = {
        let log = log.clone();
        move || {
            let key = key_state.value();
            let log = log.clone();
            cranpose_core::LaunchedEffectAsync!(key, move |scope| {
                let log = log.clone();
                Box::pin(async move {
                    let clock = scope.runtime().frame_clock();
                    loop {
                        clock.next_frame().await;
                        if !scope.is_active() {
                            return;
                        }
                        log.borrow_mut().push(key);
                    }
                })
            });
        }
    };

    composition
        .render(location_key(file!(), line!(), column!()), &mut render)
        .expect("initial render");

    runtime_handle.drain_ui();
    runtime_handle.drain_frame_callbacks(1);
    runtime_handle.drain_ui();
    runtime_handle.drain_frame_callbacks(2);
    runtime_handle.drain_ui();

    {
        let log = log.borrow();
        assert_eq!(log.as_slice(), &[0, 0]);
    }

    key_state.set_value(1);
    composition
        .render(location_key(file!(), line!(), column!()), &mut render)
        .expect("re-render with new key");

    runtime_handle.drain_ui();
    runtime_handle.drain_frame_callbacks(3);
    runtime_handle.drain_ui();

    {
        let log = log.borrow();
        assert_eq!(log.as_slice(), &[0, 0, 1]);
    }

    drop(composition);
    runtime_handle.drain_frame_callbacks(4);
    runtime_handle.drain_ui();

    {
        let log = log.borrow();
        assert_eq!(log.as_slice(), &[0, 0, 1]);
    }
}

#[test]
fn draining_callbacks_clears_needs_frame() {
    let runtime = Runtime::new(Arc::new(TestScheduler));
    let handle = runtime.handle();
    let clock = runtime.frame_clock();

    let guard = clock.with_frame_nanos(|_| {});
    assert!(runtime.needs_frame());
    handle.drain_frame_callbacks(128);
    drop(guard);
    assert!(!runtime.needs_frame());
}

#[composable]
fn frame_callback_node(events: Rc<RefCell<Vec<&'static str>>>) -> NodeId {
    let runtime = cranpose_core::with_current_composer(|composer| composer.runtime_handle());
    DisposableEffect!((), move |scope| {
        let clock = runtime.frame_clock();
        let events = events.clone();
        let registration = clock.with_frame_nanos(move |_| {
            events.borrow_mut().push("fired");
        });
        scope.on_dispose(move || drop(registration));
        DisposableEffectResult::default()
    });
    cranpose_test_node(TestTextNode::default)
}

#[test]
fn disposing_scope_cancels_pending_frame_callback() {
    let mut composition = Composition::new(MemoryApplier::new());
    let runtime_handle = composition.runtime_handle();
    let events: Rc<RefCell<Vec<&'static str>>> = Rc::new(RefCell::new(Vec::new()));
    let active = cranpose_core::MutableState::with_runtime(true, runtime_handle.clone());
    let mut render = {
        let events = events.clone();
        move || {
            if active.value() {
                frame_callback_node(events.clone());
            }
        }
    };

    composition
        .render(location_key(file!(), line!(), column!()), &mut render)
        .expect("initial render");

    active.set(false);
    composition
        .render(location_key(file!(), line!(), column!()), &mut render)
        .expect("removal render");

    runtime_handle.drain_frame_callbacks(512);
    assert!(events.borrow().is_empty());
}
