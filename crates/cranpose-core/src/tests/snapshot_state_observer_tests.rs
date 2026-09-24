use std::cell::Cell;

use super::*;
use crate::{
    snapshot_v2::{TestRuntimeGuard, reset_runtime_for_tests, take_mutable_snapshot},
    state::{NeverEqual, SnapshotMutableState},
};

fn reset_runtime() -> TestRuntimeGuard {
    reset_runtime_for_tests()
}

#[derive(Clone, Eq, Hash, PartialEq)]
struct TestScope(&'static str);

#[test]
fn scope_update_reuses_storage_and_replaces_payload_and_callback() {
    let first = Rc::new(String::from("first"));
    let second = Rc::new(String::from("second"));
    let delivered = Rc::new(RefCell::new(Vec::new()));
    let mut entry = ScopeEntry::new(0, first.clone(), Rc::new(|_| panic!("stale callback")));
    let ScopeStorage::Owned(stored) = &entry.scope else {
        panic!("expected owned scope");
    };
    let address = stored.downcast_ref::<Rc<String>>().unwrap() as *const Rc<String>;
    let received = delivered.clone();
    entry.update(
        second.clone(),
        Rc::new(move |scope| {
            received.borrow_mut().push(
                scope
                    .downcast_ref::<Rc<String>>()
                    .unwrap()
                    .as_str()
                    .to_owned(),
            );
        }),
    );
    assert_eq!(Rc::strong_count(&first), 1);
    assert_eq!(Rc::strong_count(&second), 2);
    entry.notify();
    assert_eq!(*delivered.borrow(), vec!["second"]);
    let ScopeStorage::Owned(stored) = &entry.scope else {
        panic!("expected owned scope");
    };
    assert_eq!(
        stored.downcast_ref::<Rc<String>>().unwrap() as *const Rc<String>,
        address
    );
    drop(entry);
    assert_eq!(Rc::strong_count(&second), 1);
}

#[test]
fn reobservation_refreshes_captures_and_preserves_shared_callbacks() {
    let _guard = reset_runtime();
    let state = SnapshotMutableState::new_in_arc(0, Arc::new(NeverEqual));
    let delivered = Rc::new(RefCell::new(Vec::new()));
    let callback = |generation| {
        let delivered = delivered.clone();
        move |scope: &TestScope| delivered.borrow_mut().push((generation, scope.0))
    };
    let scope = TestScope("callback");
    let observer = SnapshotStateObserver::new(|callback| callback());
    let read = || {
        let _ = state.get();
    };
    observer.observe_reads(scope.clone(), callback(1), read);
    let entry = observer.inner.find_scope_entry(&scope).unwrap();
    let held = entry.borrow().on_changed.clone();
    observer.observe_reads(scope.clone(), callback(2), read);
    held(&scope);
    entry.borrow().notify();
    drop(held);

    let allocation = Rc::as_ptr(&entry.borrow().on_changed);
    observer.observe_reads(scope.clone(), callback(3), read);
    assert!(std::ptr::addr_eq(
        allocation,
        Rc::as_ptr(&entry.borrow().on_changed)
    ));
    entry.borrow().notify();

    let received = delivered.clone();
    observer.observe_reads(
        scope,
        move |scope| received.borrow_mut().push((4, scope.0)),
        read,
    );
    entry.borrow().notify();
    assert_eq!(
        *delivered.borrow(),
        [
            (1, "callback"),
            (2, "callback"),
            (3, "callback"),
            (4, "callback")
        ]
    );
}

#[test]
fn reobservation_across_storage_thresholds_replaces_dependencies_and_callbacks() {
    let _guard = reset_runtime();
    let states: Vec<_> = (0..MAX_OBSERVED_STATES + 2)
        .map(|_| SnapshotMutableState::new_in_arc(0, Arc::new(NeverEqual)))
        .collect();
    let notifications = Rc::new(RefCell::new(Vec::new()));
    let observer = SnapshotStateObserver::new(|callback| callback());
    observer.start();
    for (generation, count) in [MAX_OBSERVED_STATES, MAX_OBSERVED_STATES + 1, 2, 0]
        .into_iter()
        .enumerate()
    {
        observer.begin_frame();
        let recorded = notifications.clone();
        observer.observe_reads(
            TestScope("changing"),
            move |scope| {
                assert_eq!(scope.0, "changing");
                recorded.borrow_mut().push(generation);
            },
            || {
                for state in states.iter().take(count) {
                    let _ = state.get();
                    let _ = state.get();
                }
            },
        );
        notifications.borrow_mut().clear();
        for (index, state) in states.iter().enumerate() {
            let snapshot = take_mutable_snapshot(None, None);
            snapshot.enter(|| state.set(generation as i32));
            snapshot.apply().check();
            let expected = (index + 1).min(count);
            assert_eq!(
                *notifications.borrow(),
                vec![generation; expected],
                "count={count}, changed state={index}"
            );
        }
    }
}

#[test]
fn reobservation_preserves_notifications_when_dependencies_repeat_or_change() {
    let _guard = reset_runtime();
    let states: Vec<_> = (0..3)
        .map(|_| SnapshotMutableState::new_in_arc(0, Arc::new(NeverEqual)))
        .collect();
    let notifications = Rc::new(RefCell::new(Vec::new()));
    let observer = SnapshotStateObserver::new(|callback| callback());
    observer.start();
    for (generation, indices) in [[0, 1], [0, 1], [1, 0], [1, 2], [1, 2]]
        .into_iter()
        .enumerate()
    {
        observer.begin_frame();
        let received = notifications.clone();
        observer.observe_reads(
            TestScope("repeated"),
            move |_| received.borrow_mut().push(generation),
            || {
                for index in indices {
                    let _ = states[index].get();
                }
            },
        );
        for (index, state) in states.iter().enumerate() {
            notifications.borrow_mut().clear();
            let snapshot = take_mutable_snapshot(None, None);
            snapshot.enter(|| state.set(generation as i32));
            snapshot.apply().check();
            assert_eq!(
                *notifications.borrow(),
                if indices.contains(&index) {
                    vec![generation]
                } else {
                    vec![]
                },
                "generation={generation}, state={index}"
            );
        }
    }
    observer.clear(&TestScope("repeated"));
    notifications.borrow_mut().clear();
    for state in states {
        observer.notify_changes(&[state]);
    }
    assert!(notifications.borrow().is_empty());
}

#[test]
fn stateless_scope_can_start_observing_and_replace_its_callback_before_the_block() {
    let _guard = reset_runtime();
    let observer = SnapshotStateObserver::new(|callback| callback());
    let state = SnapshotMutableState::new_in_arc(0, Arc::new(NeverEqual));
    let observed: Arc<dyn StateObject> = state.clone();
    let notifications = Rc::new(RefCell::new(Vec::new()));
    let discarded = notifications.clone();
    observer.observe_reads(
        TestScope("changing"),
        move |_| discarded.borrow_mut().push(0),
        || {},
    );
    assert_eq!(Rc::strong_count(&notifications), 1);
    assert_eq!(observer.debug_stats().scopes_len, 0);
    for generation in 1..=2 {
        observer.begin_frame();
        let delivered = notifications.clone();
        observer.observe_reads(
            TestScope("changing"),
            move |_| delivered.borrow_mut().push(generation),
            || {
                observer.notify_changes(std::slice::from_ref(&observed));
                let _ = state.get();
            },
        );
        observer.notify_changes(std::slice::from_ref(&observed));
    }
    assert_eq!(*notifications.borrow(), vec![1, 2, 2]);
    observer.clear(&TestScope("changing"));
    assert_eq!(Rc::strong_count(&notifications), 1);
}

#[test]
fn callback_captures_are_released_on_replacement_and_clear() {
    let _guard = reset_runtime();
    let state = SnapshotMutableState::new_in_arc(0, Arc::new(NeverEqual));
    let observer = SnapshotStateObserver::new(|callback| callback());
    let owners = [Rc::new(Cell::new(0)), Rc::new(Cell::new(0))];
    for owner in &owners {
        let captured = owner.clone();
        observer.observe_reads(
            TestScope("owner"),
            move |_| captured.set(captured.get() + 1),
            || {
                let _ = state.get();
            },
        );
        assert_eq!(Rc::strong_count(owner), 2);
    }
    assert_eq!(Rc::strong_count(&owners[0]), 1);
    observer.clear(&TestScope("owner"));
    assert_eq!(Rc::strong_count(&owners[1]), 1);
}

#[test]
fn notifies_scope_when_state_changes() {
    let _guard = reset_runtime();

    let state = SnapshotMutableState::new_in_arc(0, Arc::new(NeverEqual));
    let triggered = Rc::new(Cell::new(0));
    let observer_trigger = triggered.clone();

    let observer = SnapshotStateObserver::new(|callback| callback());
    observer.start();

    let scope = TestScope("scope");
    observer.observe_reads(
        scope,
        move |_| {
            observer_trigger.set(observer_trigger.get() + 1);
        },
        || {
            let _ = state.get();
        },
    );

    let snapshot = take_mutable_snapshot(None, None);
    snapshot.enter(|| {
        state.set(1);
    });
    snapshot.apply().check();

    assert_eq!(triggered.get(), 1);
    observer.stop();
}

#[test]
fn clear_removes_scope_observation() {
    let _guard = reset_runtime();

    let state = SnapshotMutableState::new_in_arc(0, Arc::new(NeverEqual));
    let triggered = Rc::new(Cell::new(0));
    let observer_trigger = triggered.clone();

    let observer = SnapshotStateObserver::new(|callback| callback());
    observer.start();

    let scope = TestScope("scope");
    observer.observe_reads(
        scope.clone(),
        move |_| {
            observer_trigger.set(observer_trigger.get() + 1);
        },
        || {
            let _ = state.get();
        },
    );

    observer.clear(&scope);

    let snapshot = take_mutable_snapshot(None, None);
    snapshot.enter(|| {
        state.set(1);
    });
    snapshot.apply().check();

    assert_eq!(triggered.get(), 0);
    observer.stop();
}

#[test]
fn repeated_owned_scope_observations_reuse_the_same_entry() {
    let _guard = reset_runtime();

    let state = SnapshotMutableState::new_in_arc(0, Arc::new(NeverEqual));
    let observer = SnapshotStateObserver::new(|callback| callback());
    let scope = TestScope("scope");

    observer.observe_reads(
        scope.clone(),
        |_| {},
        || {
            let _ = state.get();
        },
    );
    observer.observe_reads(
        scope,
        |_| {},
        || {
            let _ = state.get();
        },
    );

    let stats = observer.debug_stats();
    assert_eq!(stats.scopes_len, 1);
    assert_eq!(stats.fast_scopes_len, 0);
}

#[test]
fn with_no_observations_skips_reads() {
    let _guard = reset_runtime();

    let state = SnapshotMutableState::new_in_arc(0, Arc::new(NeverEqual));
    let triggered = Rc::new(Cell::new(0));
    let observer_trigger = triggered.clone();

    let observer = SnapshotStateObserver::new(|callback| callback());
    observer.start();

    let scope = TestScope("scope");
    observer.observe_reads(
        scope,
        move |_| {
            observer_trigger.set(observer_trigger.get() + 1);
        },
        || {
            observer.with_no_observations(|| {
                let _ = state.get();
            });
        },
    );

    let snapshot = take_mutable_snapshot(None, None);
    snapshot.enter(|| {
        state.set(1);
    });
    snapshot.apply().check();

    assert_eq!(triggered.get(), 0);
    observer.stop();
}

#[test]
fn recycled_observation_refreshes_snapshot_state() {
    let _guard = reset_runtime();
    let state = SnapshotMutableState::new_in_arc(0, Arc::new(NeverEqual));
    let observer = SnapshotStateObserver::new(|callback| callback());
    let mut allocation = None;
    for value in 1..=3 {
        let parent = take_mutable_snapshot(None, None);
        parent.enter(|| {
            state.set(value);
            let expected = crate::snapshot_v2::current_snapshot().unwrap();
            observer.inner.run_with_read_observer(|| {
                let crate::snapshot_v2::AnySnapshot::TransparentMutable(current) =
                    crate::snapshot_v2::current_snapshot().unwrap()
                else {
                    panic!("expected an observation snapshot");
                };
                assert_eq!(current.snapshot_id(), expected.snapshot_id());
                assert_eq!(current.invalid(), expected.invalid());
                assert!(!current.is_disposed());
                assert!(!current.has_pending_changes());
                assert_eq!(state.get(), value);
                let address = Arc::as_ptr(&current) as usize;
                assert_eq!(*allocation.get_or_insert(address), address);
            });
        });
        parent.apply().check();
    }
}

#[test]
fn recycled_observation_preserves_escaped_snapshots() {
    let _guard = reset_runtime();
    let observer = SnapshotStateObserver::new(|callback| callback());
    let escaped = observer
        .inner
        .run_with_read_observer(|| crate::snapshot_v2::current_snapshot().unwrap());
    let id = escaped.snapshot_id();
    observer.inner.run_with_read_observer(|| {
        let current = crate::snapshot_v2::current_snapshot().unwrap();
        let crate::snapshot_v2::AnySnapshot::TransparentMutable(escaped) = &escaped else {
            panic!("expected an observation snapshot");
        };
        assert!(!current.is_same_transparent(escaped));
        assert_eq!(escaped.snapshot_id(), id);
        assert!(escaped.is_disposed());
    });
    let weak = observer.inner.run_with_read_observer(|| {
        let crate::snapshot_v2::AnySnapshot::TransparentMutable(current) =
            crate::snapshot_v2::current_snapshot().unwrap()
        else {
            panic!("expected an observation snapshot");
        };
        Arc::downgrade(&current)
    });
    assert!(weak.upgrade().is_none());
    observer.inner.run_with_read_observer(|| {
        assert!(weak.upgrade().is_none());
    });
}

#[test]
fn recycled_observation_does_not_retain_written_state() {
    let _guard = reset_runtime();
    let observer = SnapshotStateObserver::new(|callback| callback());
    let state = SnapshotMutableState::new_in_arc(0, Arc::new(NeverEqual));
    let owners = Arc::strong_count(&state);
    observer.inner.run_with_read_observer(|| {
        let current = crate::snapshot_v2::current_snapshot().unwrap();
        current.record_write(state.clone());
    });
    assert_eq!(Arc::strong_count(&state), owners);
}

#[test]
fn nested_observe_reads_attributes_state_to_innermost_scope_only() {
    let _guard = reset_runtime();

    let state = SnapshotMutableState::new_in_arc(0, Arc::new(NeverEqual));
    let outer_state = SnapshotMutableState::new_in_arc(0, Arc::new(NeverEqual));
    let outer_triggered = Rc::new(Cell::new(0));
    let inner_triggered = Rc::new(Cell::new(0));

    let observer = SnapshotStateObserver::new(|callback| callback());
    observer.start();

    let outer_scope = TestScope("outer");
    let inner_scope = TestScope("inner");
    observer.observe_reads(
        outer_scope,
        {
            let outer_triggered = Rc::clone(&outer_triggered);
            move |_| outer_triggered.set(outer_triggered.get() + 1)
        },
        || {
            let _ = outer_state.get();
            observer.observe_reads(
                inner_scope.clone(),
                {
                    let inner_triggered = Rc::clone(&inner_triggered);
                    move |_| inner_triggered.set(inner_triggered.get() + 1)
                },
                || {
                    let _ = state.get();
                },
            );
        },
    );

    let snapshot = take_mutable_snapshot(None, None);
    snapshot.enter(|| {
        state.set(1);
    });
    snapshot.apply().check();

    assert_eq!(outer_triggered.get(), 0);
    assert_eq!(inner_triggered.get(), 1);
    let snapshot = take_mutable_snapshot(None, None);
    snapshot.enter(|| outer_state.set(1));
    snapshot.apply().check();
    assert_eq!(outer_triggered.get(), 1);
    assert_eq!(inner_triggered.get(), 1);
    observer.stop();
}

#[test]
fn unwound_observation_does_not_leak_reads_into_reused_storage() {
    let _guard = reset_runtime();
    let abandoned = SnapshotMutableState::new_in_arc(0, Arc::new(NeverEqual));
    let live = SnapshotMutableState::new_in_arc(0, Arc::new(NeverEqual));
    let triggered = Rc::new(Cell::new(0));
    let observer = SnapshotStateObserver::new(|callback| callback());
    observer.start();

    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        observer.observe_reads(
            TestScope("abandoned"),
            |_| {},
            || {
                let _ = abandoned.get();
                panic!("abandon observation");
            },
        );
    }));
    assert!(result.is_err());
    observer.observe_reads(
        TestScope("live"),
        {
            let triggered = Rc::clone(&triggered);
            move |_| triggered.set(triggered.get() + 1)
        },
        || {
            let _ = live.get();
        },
    );

    let snapshot = take_mutable_snapshot(None, None);
    snapshot.enter(|| abandoned.set(1));
    snapshot.apply().check();
    assert_eq!(triggered.get(), 0);
    let snapshot = take_mutable_snapshot(None, None);
    snapshot.enter(|| live.set(1));
    snapshot.apply().check();
    assert_eq!(triggered.get(), 1);
    observer.stop();
}

