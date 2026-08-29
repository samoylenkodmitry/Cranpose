use super::*;

struct SumPolicy;

impl MutationPolicy<i32> for SumPolicy {
    fn equivalent(&self, a: &i32, b: &i32) -> bool {
        a == b
    }

    fn merge(&self, previous: &i32, current: &i32, applied: &i32) -> Option<i32> {
        Some((current - previous) + (applied - previous) + previous)
    }
}

#[test]
fn snapshot_state_global_write_then_read() {
    let _guard = reset_snapshot_runtime();
    let state = SnapshotMutableState::new_in_arc(0, Arc::new(SumPolicy));
    assert_eq!(state.get(), 0);
    state.set(1);
    assert_eq!(state.get(), 1);
}

#[test]
fn snapshot_state_global_equivalent_write_preserves_mutation_policy() {
    let _guard = reset_snapshot_runtime();
    let state = SnapshotMutableState::new_in_arc(0, Arc::new(SumPolicy));
    let notifications = Rc::new(RefCell::new(Vec::new()));
    let notifications_for_observer = Rc::clone(&notifications);
    let _handle =
        crate::snapshot_v2::register_apply_observer(Rc::new(move |modified, snapshot_id| {
            notifications_for_observer
                .borrow_mut()
                .push((modified.len(), snapshot_id));
        }));

    state.set(0);

    let notifications = notifications.borrow();
    assert_eq!(
        notifications.len(),
        0,
        "global equivalent writes must not notify apply observers"
    );
}

#[test]
fn snapshot_state_child_isolation_and_apply() {
    let _guard = reset_snapshot_runtime();
    let state = SnapshotMutableState::new_in_arc(0, Arc::new(SumPolicy));

    let child = take_mutable_snapshot(None, None);
    child.enter(|| {
        state.set(2);
        assert_eq!(state.get(), 2);
    });

    assert_eq!(state.get(), 0);

    child.apply().check();
    assert_eq!(state.get(), 2);
}

#[test]
fn snapshot_state_concurrent_children_merge() {
    let _guard = reset_snapshot_runtime();
    let state = SnapshotMutableState::new_in_arc(0, Arc::new(SumPolicy));

    let first = take_mutable_snapshot(None, None);
    let second = take_mutable_snapshot(None, None);

    first.enter(|| state.set(1));
    second.enter(|| state.set(2));

    first.apply().check();
    second.apply().check();
    assert_eq!(state.get(), 3);
}

#[test]
fn snapshot_state_child_apply_after_parent_history() {
    let _guard = reset_snapshot_runtime();
    let state = SnapshotMutableState::new_in_arc(0, Arc::new(SumPolicy));

    for value in 1..=5 {
        state.set(value);
    }

    let child = take_mutable_snapshot(None, None);
    child.enter(|| state.set(42));

    child.apply().check();
    assert_eq!(state.get(), 42);
}

#[composable]
fn anchor_progress_content(toggle: MutableState<bool>, stats: MutableState<i32>) {
    let show_progress = toggle.value();
    cranpose_core::with_current_composer(|composer| {
        composer.with_group(location_key(file!(), line!(), column!()), |composer| {
            if show_progress {
                composer.with_group(location_key(file!(), line!(), column!()), |composer| {
                    composer.emit_node(|| TestDummyNode);
                });
            }
        });
    });
    let _ = stats.value();
}

#[test]
fn stats_watchers_survive_conditional_toggle() {
    fn drain_all(composition: &mut Composition<MemoryApplier>) -> Result<(), NodeError> {
        loop {
            if !composition.process_invalid_scopes()? {
                break;
            }
        }
        Ok(())
    }

    let mut composition = test_composition();
    let runtime = composition.runtime_handle();
    let toggle = MutableState::with_runtime(true, runtime.clone());
    let stats = MutableState::with_runtime(0i32, runtime.clone());

    let mut render = { move || anchor_progress_content(toggle, stats) };

    let key = location_key(file!(), line!(), column!());
    composition
        .render(key, &mut render)
        .expect("initial render");
    drain_all(&mut composition).expect("initial drain");
    assert!(
        stats.watcher_count() > 0,
        "initial render should register stats watcher"
    );

    toggle.set_value(false);
    composition
        .render(key, &mut render)
        .expect("render without progress");
    drain_all(&mut composition).expect("drain without progress");
    assert!(
        stats.watcher_count() > 0,
        "conditional removal should not drop stats watcher"
    );

    toggle.set_value(true);
    composition
        .render(key, &mut render)
        .expect("render with progress again");
    drain_all(&mut composition).expect("drain with progress");
    assert!(
        stats.watcher_count() > 0,
        "restoring progress should keep stats watcher"
    );
}

#[test]
fn state_write_prunes_dropped_watchers() {
    let runtime = TestRuntime::new();
    let handle = runtime.handle();
    let state = MutableState::with_runtime(0i32, handle.clone());
    let scope = RecomposeScope::new_for_test(handle);

    state.subscribe_scope_for_test(&scope);
    assert_eq!(state.watcher_count(), 1);

    drop(scope);
    state.set_value(1);

    assert_eq!(state.watcher_count(), 0);
}

#[test]
fn state_subscriber_callback_tracks_first_live_scope() {
    let runtime = TestRuntime::new();
    let handle = runtime.handle();
    let state = MutableState::with_runtime(0i32, handle.clone());
    let notifications = Rc::new(Cell::new(0));
    let notifications_for_callback = Rc::clone(&notifications);
    let callback = Rc::new(move || {
        notifications_for_callback.set(notifications_for_callback.get() + 1);
    });
    state.as_state().on_subscriber(callback.clone());
    let observer = SnapshotStateObserver::new(|callback| callback());

    observer.observe_reads(1usize, |_| {}, || state.get());
    assert_eq!(notifications.get(), 1);

    observer.observe_reads(2usize, |_| {}, || state.get());
    assert_eq!(notifications.get(), 1);

    observer.clear(&1usize);
    observer.clear(&2usize);
    assert!(!state.as_state().has_subscribers());

    observer.observe_reads(3usize, |_| {}, || state.get());
    assert_eq!(notifications.get(), 2);
}

#[composable]
fn subscriber_callback_reader(state: MutableState<i32>) {
    let _ = state.get();
}

#[test]
fn state_subscriber_callback_fires_once_for_a_composition_read() {
    let mut composition = test_composition();
    let state = MutableState::with_runtime(0i32, composition.runtime_handle());
    let notifications = Rc::new(Cell::new(0));
    let notifications_for_callback = Rc::clone(&notifications);
    let callback: Rc<dyn Fn()> = Rc::new(move || {
        notifications_for_callback.set(notifications_for_callback.get() + 1);
    });
    state.as_state().on_subscriber(callback.clone());

    composition
        .render(location_key(file!(), line!(), column!()), move || {
            subscriber_callback_reader(state);
        })
        .expect("composition read");
    assert_eq!(notifications.get(), 1);

    state.set(1);
    while composition
        .process_invalid_scopes()
        .expect("process state invalidation")
    {}
    assert_eq!(notifications.get(), 1);
}

#[test]
fn state_subscriber_callback_treats_scope_and_read_observers_as_one_subscription() {
    let runtime = TestRuntime::new();
    let state = MutableState::with_runtime(0i32, runtime.handle());
    let notifications = Rc::new(Cell::new(0));
    let notifications_for_callback = Rc::clone(&notifications);
    let callback: Rc<dyn Fn()> = Rc::new(move || {
        notifications_for_callback.set(notifications_for_callback.get() + 1);
    });
    state.as_state().on_subscriber(callback.clone());

    let observer = SnapshotStateObserver::new(|callback| callback());
    observer.observe_reads(1usize, |_| {}, || state.get());
    let scope = RecomposeScope::new_for_test(runtime.handle());
    state.subscribe_scope_for_test(&scope);
    observer.clear(&1usize);
    observer.observe_reads(2usize, |_| {}, || state.get());

    assert_eq!(notifications.get(), 1);
}