#[test]
fn clearing_one_scope_keeps_shared_state_registered_for_other_scope() {
    let _guard = reset_runtime();

    let state = SnapshotMutableState::new_in_arc(0, Arc::new(NeverEqual));
    let first_triggered = Rc::new(Cell::new(0));
    let second_triggered = Rc::new(Cell::new(0));

    let observer = SnapshotStateObserver::new(|callback| callback());
    observer.start();

    let first_scope = TestScope("first");
    let second_scope = TestScope("second");
    observer.observe_reads(
        first_scope.clone(),
        {
            let first_triggered = Rc::clone(&first_triggered);
            move |_| first_triggered.set(first_triggered.get() + 1)
        },
        || {
            let _ = state.get();
        },
    );
    observer.observe_reads(
        second_scope,
        {
            let second_triggered = Rc::clone(&second_triggered);
            move |_| second_triggered.set(second_triggered.get() + 1)
        },
        || {
            let _ = state.get();
        },
    );

    observer.clear(&first_scope);

    let snapshot = take_mutable_snapshot(None, None);
    snapshot.enter(|| {
        state.set(1);
    });
    snapshot.apply().check();

    assert_eq!(first_triggered.get(), 0);
    assert_eq!(second_triggered.get(), 1);
    observer.stop();
}

#[test]
fn shared_state_notifies_scopes_in_registration_order() {
    let _guard = reset_runtime();

    let state = SnapshotMutableState::new_in_arc(0, Arc::new(NeverEqual));
    let notifications = Rc::new(RefCell::new(Vec::new()));

    let observer = SnapshotStateObserver::new(|callback| callback());
    observer.start();

    observer.observe_reads(
        TestScope("first"),
        {
            let notifications = Rc::clone(&notifications);
            move |_| notifications.borrow_mut().push("first")
        },
        || {
            let _ = state.get();
        },
    );
    observer.observe_reads(
        TestScope("second"),
        {
            let notifications = Rc::clone(&notifications);
            move |_| notifications.borrow_mut().push("second")
        },
        || {
            let _ = state.get();
        },
    );

    let snapshot = take_mutable_snapshot(None, None);
    snapshot.enter(|| {
        state.set(1);
    });
    snapshot.apply().check();

    assert_eq!(notifications.borrow().as_slice(), &["first", "second"]);
    observer.stop();
}

#[test]
fn stateless_recompose_scope_does_not_retain_observer_entry() {
    let _guard = reset_runtime();

    let observer = SnapshotStateObserver::new(|callback| callback());
    let runtime = crate::TestRuntime::new();
    let scope = RecomposeScope::new_for_test(runtime.handle());

    observer.observe_reads(scope, |_| {}, || {});

    let stats = observer.debug_stats();
    assert_eq!(stats.scopes_len, 0);
    assert_eq!(stats.fast_scopes_len, 0);
    assert_eq!(stats.stateless_scope_count, 0);
}