#[test]
fn state_subscriber_callback_does_not_retain_dropped_callback() {
    let runtime = TestRuntime::new();
    let handle = runtime.handle();
    let state = MutableState::with_runtime(0i32, handle.clone());
    let callback = Rc::new(|| {});
    let weak = Rc::downgrade(&callback);
    state.as_state().on_subscriber(callback.clone());
    drop(callback);

    let observer = SnapshotStateObserver::new(|callback| callback());
    observer.observe_reads(1usize, |_| {}, || state.get());
    assert!(weak.upgrade().is_none());
}

#[test]
fn state_subscriber_callback_keeps_reentrant_registration() {
    let runtime = TestRuntime::new();
    let handle = runtime.handle();
    let state = MutableState::with_runtime(0i32, handle.clone());
    let nested_notifications = Rc::new(Cell::new(0));
    let nested_callback: Rc<dyn Fn()> = {
        let nested_notifications = Rc::clone(&nested_notifications);
        Rc::new(move || nested_notifications.set(nested_notifications.get() + 1))
    };
    let nested_lifetime = Rc::new(RefCell::new(Some(Rc::clone(&nested_callback))));
    let registered = Rc::new(Cell::new(false));
    let callback: Rc<dyn Fn()> = {
        let registered = Rc::clone(&registered);
        let nested_lifetime = Rc::clone(&nested_lifetime);
        Rc::new(move || {
            if !registered.replace(true) {
                state.as_state().on_subscriber(
                    nested_lifetime
                        .borrow()
                        .as_ref()
                        .expect("nested callback")
                        .clone(),
                );
            }
        })
    };
    state.as_state().on_subscriber(callback.clone());
    let observer = SnapshotStateObserver::new(|callback| callback());

    observer.observe_reads(1usize, |_| {}, || state.get());
    assert_eq!(nested_notifications.get(), 1);
    observer.clear(&1usize);

    observer.observe_reads(2usize, |_| {}, || state.get());
    assert_eq!(nested_notifications.get(), 2);
}

#[test]
fn state_subscriber_callback_prunes_dead_registrations_while_subscribed() {
    let runtime = TestRuntime::new();
    let handle = runtime.handle();
    let state = MutableState::with_runtime(0i32, handle.clone());
    let observer = SnapshotStateObserver::new(|callback| callback());
    observer.observe_reads(1usize, |_| {}, || state.get());

    for _ in 0..256 {
        state.as_state().on_subscriber(Rc::new(|| {}));
    }

    assert_eq!(state.subscriber_callback_count(), 0);
}

#[test]
fn dropping_scope_unregisters_watchers_without_global_prune() {
    let runtime = TestRuntime::new();
    let handle = runtime.handle();
    let state = MutableState::with_runtime(0i32, handle.clone());

    let scopes: Vec<_> = (0..1024)
        .map(|_| {
            let scope = RecomposeScope::new_for_test(handle.clone());
            state.subscribe_scope_for_test(&scope);
            scope
        })
        .collect();

    assert_eq!(state.watcher_count(), scopes.len());
    assert!(
        state.watcher_capacity() >= scopes.len(),
        "watcher capacity should reflect the retained subscriptions"
    );

    drop(scopes);

    assert_eq!(state.watcher_count(), 0);
    assert!(
        state.watcher_capacity() <= 32,
        "watcher capacity should shrink after scope drop cleanup; got {}",
        state.watcher_capacity()
    );
}

#[test]
fn dropping_scope_after_state_release_is_a_noop() {
    let runtime = TestRuntime::new();
    let handle = runtime.handle();
    let scope = RecomposeScope::new_for_test(handle.clone());
    let state = OwnedMutableState::with_runtime(0i32, handle);

    state.handle().subscribe_scope_for_test(&scope);
    drop(state);
    drop(scope);
}