#[test]
fn scope_that_stops_reading_state_is_removed_immediately() {
    let _guard = reset_runtime();

    let state = SnapshotMutableState::new_in_arc(0, Arc::new(NeverEqual));
    let observer = SnapshotStateObserver::new(|callback| callback());
    let runtime = crate::TestRuntime::new();
    let scope = RecomposeScope::new_for_test(runtime.handle());
    let triggered = Rc::new(Cell::new(0));
    let observer_trigger = Rc::clone(&triggered);

    observer.observe_reads(
        scope.clone(),
        move |_| observer_trigger.set(observer_trigger.get() + 1),
        || {
            let _ = state.get();
        },
    );

    let after_stateful = observer.debug_stats();
    assert_eq!(after_stateful.scopes_len, 1);
    assert_eq!(after_stateful.fast_scopes_len, 1);

    observer.observe_reads(scope, |_| {}, || {});

    let after_stateless = observer.debug_stats();
    assert_eq!(after_stateless.scopes_len, 0);
    assert_eq!(after_stateless.fast_scopes_len, 0);

    let snapshot = take_mutable_snapshot(None, None);
    snapshot.enter(|| {
        state.set(1);
    });
    snapshot.apply().check();

    assert_eq!(triggered.get(), 0);
}

#[test]
fn begin_frame_prunes_dropped_recompose_scope_entries() {
    let _guard = reset_runtime();

    let state = SnapshotMutableState::new_in_arc(0, Arc::new(NeverEqual));
    let observer = SnapshotStateObserver::new(|callback| callback());
    let runtime = crate::TestRuntime::new();
    let scope = RecomposeScope::new_for_test(runtime.handle());

    observer.observe_reads(
        scope.clone(),
        |_| {},
        || {
            let _ = state.get();
        },
    );

    let before_prune = observer.debug_stats();
    assert_eq!(before_prune.scopes_len, 1);
    assert_eq!(before_prune.fast_scopes_len, 1);

    drop(scope);
    observer.begin_frame();

    let after_prune = observer.debug_stats();
    assert_eq!(after_prune.scopes_len, 0);
    assert_eq!(after_prune.fast_scopes_len, 0);
}
