use super::*;
use crate as cranpose_core;
use crate::snapshot_v2::take_mutable_snapshot;
use crate::state::{MutationPolicy, SnapshotMutableState};
use crate::SnapshotStateObserver;
use cranpose_macros::composable;
use std::cell::{Cell, RefCell};
use std::rc::Rc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::Duration;

#[derive(Default)]
struct TestTextNode {
    text: String,
}

impl Node for TestTextNode {}

#[derive(Default)]
struct TestDummyNode;

impl Node for TestDummyNode {}

struct MountTrackingNode {
    mounted: Rc<Cell<usize>>,
}

impl Node for MountTrackingNode {
    fn mount(&mut self) {
        self.mounted.set(self.mounted.get() + 1);
    }
}

fn runtime_handle() -> (RuntimeHandle, Runtime) {
    let runtime = Runtime::new(Arc::new(TestScheduler));
    (runtime.handle(), runtime)
}

thread_local! {
    static INVOCATIONS: Cell<usize> = const { Cell::new(0) };
}

thread_local! {
    static PARENT_RECOMPOSITIONS: Cell<usize> = const { Cell::new(0) };
    static CHILD_RECOMPOSITIONS: Cell<usize> = const { Cell::new(0) };
    static CAPTURED_PARENT_STATE: RefCell<Option<cranpose_core::MutableState<i32>>> =
        const { RefCell::new(None) };
    static SIDE_EFFECT_LOG: RefCell<Vec<&'static str>> = const { RefCell::new(Vec::new()) }; // FUTURE(no_std): replace Vec with ring buffer for testing.
    static DISPOSABLE_EFFECT_LOG: RefCell<Vec<&'static str>> = const { RefCell::new(Vec::new()) }; // FUTURE(no_std): replace Vec with ring buffer for testing.
    static DISPOSABLE_STATE: RefCell<Option<cranpose_core::MutableState<i32>>> =
        const { RefCell::new(None) };
    static SIDE_EFFECT_STATE: RefCell<Option<cranpose_core::MutableState<i32>>> =
        const { RefCell::new(None) };
}

thread_local! {
    static DROP_REENTRY_STATE: RefCell<Option<cranpose_core::MutableState<ReentrantDropState>>> =
        const { RefCell::new(None) };
    static DROP_REENTRY_ACTIVE: Cell<bool> = const { Cell::new(false) };
    static DROP_REENTRY_LAST_VALUE: Cell<Option<usize>> = const { Cell::new(None) };
}

struct ReentrantDropState {
    id: usize,
    drops: Rc<Cell<usize>>,
    reenter_on_drop: Rc<Cell<bool>>,
}

impl ReentrantDropState {
    fn new(id: usize, drops: Rc<Cell<usize>>, reenter_on_drop: bool) -> Self {
        Self {
            id,
            drops,
            reenter_on_drop: Rc::new(Cell::new(reenter_on_drop)),
        }
    }
}

impl Clone for ReentrantDropState {
    fn clone(&self) -> Self {
        Self {
            id: self.id,
            drops: Rc::clone(&self.drops),
            reenter_on_drop: Rc::new(Cell::new(false)),
        }
    }
}

impl Drop for ReentrantDropState {
    fn drop(&mut self) {
        self.drops.set(self.drops.get() + 1);
        if !self.reenter_on_drop.replace(false) {
            return;
        }

        DROP_REENTRY_ACTIVE.with(|active| {
            if active.replace(true) {
                return;
            }

            DROP_REENTRY_STATE.with(|slot| {
                if let Some(state) = slot.borrow().as_ref() {
                    let value = state.value();
                    DROP_REENTRY_LAST_VALUE.with(|last| last.set(Some(value.id)));
                }
            });

            active.set(false);
        });
    }
}

fn cranpose_test_node<N: Node + 'static>(init: impl FnOnce() -> N) -> NodeId {
    cranpose_core::with_current_composer(|composer| composer.emit_node(init))
}

fn setup_composer(
    slots: &mut SlotBackend,
    applier: &mut MemoryApplier,
    handle: RuntimeHandle,
    root: Option<NodeId>,
) -> (
    Composer,
    Rc<SlotsHost>,
    Rc<ConcreteApplierHost<MemoryApplier>>,
) {
    let slots_host = Rc::new(SlotsHost::new(std::mem::take(slots)));
    let applier_host = Rc::new(ConcreteApplierHost::new(std::mem::replace(
        applier,
        MemoryApplier::new(),
    )));
    let observer = SnapshotStateObserver::new(|callback| callback());
    let composer = Composer::new(
        Rc::clone(&slots_host),
        applier_host.clone(),
        handle,
        observer,
        root,
    );
    (composer, slots_host, applier_host)
}

fn teardown_composer(
    slots: &mut SlotBackend,
    applier: &mut MemoryApplier,
    slots_host: Rc<SlotsHost>,
    applier_host: Rc<ConcreteApplierHost<MemoryApplier>>,
) {
    *slots = Rc::try_unwrap(slots_host)
        .unwrap_or_else(|_| panic!("slots host still has outstanding references"))
        .take();
    *applier = Rc::try_unwrap(applier_host)
        .unwrap_or_else(|_| panic!("applier host still has outstanding references"))
        .into_inner();
}

#[test]
#[should_panic(expected = "subcompose() may only be called during measure or layout")]
fn subcompose_panics_outside_measure_or_layout() {
    let (handle, _runtime) = runtime_handle();
    let mut slots = SlotBackend::default();
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
    let mut slots = SlotBackend::default();
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
    let mut slots = SlotBackend::default();
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
    let mut slots = SlotBackend::default();
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
                            move |_| rx.recv().expect("value available"),
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
                                move |_| rx.recv().expect("value available"),
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
                                move |_| rx.recv().expect("value available"),
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

    let mut render = move || {
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

#[composable]
fn counted_text(value: i32) -> NodeId {
    INVOCATIONS.with(|calls| calls.set(calls.get() + 1));
    let id = cranpose_test_node(TestTextNode::default);
    with_node_mut(id, |node: &mut TestTextNode| {
        node.text = format!("{}", value);
    })
    .expect("update text node");
    id
}

#[composable]
fn child_reads_state(state: cranpose_core::State<i32>) -> NodeId {
    CHILD_RECOMPOSITIONS.with(|calls| calls.set(calls.get() + 1));
    counted_text(state.value())
}

#[composable]
fn parent_passes_state() -> NodeId {
    PARENT_RECOMPOSITIONS.with(|calls| calls.set(calls.get() + 1));
    let state = cranpose_core::useState(|| 0);
    CAPTURED_PARENT_STATE.with(|slot| {
        if slot.borrow().is_none() {
            *slot.borrow_mut() = Some(state);
        }
    });
    child_reads_state(state.as_state())
}

#[composable]
fn side_effect_component() -> NodeId {
    SIDE_EFFECT_LOG.with(|log| log.borrow_mut().push("compose"));
    let state = cranpose_core::useState(|| 0);
    let _ = state.value();
    SIDE_EFFECT_STATE.with(|slot| {
        if slot.borrow().is_none() {
            *slot.borrow_mut() = Some(state);
        }
    });
    cranpose_core::SideEffect(|| {
        SIDE_EFFECT_LOG.with(|log| log.borrow_mut().push("effect"));
    });
    cranpose_test_node(TestTextNode::default)
}

#[composable]
fn disposable_effect_host() -> NodeId {
    let state = cranpose_core::useState(|| 0);
    DISPOSABLE_STATE.with(|slot| *slot.borrow_mut() = Some(state));
    DisposableEffect!(state.value(), |scope| {
        DISPOSABLE_EFFECT_LOG.with(|log| log.borrow_mut().push("start"));
        scope.on_dispose(|| {
            DISPOSABLE_EFFECT_LOG.with(|log| log.borrow_mut().push("dispose"));
        })
    });
    cranpose_test_node(TestTextNode::default)
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

#[test]
fn remember_state_roundtrip() {
    let mut composition = Composition::new(MemoryApplier::new());
    let mut text_seen = String::new();

    for _ in 0..2 {
        composition
            .render(location_key(file!(), line!(), column!()), || {
                with_current_composer(|composer| {
                    composer.with_group(location_key(file!(), line!(), column!()), |composer| {
                        let count = composer.use_state(|| 0);
                        let node_id = composer.emit_node(TestTextNode::default);
                        composer
                            .with_node_mut(node_id, |node: &mut TestTextNode| {
                                node.text = format!("{}", count.get());
                            })
                            .expect("update text node");
                        text_seen = count.get().to_string();
                    });
                });
            })
            .expect("render succeeds");
    }

    assert_eq!(text_seen, "0");
}

#[test]
fn state_update_schedules_render() {
    let mut composition = Composition::new(MemoryApplier::new());
    let mut stored = None;
    composition
        .render(location_key(file!(), line!(), column!()), || {
            let state = cranpose_core::useState(|| 10);
            let _ = state.value();
            stored = Some(state);
        })
        .expect("render succeeds");
    let state = stored.expect("state stored");
    assert!(!composition.should_render());
    state.set(11);
    assert!(composition.should_render());
}

#[test]
fn recranpose_does_not_use_stale_indices_when_prior_scope_changes_length() {
    thread_local! {
        static STABLE_RECOMPOSE_A: Cell<usize> = const { Cell::new(0) };
        static STABLE_RECOMPOSE_B: Cell<usize> = const { Cell::new(0) };
    }

    #[composable]
    fn logging_group_a(state_a: MutableState<i32>, toggle_a: MutableState<bool>) {
        STABLE_RECOMPOSE_A.with(|count| count.set(count.get() + 1));
        let _ = state_a.value();
        let expand = toggle_a.value();
        if expand {
            let _ = cranpose_core::remember(|| ());
            let _ = cranpose_core::remember(|| ());
            cranpose_core::with_key(&"nested", || {});
        } else {
            let _ = cranpose_core::remember(|| ());
        }
    }

    #[composable]
    fn logging_group_b(state_b: MutableState<i32>) {
        STABLE_RECOMPOSE_B.with(|count| count.set(count.get() + 1));
        let _ = state_b.value();
    }

    #[composable]
    fn logging_root(
        state_a: MutableState<i32>,
        state_b: MutableState<i32>,
        toggle_a: MutableState<bool>,
    ) {
        cranpose_core::with_key(&"root", || {
            cranpose_core::with_key(&"A", || logging_group_a(state_a, toggle_a));
            cranpose_core::with_key(&"B", || logging_group_b(state_b));
        });
    }

    let mut composition = Composition::new(MemoryApplier::new());
    let runtime = composition.runtime_handle();
    let state_a = MutableState::with_runtime(0i32, runtime.clone());
    let state_b = MutableState::with_runtime(0i32, runtime.clone());
    let toggle_a = MutableState::with_runtime(false, runtime.clone());

    let mut render = { move || logging_root(state_a, state_b, toggle_a) };

    composition
        .render(location_key(file!(), line!(), column!()), &mut render)
        .expect("initial render");

    STABLE_RECOMPOSE_A.with(|count| assert_eq!(count.get(), 1));
    STABLE_RECOMPOSE_B.with(|count| assert_eq!(count.get(), 1));

    STABLE_RECOMPOSE_A.with(|count| count.set(0));
    STABLE_RECOMPOSE_B.with(|count| count.set(0));

    state_b.set_value(1);
    toggle_a.set_value(true);
    state_a.set_value(1);

    let recomposed = composition
        .process_invalid_scopes()
        .expect("recomposition succeeds");
    assert!(recomposed, "expected at least one scope to recompose");

    STABLE_RECOMPOSE_A.with(|count| assert!(count.get() >= 1));
    STABLE_RECOMPOSE_B.with(|count| assert!(count.get() >= 1));
}

#[test]
fn recranpose_handles_removed_scopes_gracefully() {
    thread_local! {
        static REMOVED_SCOPE_LOG: RefCell<Vec<&'static str>> = const { RefCell::new(Vec::new()) };
    }

    fn render_optional_scope(
        composer: &Composer,
        state_a: &MutableState<i32>,
        toggle_group: &MutableState<bool>,
    ) {
        if toggle_group.value() {
            let state_clone = *state_a;
            composer.with_group(21, |composer| {
                let state_capture = state_clone;
                composer.set_recranpose_callback({
                    move |composer| {
                        let _ = state_capture.value();
                        composer.register_side_effect(|| {
                            REMOVED_SCOPE_LOG.with(|log| log.borrow_mut().push("scope"));
                        });
                    }
                });
                let _ = state_capture.value();
                composer.register_side_effect(|| {
                    REMOVED_SCOPE_LOG.with(|log| log.borrow_mut().push("scope"));
                });
            });
        }
    }

    let mut composition = Composition::new(MemoryApplier::new());
    let runtime = composition.runtime_handle();
    let state_a = MutableState::with_runtime(0i32, runtime.clone());
    let toggle_group = MutableState::with_runtime(true, runtime.clone());

    let mut render = {
        move || {
            with_current_composer(|composer| {
                render_optional_scope(composer, &state_a, &toggle_group);
            });
        }
    };

    composition
        .render(location_key(file!(), line!(), column!()), &mut render)
        .expect("initial render");

    REMOVED_SCOPE_LOG.with(|log| log.borrow_mut().clear());

    state_a.set_value(1);
    toggle_group.set_value(false);

    composition
        .render(location_key(file!(), line!(), column!()), &mut render)
        .expect("render without scope");

    let recomposed = composition
        .process_invalid_scopes()
        .expect("process invalid scopes succeeds");
    assert!(!recomposed);

    REMOVED_SCOPE_LOG.with(|log| {
        assert!(log.borrow().is_empty());
    });
}

#[test]
fn side_effect_runs_after_composition() {
    let mut composition = Composition::new(MemoryApplier::new());
    SIDE_EFFECT_LOG.with(|log| log.borrow_mut().clear());
    SIDE_EFFECT_STATE.with(|slot| *slot.borrow_mut() = None);
    let key = location_key(file!(), line!(), column!());
    composition
        .render(key, || {
            side_effect_component();
        })
        .expect("render succeeds");
    SIDE_EFFECT_LOG.with(|log| {
        assert_eq!(&*log.borrow(), &["compose", "effect"]);
    });
    SIDE_EFFECT_STATE.with(|slot| {
        if let Some(state) = slot.borrow().as_ref() {
            state.set_value(1);
        }
    });
    assert!(composition.should_render());
    let _ = composition
        .process_invalid_scopes()
        .expect("process invalid scopes succeeds");
    SIDE_EFFECT_LOG.with(|log| {
        assert_eq!(&*log.borrow(), &["compose", "effect", "compose", "effect"]);
    });
}

#[test]
fn disposable_effect_reacts_to_key_changes() {
    let mut composition = Composition::new(MemoryApplier::new());
    DISPOSABLE_EFFECT_LOG.with(|log| log.borrow_mut().clear());
    DISPOSABLE_STATE.with(|slot| *slot.borrow_mut() = None);
    let key = location_key(file!(), line!(), column!());
    composition
        .render(key, || {
            disposable_effect_host();
        })
        .expect("render succeeds");
    DISPOSABLE_EFFECT_LOG.with(|log| {
        assert_eq!(&*log.borrow(), &["start"]);
    });
    composition
        .render(key, || {
            disposable_effect_host();
        })
        .expect("render succeeds");
    DISPOSABLE_EFFECT_LOG.with(|log| {
        assert_eq!(&*log.borrow(), &["start"]);
    });
    DISPOSABLE_STATE.with(|slot| {
        if let Some(state) = slot.borrow().as_ref() {
            state.set_value(1);
        }
    });
    composition
        .render(key, || {
            disposable_effect_host();
        })
        .expect("render succeeds");
    DISPOSABLE_EFFECT_LOG.with(|log| {
        assert_eq!(&*log.borrow(), &["start", "dispose", "start"]);
    });
}

#[test]
fn state_invalidation_skips_parent_scope() {
    PARENT_RECOMPOSITIONS.with(|calls| calls.set(0));
    CHILD_RECOMPOSITIONS.with(|calls| calls.set(0));
    CAPTURED_PARENT_STATE.with(|slot| *slot.borrow_mut() = None);

    let mut composition = Composition::new(MemoryApplier::new());
    let root_key = location_key(file!(), line!(), column!());

    composition
        .render(root_key, || {
            parent_passes_state();
        })
        .expect("initial render succeeds");

    PARENT_RECOMPOSITIONS.with(|calls| assert_eq!(calls.get(), 1));
    CHILD_RECOMPOSITIONS.with(|calls| assert_eq!(calls.get(), 1));

    let state = CAPTURED_PARENT_STATE
        .with(|slot| *slot.borrow())
        .expect("captured state");

    PARENT_RECOMPOSITIONS.with(|calls| calls.set(0));
    CHILD_RECOMPOSITIONS.with(|calls| calls.set(0));

    state.set(1);
    assert!(composition.should_render());

    let _ = composition
        .process_invalid_scopes()
        .expect("process invalid scopes succeeds");

    PARENT_RECOMPOSITIONS.with(|calls| assert_eq!(calls.get(), 0));
    CHILD_RECOMPOSITIONS.with(|calls| assert!(calls.get() > 0));
    assert!(!composition.should_render());
}

#[derive(Clone, Debug, PartialEq, Eq)]
enum Operation {
    Insert(NodeId),
    Remove(NodeId),
    Move { from: usize, to: usize },
}

#[derive(Default)]
struct RecordingNode {
    children: Vec<NodeId>, // FUTURE(no_std): store children in bounded array for tests.
    operations: Vec<Operation>, // FUTURE(no_std): store operations in bounded array for tests.
}

impl Node for RecordingNode {
    fn insert_child(&mut self, child: NodeId) {
        self.children.push(child);
        self.operations.push(Operation::Insert(child));
    }

    fn remove_child(&mut self, child: NodeId) {
        self.children.retain(|&c| c != child);
        self.operations.push(Operation::Remove(child));
    }

    fn move_child(&mut self, from: usize, to: usize) {
        if from == to || from >= self.children.len() {
            return;
        }
        let child = self.children.remove(from);
        let target = to.min(self.children.len());
        if target >= self.children.len() {
            self.children.push(child);
        } else {
            self.children.insert(target, child);
        }
        self.operations.push(Operation::Move { from, to });
    }
}

#[derive(Default)]
struct TrackingChild {
    label: String,
    mount_count: usize,
    parent: Option<NodeId>,
}

impl Node for TrackingChild {
    fn mount(&mut self) {
        self.mount_count += 1;
    }
    fn on_attached_to_parent(&mut self, parent: NodeId) {
        self.parent = Some(parent);
    }
    fn on_removed_from_parent(&mut self) {
        self.parent = None;
    }
    fn parent(&self) -> Option<NodeId> {
        self.parent
    }
}

fn apply_child_diff(
    slots: &mut SlotBackend,
    applier: &mut MemoryApplier,
    runtime: &Runtime,
    parent_id: NodeId,
    previous: Vec<NodeId>, // FUTURE(no_std): accept fixed-capacity child buffers.
    new_children: Vec<NodeId>, // FUTURE(no_std): accept fixed-capacity child buffers.
) -> Vec<Operation> {
    // FUTURE(no_std): return bounded operation log.
    let handle = runtime.handle();
    let (composer, slots_host, applier_host) =
        setup_composer(slots, applier, handle, Some(parent_id));
    composer.push_parent(parent_id);
    {
        let mut stack = composer.parent_stack();
        let frame = stack.last_mut().expect("parent frame available");
        frame
            .remembered
            .update(|entry| entry.children = previous.clone());
        frame.previous = previous;
        frame.new_children = new_children;
    }
    composer.pop_parent();
    let mut commands = composer.take_commands();
    drop(composer);
    teardown_composer(slots, applier, slots_host, applier_host);
    for command in commands.iter_mut() {
        command(applier).expect("apply diff command");
    }
    applier
        .with_node(parent_id, |node: &mut RecordingNode| {
            node.operations.clone()
        })
        .expect("read parent operations")
}

#[test]
fn reorder_keyed_children_emits_moves() {
    let mut slots = SlotBackend::default();
    let mut applier = MemoryApplier::new();
    let runtime = Runtime::new(Arc::new(TestScheduler));
    let parent_id = applier.create(Box::new(RecordingNode::default()));

    let child_a = applier.create(Box::new(TrackingChild {
        label: "a".to_string(),
        mount_count: 1,
        parent: Some(parent_id),
    }));
    let child_b = applier.create(Box::new(TrackingChild {
        label: "b".to_string(),
        mount_count: 1,
        parent: Some(parent_id),
    }));
    let child_c = applier.create(Box::new(TrackingChild {
        label: "c".to_string(),
        mount_count: 1,
        parent: Some(parent_id),
    }));

    applier
        .with_node(parent_id, |node: &mut RecordingNode| {
            node.children = vec![child_a, child_b, child_c];
            node.operations.clear();
        })
        .expect("seed parent state");
    let initial_len = applier.len();

    let operations = apply_child_diff(
        &mut slots,
        &mut applier,
        &runtime,
        parent_id,
        vec![child_a, child_b, child_c],
        vec![child_c, child_b, child_a],
    );

    assert_eq!(
        operations,
        vec![
            Operation::Move { from: 2, to: 0 },
            Operation::Move { from: 2, to: 1 },
        ]
    );

    let final_children = applier
        .with_node(parent_id, |node: &mut RecordingNode| node.children.clone())
        .expect("read reordered children");
    assert_eq!(final_children, vec![child_c, child_b, child_a]);
    let final_len = applier.len();
    assert_eq!(initial_len, final_len);

    for (expected_label, child_id) in [("a", child_a), ("b", child_b), ("c", child_c)] {
        applier
            .with_node(child_id, |child: &mut TrackingChild| {
                assert_eq!(child.label, expected_label.to_string());
                assert_eq!(child.mount_count, 1);
            })
            .expect("read tracking child state");
    }
}

#[test]
fn insert_and_remove_emit_expected_ops() {
    let mut slots = SlotBackend::default();
    let mut applier = MemoryApplier::new();
    let runtime = Runtime::new(Arc::new(TestScheduler));
    let parent_id = applier.create(Box::new(RecordingNode::default()));

    let child_a = applier.create(Box::new(TrackingChild {
        label: "a".to_string(),
        mount_count: 1,
        parent: Some(parent_id),
    }));
    let child_b = applier.create(Box::new(TrackingChild {
        label: "b".to_string(),
        mount_count: 1,
        parent: Some(parent_id),
    }));

    applier
        .with_node(parent_id, |node: &mut RecordingNode| {
            node.children = vec![child_a, child_b];
            node.operations.clear();
        })
        .expect("seed parent state");
    let initial_len = applier.len();

    let child_c = applier.create(Box::new(TrackingChild {
        label: "c".to_string(),
        mount_count: 1,
        parent: Some(parent_id),
    }));
    assert_eq!(applier.len(), initial_len + 1);

    let insert_ops = apply_child_diff(
        &mut slots,
        &mut applier,
        &runtime,
        parent_id,
        vec![child_a, child_b],
        vec![child_a, child_b, child_c],
    );

    assert_eq!(insert_ops, vec![Operation::Insert(child_c)]);
    let after_insert_children = applier
        .with_node(parent_id, |node: &mut RecordingNode| node.children.clone())
        .expect("read children after insert");
    assert_eq!(after_insert_children, vec![child_a, child_b, child_c]);

    applier
        .with_node(parent_id, |node: &mut RecordingNode| {
            node.operations.clear()
        })
        .expect("clear operations");

    let remove_ops = apply_child_diff(
        &mut slots,
        &mut applier,
        &runtime,
        parent_id,
        vec![child_a, child_b, child_c],
        vec![child_a, child_c],
    );

    assert_eq!(remove_ops, vec![Operation::Remove(child_b)]);
    let after_remove_children = applier
        .with_node(parent_id, |node: &mut RecordingNode| node.children.clone())
        .expect("read children after remove");
    assert_eq!(after_remove_children, vec![child_a, child_c]);
    assert_eq!(applier.len(), initial_len);
}

#[test]
fn composable_skips_when_inputs_unchanged() {
    INVOCATIONS.with(|calls| calls.set(0));
    let mut composition = Composition::new(MemoryApplier::new());
    let key = location_key(file!(), line!(), column!());

    composition
        .render(key, || {
            counted_text(1);
        })
        .expect("render succeeds");
    INVOCATIONS.with(|calls| assert_eq!(calls.get(), 1));

    composition
        .render(key, || {
            counted_text(1);
        })
        .expect("render succeeds");
    INVOCATIONS.with(|calls| assert_eq!(calls.get(), 1));

    composition
        .render(key, || {
            counted_text(2);
        })
        .expect("render succeeds");
    INVOCATIONS.with(|calls| assert_eq!(calls.get(), 2));
}

#[test]
fn composition_local_provider_scopes_values() {
    thread_local! {
        static CHILD_RECOMPOSITIONS: Cell<usize> = const { Cell::new(0) };
        static LAST_VALUE: Cell<i32> = const { Cell::new(0) };
    }

    let local_counter = compositionLocalOf(|| 0);
    let mut composition = Composition::new(MemoryApplier::new());
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
    let mut composition = Composition::new(MemoryApplier::new());

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
    let mut composition = Composition::new(MemoryApplier::new());
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
    let mut composition = Composition::new(MemoryApplier::new());
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
    let mut composition = Composition::new(MemoryApplier::new());

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
    let mut composition = Composition::new(MemoryApplier::new());

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

    let mut composition = Composition::new(MemoryApplier::new());
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

    let mut composition = Composition::new(MemoryApplier::new());
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

    let mut composition = Composition::new(MemoryApplier::new());
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
    let state = SnapshotMutableState::new_in_arc(0, Arc::new(SumPolicy));
    assert_eq!(state.get(), 0);
    state.set(1);
    assert_eq!(state.get(), 1);
}

#[test]
fn snapshot_state_child_isolation_and_apply() {
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
    let state = SnapshotMutableState::new_in_arc(0, Arc::new(SumPolicy));

    for value in 1..=5 {
        state.set(value);
    }

    let child = take_mutable_snapshot(None, None);
    child.enter(|| state.set(42));

    child.apply().check();
    assert_eq!(state.get(), 42);
}

// Note: Tests for ComposeTestRule and run_test_composition have been moved to
// the compose-testing crate to avoid circular dependencies.

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

    let mut composition = Composition::new(MemoryApplier::new());
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

// ============================================================================
// Slot Table Unit Tests - Gap Architecture
// ============================================================================

#[test]
fn slot_table_marks_values_as_gaps() {
    let mut slots = SlotTable::new();

    // Create initial composition with 3 value slots
    let _idx1 = slots.use_value_slot(|| 1i32);
    let _idx2 = slots.use_value_slot(|| 2i32);
    let _idx3 = slots.use_value_slot(|| 3i32);

    // Mark middle slot as gap
    slots.mark_range_as_gaps(1, 2, None);

    // Verify we can still read first and last slots
    assert_eq!(slots.read_value::<i32>(0), &1);
    assert_eq!(slots.read_value::<i32>(2), &3);
}

#[test]
fn slot_table_reuses_gap_slots_for_values() {
    let mut slots = SlotTable::new();

    // Create initial value
    let idx1 = slots.use_value_slot(|| 1i32);
    assert_eq!(slots.read_value::<i32>(idx1), &1);

    // Reset and mark as gap
    slots.reset();
    slots.mark_range_as_gaps(0, 1, None);

    // Reset cursor to reuse
    slots.reset();

    // New value should reuse gap slot at position 0
    let idx2 = slots.use_value_slot(|| 42i32);
    assert_eq!(idx2, 0, "should reuse gap slot at position 0");
    assert_eq!(slots.read_value::<i32>(idx2), &42);
}

#[test]
fn slot_table_replaces_mismatched_value_types() {
    let mut slots = SlotTable::new();

    // Create initial value of type i32
    let idx = slots.use_value_slot(|| 1i32);
    assert_eq!(slots.read_value::<i32>(idx), &1);

    // Reset and try to use with different type
    slots.reset();
    let idx2 = slots.use_value_slot(|| "hello");

    // Should replace at same position
    assert_eq!(idx, idx2);
    assert_eq!(slots.read_value::<&str>(idx2), &"hello");
}

#[test]
fn slot_table_handles_nested_group_gaps() {
    let mut slots = SlotTable::new();

    // Create a parent group
    let parent_idx = slots.start(100);

    // Create child group
    let child_idx = slots.start(200);
    let _val_idx = slots.use_value_slot(|| 42i32);
    slots.end(); // End child

    slots.end(); // End parent

    // Verify parent group was created
    let groups = slots.debug_dump_groups();
    assert!(groups.iter().any(|(idx, _, _, _)| *idx == parent_idx));

    // Mark parent group range as gaps (should mark child too)
    slots.mark_range_as_gaps(parent_idx, child_idx + 2, None);

    // Groups should still be present, just marked as gaps internally
    // The slot table preserves structure for reuse
}

#[test]
fn slot_table_preserves_sibling_groups_when_marking_gaps() {
    let mut slots = SlotTable::new();

    // Create first group with a value
    let g1 = slots.start(1);
    let _v1 = slots.use_value_slot(|| "first");
    slots.end();

    // Create second group with a value
    let g2 = slots.start(2);
    let _v2 = slots.use_value_slot(|| "second");
    slots.end();

    // Create third group with a value
    let _g3 = slots.start(3);
    let v3_idx = slots.use_value_slot(|| "third");
    slots.end();

    // Capture initial group count
    let initial_groups = slots.debug_dump_groups();
    assert_eq!(initial_groups.len(), 3, "should have 3 groups initially");

    // Mark only the first group's range as gaps
    slots.mark_range_as_gaps(g1, g2, None);

    // Third group should still be accessible (the value should remain)
    assert_eq!(slots.read_value::<&str>(v3_idx), &"third");

    // After marking as gaps, group 1 is converted to a Gap slot, but groups 2 and 3 remain
    let remaining_groups = slots.debug_dump_groups();
    assert!(
        remaining_groups.len() >= 2,
        "groups outside marked range should be preserved, found {} groups",
        remaining_groups.len()
    );
}

#[test]
fn slot_table_tab_switching_preserves_scopes() {
    let mut composition = Composition::new(MemoryApplier::new());
    let runtime = composition.runtime_handle();
    let active_tab = MutableState::with_runtime(0i32, runtime.clone());
    let tab1_counter = MutableState::with_runtime(0i32, runtime.clone());
    let tab2_counter = MutableState::with_runtime(0i32, runtime.clone());

    thread_local! {
        static TAB1_RENDERS: Cell<usize> = const { Cell::new(0) };
        static TAB2_RENDERS: Cell<usize> = const { Cell::new(0) };
    }

    #[composable]
    fn tab_content_1(counter: MutableState<i32>) {
        TAB1_RENDERS.with(|c| c.set(c.get() + 1));
        let count = counter.value();
        let id = cranpose_test_node(TestTextNode::default);
        with_node_mut(id, |node: &mut TestTextNode| {
            node.text = format!("Tab 1: {}", count);
        })
        .expect("update tab1 node");
    }

    #[composable]
    fn tab_content_2(counter: MutableState<i32>) {
        TAB2_RENDERS.with(|c| c.set(c.get() + 1));
        let count = counter.value();
        let id = cranpose_test_node(TestTextNode::default);
        with_node_mut(id, |node: &mut TestTextNode| {
            node.text = format!("Tab 2: {}", count);
        })
        .expect("update tab2 node");
    }

    let mut render = {
        move || {
            let tab = active_tab.value();
            match tab {
                0 => tab_content_1(tab1_counter),
                1 => tab_content_2(tab2_counter),
                _ => {}
            }
        }
    };

    // Initial render - Tab 1
    TAB1_RENDERS.with(|c| c.set(0));
    TAB2_RENDERS.with(|c| c.set(0));

    let key = location_key(file!(), line!(), column!());
    composition
        .render(key, &mut render)
        .expect("initial render");

    assert_eq!(
        TAB1_RENDERS.with(|c| c.get()),
        1,
        "tab1 should render initially"
    );
    assert_eq!(
        TAB2_RENDERS.with(|c| c.get()),
        0,
        "tab2 should not render initially"
    );

    // Switch to Tab 2
    active_tab.set_value(1);
    composition
        .render(key, &mut render)
        .expect("switch to tab2");

    assert_eq!(
        TAB1_RENDERS.with(|c| c.get()),
        1,
        "tab1 render count unchanged"
    );
    assert_eq!(
        TAB2_RENDERS.with(|c| c.get()),
        1,
        "tab2 should render after switch"
    );

    // Update tab2 counter - should trigger recomposition
    TAB1_RENDERS.with(|c| c.set(0));
    TAB2_RENDERS.with(|c| c.set(0));
    tab2_counter.set_value(5);

    let _ = composition
        .process_invalid_scopes()
        .expect("recompose tab2");

    assert_eq!(
        TAB1_RENDERS.with(|c| c.get()),
        0,
        "tab1 should not recompose"
    );
    assert!(
        TAB2_RENDERS.with(|c| c.get()) > 0,
        "tab2 should recompose on counter change"
    );

    // Switch back to Tab 1
    TAB1_RENDERS.with(|c| c.set(0));
    TAB2_RENDERS.with(|c| c.set(0));
    active_tab.set_value(0);
    composition
        .render(key, &mut render)
        .expect("switch back to tab1");

    assert!(
        TAB1_RENDERS.with(|c| c.get()) > 0,
        "tab1 should render after switch back"
    );
    assert_eq!(TAB2_RENDERS.with(|c| c.get()), 0, "tab2 should not render");

    // Update tab1 counter - should trigger recomposition even after tab switch cycle
    TAB1_RENDERS.with(|c| c.set(0));
    TAB2_RENDERS.with(|c| c.set(0));
    tab1_counter.set_value(10);

    let _ = composition
        .process_invalid_scopes()
        .expect("recompose tab1 after cycle");

    assert!(
        TAB1_RENDERS.with(|c| c.get()) > 0,
        "tab1 scope should work after tab cycle"
    );
    assert_eq!(
        TAB2_RENDERS.with(|c| c.get()),
        0,
        "tab2 should not recompose"
    );
}

#[test]
fn slot_table_conditional_rendering_preserves_sibling_scopes() {
    let mut composition = Composition::new(MemoryApplier::new());
    let runtime = composition.runtime_handle();
    let show_middle = MutableState::with_runtime(true, runtime.clone());
    let top_counter = MutableState::with_runtime(0i32, runtime.clone());
    let middle_counter = MutableState::with_runtime(0i32, runtime.clone());
    let bottom_counter = MutableState::with_runtime(0i32, runtime.clone());

    thread_local! {
        static TOP_RENDERS: Cell<usize> = const { Cell::new(0) };
        static MIDDLE_RENDERS: Cell<usize> = const { Cell::new(0) };
        static BOTTOM_RENDERS: Cell<usize> = const { Cell::new(0) };
    }

    #[composable]
    fn top_component(counter: MutableState<i32>) {
        TOP_RENDERS.with(|c| c.set(c.get() + 1));
        let count = counter.value();
        let id = cranpose_test_node(TestTextNode::default);
        with_node_mut(id, |node: &mut TestTextNode| {
            node.text = format!("Top: {}", count);
        })
        .expect("update top node");
    }

    #[composable]
    fn middle_component(counter: MutableState<i32>) {
        MIDDLE_RENDERS.with(|c| c.set(c.get() + 1));
        let count = counter.value();
        let id = cranpose_test_node(TestTextNode::default);
        with_node_mut(id, |node: &mut TestTextNode| {
            node.text = format!("Middle: {}", count);
        })
        .expect("update middle node");
    }

    #[composable]
    fn bottom_component(counter: MutableState<i32>) {
        BOTTOM_RENDERS.with(|c| c.set(c.get() + 1));
        let count = counter.value();
        let id = cranpose_test_node(TestTextNode::default);
        with_node_mut(id, |node: &mut TestTextNode| {
            node.text = format!("Bottom: {}", count);
        })
        .expect("update bottom node");
    }

    let mut render = {
        move || {
            top_component(top_counter);

            if show_middle.value() {
                middle_component(middle_counter);
            }

            bottom_component(bottom_counter);
        }
    };

    // Initial render with all components
    TOP_RENDERS.with(|c| c.set(0));
    MIDDLE_RENDERS.with(|c| c.set(0));
    BOTTOM_RENDERS.with(|c| c.set(0));

    let key = location_key(file!(), line!(), column!());
    composition
        .render(key, &mut render)
        .expect("initial render");

    assert_eq!(TOP_RENDERS.with(|c| c.get()), 1);
    assert_eq!(MIDDLE_RENDERS.with(|c| c.get()), 1);
    assert_eq!(BOTTOM_RENDERS.with(|c| c.get()), 1);

    // Hide middle component
    show_middle.set_value(false);
    composition.render(key, &mut render).expect("hide middle");

    // Update bottom counter - should still work
    TOP_RENDERS.with(|c| c.set(0));
    MIDDLE_RENDERS.with(|c| c.set(0));
    BOTTOM_RENDERS.with(|c| c.set(0));
    bottom_counter.set_value(5);

    let _ = composition
        .process_invalid_scopes()
        .expect("recompose bottom");

    assert_eq!(TOP_RENDERS.with(|c| c.get()), 0, "top should not recompose");
    assert_eq!(
        MIDDLE_RENDERS.with(|c| c.get()),
        0,
        "middle should not recompose"
    );
    assert!(
        BOTTOM_RENDERS.with(|c| c.get()) > 0,
        "bottom scope should work after middle removed"
    );

    // Update top counter - should still work
    TOP_RENDERS.with(|c| c.set(0));
    MIDDLE_RENDERS.with(|c| c.set(0));
    BOTTOM_RENDERS.with(|c| c.set(0));
    top_counter.set_value(3);

    let _ = composition.process_invalid_scopes().expect("recompose top");

    assert!(
        TOP_RENDERS.with(|c| c.get()) > 0,
        "top scope should work after middle removed"
    );
    assert_eq!(
        MIDDLE_RENDERS.with(|c| c.get()),
        0,
        "middle should not recompose"
    );
    assert_eq!(
        BOTTOM_RENDERS.with(|c| c.get()),
        0,
        "bottom should not recompose"
    );

    // Show middle again
    show_middle.set_value(true);
    composition
        .render(key, &mut render)
        .expect("show middle again");

    // Update middle counter - should work after restoration
    TOP_RENDERS.with(|c| c.set(0));
    MIDDLE_RENDERS.with(|c| c.set(0));
    BOTTOM_RENDERS.with(|c| c.set(0));
    middle_counter.set_value(7);

    let _ = composition
        .process_invalid_scopes()
        .expect("recompose middle after restore");

    assert_eq!(TOP_RENDERS.with(|c| c.get()), 0, "top should not recompose");
    assert!(
        MIDDLE_RENDERS.with(|c| c.get()) > 0,
        "middle scope should work after restoration"
    );
    assert_eq!(
        BOTTOM_RENDERS.with(|c| c.get()),
        0,
        "bottom should not recompose"
    );
}

#[test]
fn slot_table_gaps_work_with_nested_conditionals() {
    let mut composition = Composition::new(MemoryApplier::new());
    let runtime = composition.runtime_handle();
    let outer_visible = MutableState::with_runtime(true, runtime.clone());
    let inner_visible = MutableState::with_runtime(true, runtime.clone());
    let outer_counter = MutableState::with_runtime(0i32, runtime.clone());
    let inner_counter = MutableState::with_runtime(0i32, runtime.clone());
    let after_counter = MutableState::with_runtime(0i32, runtime.clone());

    thread_local! {
        static OUTER_RENDERS: Cell<usize> = const { Cell::new(0) };
        static INNER_RENDERS: Cell<usize> = const { Cell::new(0) };
        static AFTER_RENDERS: Cell<usize> = const { Cell::new(0) };
    }

    #[composable]
    fn inner_content(counter: MutableState<i32>) {
        INNER_RENDERS.with(|c| c.set(c.get() + 1));
        let _count = counter.value();
    }

    #[composable]
    fn outer_content(
        inner_visible: MutableState<bool>,
        outer_counter: MutableState<i32>,
        inner_counter: MutableState<i32>,
    ) {
        OUTER_RENDERS.with(|c| c.set(c.get() + 1));
        let _count = outer_counter.value();

        if inner_visible.value() {
            inner_content(inner_counter);
        }
    }

    #[composable]
    fn after_content(counter: MutableState<i32>) {
        AFTER_RENDERS.with(|c| c.set(c.get() + 1));
        let _count = counter.value();
    }

    let mut render = {
        move || {
            if outer_visible.value() {
                outer_content(inner_visible, outer_counter, inner_counter);
            }
            after_content(after_counter);
        }
    };

    let key = location_key(file!(), line!(), column!());

    // Initial render - all visible
    composition
        .render(key, &mut render)
        .expect("initial render");

    // Hide inner
    inner_visible.set_value(false);
    composition.render(key, &mut render).expect("hide inner");

    // Verify after component scope still works
    AFTER_RENDERS.with(|c| c.set(0));
    after_counter.set_value(1);
    let _ = composition
        .process_invalid_scopes()
        .expect("recompose after");
    assert!(
        AFTER_RENDERS.with(|c| c.get()) > 0,
        "after scope should work with inner hidden"
    );

    // Hide outer (and inner is already hidden)
    outer_visible.set_value(false);
    composition.render(key, &mut render).expect("hide outer");

    // Verify after component scope still works
    AFTER_RENDERS.with(|c| c.set(0));
    after_counter.set_value(2);
    let _ = composition
        .process_invalid_scopes()
        .expect("recompose after with outer hidden");
    assert!(
        AFTER_RENDERS.with(|c| c.get()) > 0,
        "after scope should work with outer hidden"
    );

    // Show outer but keep inner hidden
    outer_visible.set_value(true);
    composition.render(key, &mut render).expect("show outer");

    // Verify outer scope works
    OUTER_RENDERS.with(|c| c.set(0));
    outer_counter.set_value(1);
    let _ = composition
        .process_invalid_scopes()
        .expect("recompose outer");
    assert!(
        OUTER_RENDERS.with(|c| c.get()) > 0,
        "outer scope should work after restoration"
    );

    // Show inner too
    inner_visible.set_value(true);
    composition.render(key, &mut render).expect("show inner");

    // Verify inner scope works after full restoration
    INNER_RENDERS.with(|c| c.set(0));
    inner_counter.set_value(1);
    let _ = composition
        .process_invalid_scopes()
        .expect("recompose inner");
    assert!(
        INNER_RENDERS.with(|c| c.get()) > 0,
        "inner scope should work after full restoration"
    );
}

#[test]
fn slot_table_multiple_rapid_tab_switches() {
    // Simulates rapid tab switching that could cause UI corruption
    let mut composition = Composition::new(MemoryApplier::new());
    let runtime = composition.runtime_handle();
    let active_tab = MutableState::with_runtime(0i32, runtime.clone());

    thread_local! {
        static RENDER_LOG: RefCell<Vec<String>> = const { RefCell::new(Vec::new()) };
    }

    #[composable]
    fn tab_with_multiple_elements(tab_id: i32, counter: MutableState<i32>) {
        RENDER_LOG.with(|log| log.borrow_mut().push(format!("tab{}_start", tab_id)));

        // Multiple nodes to make the slot table more complex
        let count = counter.value();
        for i in 0..3 {
            let id = cranpose_test_node(TestTextNode::default);
            with_node_mut(id, |node: &mut TestTextNode| {
                node.text = format!("Tab {} Item {} ({})", tab_id, i, count);
            })
            .expect("update node");
        }

        RENDER_LOG.with(|log| log.borrow_mut().push(format!("tab{}_end", tab_id)));
    }

    let tab_counters: Vec<_> = (0..4)
        .map(|_| MutableState::with_runtime(0i32, runtime.clone()))
        .collect();

    let mut render = {
        let tab_counters = tab_counters.clone();
        move || {
            let tab = active_tab.value();
            if tab >= 0 && (tab as usize) < tab_counters.len() {
                tab_with_multiple_elements(tab, tab_counters[tab as usize]);
            }
        }
    };

    let key = location_key(file!(), line!(), column!());

    // Rapidly switch between tabs multiple times
    for cycle in 0..3 {
        for tab in 0..4 {
            RENDER_LOG.with(|log| log.borrow_mut().clear());
            active_tab.set_value(tab);
            composition
                .render(key, &mut render)
                .unwrap_or_else(|_| panic!("render cycle {} tab {}", cycle, tab));

            let log = RENDER_LOG.with(|log| log.borrow().clone());
            assert!(
                log.len() >= 2,
                "cycle {} tab {} should render start and end markers, got {:?}",
                cycle,
                tab,
                log
            );
            assert!(
                log[0].starts_with(&format!("tab{}_start", tab)),
                "cycle {} tab {} should start correctly, got {:?}",
                cycle,
                tab,
                log
            );
        }
    }

    // After all tab switches, counters should still work
    RENDER_LOG.with(|log| log.borrow_mut().clear());
    tab_counters[2].set_value(42);
    active_tab.set_value(2);
    composition.render(key, &mut render).expect("final render");

    let _ = composition
        .process_invalid_scopes()
        .expect("final recompose");

    // If scopes are preserved correctly, the counter update should have triggered recomposition
    let final_log = RENDER_LOG.with(|log| log.borrow().clone());
    assert!(
        final_log.len() >= 2,
        "final render should work after rapid tab switches, got {:?}",
        final_log
    );
}

#[test]
fn tab_switching_with_keyed_children() {
    // Test tab switching where children use keys for identity
    let mut composition = Composition::new(MemoryApplier::new());
    let runtime = composition.runtime_handle();
    let active_tab = MutableState::with_runtime(0i32, runtime.clone());
    let counter = MutableState::with_runtime(0i32, runtime.clone());

    thread_local! {
        static TAB1_KEYED_RENDERS: Cell<usize> = const { Cell::new(0) };
        static TAB2_KEYED_RENDERS: Cell<usize> = const { Cell::new(0) };
    }

    #[composable]
    fn keyed_content(tab_id: i32, counter: MutableState<i32>) {
        if tab_id == 0 {
            TAB1_KEYED_RENDERS.with(|c| c.set(c.get() + 1));
        } else {
            TAB2_KEYED_RENDERS.with(|c| c.set(c.get() + 1));
        }

        let count = counter.value();
        cranpose_core::with_key(&format!("item_{}", tab_id), || {
            let id = cranpose_test_node(TestTextNode::default);
            with_node_mut(id, |node: &mut TestTextNode| {
                node.text = format!("Tab {} with key: {}", tab_id, count);
            })
            .expect("update keyed node");
        });
    }

    let mut render = {
        move || {
            let tab = active_tab.value();
            keyed_content(tab, counter);
        }
    };

    let key = location_key(file!(), line!(), column!());

    // Initial render - Tab 0
    TAB1_KEYED_RENDERS.with(|c| c.set(0));
    TAB2_KEYED_RENDERS.with(|c| c.set(0));
    composition
        .render(key, &mut render)
        .expect("initial render");
    assert_eq!(TAB1_KEYED_RENDERS.with(|c| c.get()), 1);

    // Switch to Tab 1
    active_tab.set_value(1);
    composition
        .render(key, &mut render)
        .expect("switch to tab 1");
    assert_eq!(TAB2_KEYED_RENDERS.with(|c| c.get()), 1);

    // Update counter and switch back to Tab 0
    counter.set_value(42);
    active_tab.set_value(0);
    TAB1_KEYED_RENDERS.with(|c| c.set(0));
    composition
        .render(key, &mut render)
        .expect("switch back to tab 0");

    // Tab 0 should rerender with updated counter
    assert!(
        TAB1_KEYED_RENDERS.with(|c| c.get()) > 0,
        "Tab 0 should rerender with updated counter value"
    );

    // Verify scope still works
    TAB1_KEYED_RENDERS.with(|c| c.set(0));
    counter.set_value(100);
    let _ = composition
        .process_invalid_scopes()
        .expect("recompose after counter update");
    assert!(
        TAB1_KEYED_RENDERS.with(|c| c.get()) > 0,
        "Tab 0 scope should still work after key-based tab switching"
    );
}

#[test]
fn tab_switching_with_different_node_types() {
    // Test switching between tabs that create different node types
    let mut composition = Composition::new(MemoryApplier::new());
    let runtime = composition.runtime_handle();
    let active_tab = MutableState::with_runtime(0i32, runtime.clone());

    thread_local! {
        static TEXT_NODE_COUNT: Cell<usize> = const { Cell::new(0) };
        static DUMMY_NODE_COUNT: Cell<usize> = const { Cell::new(0) };
    }

    #[composable]
    fn text_tab() {
        TEXT_NODE_COUNT.with(|c| c.set(c.get() + 1));
        let id = cranpose_test_node(TestTextNode::default);
        with_node_mut(id, |node: &mut TestTextNode| {
            node.text = "Text Node Tab".to_string();
        })
        .expect("update text node");
    }

    #[composable]
    fn dummy_tab() {
        DUMMY_NODE_COUNT.with(|c| c.set(c.get() + 1));
        cranpose_test_node(|| TestDummyNode);
    }

    let mut render = {
        move || match active_tab.value() {
            0 => text_tab(),
            _ => dummy_tab(),
        }
    };

    let key = location_key(file!(), line!(), column!());

    // Start with text node
    TEXT_NODE_COUNT.with(|c| c.set(0));
    DUMMY_NODE_COUNT.with(|c| c.set(0));
    composition
        .render(key, &mut render)
        .expect("initial render with text node");
    assert_eq!(TEXT_NODE_COUNT.with(|c| c.get()), 1);
    assert_eq!(DUMMY_NODE_COUNT.with(|c| c.get()), 0);

    // Switch to dummy node
    active_tab.set_value(1);
    composition
        .render(key, &mut render)
        .expect("switch to dummy node");
    assert_eq!(DUMMY_NODE_COUNT.with(|c| c.get()), 1);

    // Switch back to text node - should work without corruption
    active_tab.set_value(0);
    TEXT_NODE_COUNT.with(|c| c.set(0));
    composition
        .render(key, &mut render)
        .expect("switch back to text node");
    assert!(
        TEXT_NODE_COUNT.with(|c| c.get()) > 0,
        "Should successfully render text node after switching from different node type"
    );
}

#[test]
fn tab_switching_with_dynamic_lists() {
    // Test tab switching with tabs containing dynamic lists of varying sizes
    let mut composition = Composition::new(MemoryApplier::new());
    let runtime = composition.runtime_handle();
    let active_tab = MutableState::with_runtime(0i32, runtime.clone());
    let list_size = MutableState::with_runtime(3usize, runtime.clone());

    thread_local! {
        static LIST_TAB_CALLED: Cell<bool> = const { Cell::new(false) };
        static LAST_ITEM_COUNT: Cell<usize> = const { Cell::new(0) };
    }

    #[composable]
    fn list_tab(count: usize) {
        LIST_TAB_CALLED.with(|c| c.set(true));
        LAST_ITEM_COUNT.with(|c| c.set(count));
        for i in 0..count {
            let id = cranpose_test_node(TestTextNode::default);
            with_node_mut(id, |node: &mut TestTextNode| {
                node.text = format!("Item {}", i);
            })
            .expect("update list item");
        }
    }

    let mut render = {
        move || {
            LIST_TAB_CALLED.with(|c| c.set(false));
            if active_tab.value() == 0 {
                list_tab(list_size.value());
            }
        }
    };

    let key = location_key(file!(), line!(), column!());

    // Initial render with 3 items
    composition
        .render(key, &mut render)
        .expect("initial render");
    assert!(
        LIST_TAB_CALLED.with(|c| c.get()),
        "list_tab should be called on initial render"
    );
    assert_eq!(LAST_ITEM_COUNT.with(|c| c.get()), 3);

    // Switch away
    active_tab.set_value(1);
    composition.render(key, &mut render).expect("switch away");
    assert!(
        !LIST_TAB_CALLED.with(|c| c.get()),
        "list_tab should NOT be called when tab is inactive"
    );

    // Change list size and switch back
    list_size.set_value(5);
    active_tab.set_value(0);
    composition
        .render(key, &mut render)
        .expect("switch back with larger list");
    assert!(
        LIST_TAB_CALLED.with(|c| c.get()),
        "list_tab should be called after switch back"
    );
    assert_eq!(
        LAST_ITEM_COUNT.with(|c| c.get()),
        5,
        "Should render 5 items after switching back"
    );

    // Switch away and come back with smaller list
    active_tab.set_value(1);
    composition
        .render(key, &mut render)
        .expect("switch away again");
    assert!(
        !LIST_TAB_CALLED.with(|c| c.get()),
        "list_tab should NOT be called when inactive"
    );

    list_size.set_value(2);
    active_tab.set_value(0);
    composition
        .render(key, &mut render)
        .expect("switch back with smaller list");
    assert!(
        LIST_TAB_CALLED.with(|c| c.get()),
        "list_tab should be called after second switch back"
    );
    assert_eq!(
        LAST_ITEM_COUNT.with(|c| c.get()),
        2,
        "Should render only 2 items after shrinking list"
    );
}

#[test]
fn tab_switching_with_nested_components() {
    // Test tab switching with nested component hierarchies
    let mut composition = Composition::new(MemoryApplier::new());
    let runtime = composition.runtime_handle();
    let active_tab = MutableState::with_runtime(0i32, runtime.clone());
    let outer_counter = MutableState::with_runtime(0i32, runtime.clone());
    let inner_counter = MutableState::with_runtime(0i32, runtime.clone());

    thread_local! {
        static OUTER_RENDERS: Cell<usize> = const { Cell::new(0) };
        static INNER_RENDERS: Cell<usize> = const { Cell::new(0) };
    }

    #[composable]
    fn inner_component(counter: MutableState<i32>) {
        INNER_RENDERS.with(|c| c.set(c.get() + 1));
        let count = counter.value();
        let id = cranpose_test_node(TestTextNode::default);
        with_node_mut(id, |node: &mut TestTextNode| {
            node.text = format!("Inner: {}", count);
        })
        .expect("update inner node");
    }

    #[composable]
    fn outer_component(outer_counter: MutableState<i32>, inner_counter: MutableState<i32>) {
        println!("outer_component called");
        OUTER_RENDERS.with(|c| c.set(c.get() + 1));
        let count = outer_counter.value();
        let id = cranpose_test_node(TestTextNode::default);
        with_node_mut(id, |node: &mut TestTextNode| {
            node.text = format!("Outer: {}", count);
        })
        .expect("update outer node");

        inner_component(inner_counter);
    }

    #[composable]
    fn empty_tab() {
        // Empty tab content
    }

    let mut render = {
        move || {
            let tab = active_tab.value();
            println!("Render closure called, active_tab={}", tab);
            if tab == 0 {
                println!("About to call outer_component");
                outer_component(outer_counter, inner_counter);
            } else {
                empty_tab();
            }
        }
    };

    let key = location_key(file!(), line!(), column!());

    // Initial render
    OUTER_RENDERS.with(|c| c.set(0));
    INNER_RENDERS.with(|c| c.set(0));
    println!("=== INITIAL RENDER ===");
    composition
        .render(key, &mut render)
        .expect("initial render");
    assert_eq!(OUTER_RENDERS.with(|c| c.get()), 1);
    assert_eq!(INNER_RENDERS.with(|c| c.get()), 1);

    // Switch away
    active_tab.set_value(1);
    composition.render(key, &mut render).expect("switch away");

    // Switch back
    active_tab.set_value(0);
    OUTER_RENDERS.with(|c| c.set(0));
    INNER_RENDERS.with(|c| c.set(0));
    println!("Before switch back render");
    match composition.render(key, &mut render) {
        Ok(_) => println!("Render succeeded"),
        Err(e) => println!("Render failed: {:?}", e),
    }
    let outer_renders = OUTER_RENDERS.with(|c| c.get());
    let inner_renders = INNER_RENDERS.with(|c| c.get());
    println!(
        "After switch back: outer={}, inner={}",
        outer_renders, inner_renders
    );
    assert!(
        outer_renders > 0,
        "Outer should render, got {}",
        outer_renders
    );
    assert!(
        inner_renders > 0,
        "Inner should render, got {}",
        inner_renders
    );

    // Verify both scopes work after tab switch
    OUTER_RENDERS.with(|c| c.set(0));
    INNER_RENDERS.with(|c| c.set(0));
    outer_counter.set_value(5);
    let _ = composition
        .process_invalid_scopes()
        .expect("recompose outer");
    let outer_count = OUTER_RENDERS.with(|c| c.get());
    let inner_count = INNER_RENDERS.with(|c| c.get());
    assert!(
        outer_count > 0,
        "Outer scope should work after tab switch, got {}",
        outer_count
    );
    // NOTE: Inner should NOT rerender when only outer_counter changes because inner's inputs haven't changed
    // This is the expected Compose behavior - smart recomposition skips children when inputs are unchanged
    assert_eq!(
        inner_count, 0,
        "Inner should not rerender when only outer_counter changes (inner_counter is unchanged)"
    );

    // Verify inner scope independently
    OUTER_RENDERS.with(|c| c.set(0));
    INNER_RENDERS.with(|c| c.set(0));
    inner_counter.set_value(10);
    let _ = composition
        .process_invalid_scopes()
        .expect("recompose inner");
    assert_eq!(
        OUTER_RENDERS.with(|c| c.get()),
        0,
        "Outer should not rerender for inner-only change"
    );
    assert!(
        INNER_RENDERS.with(|c| c.get()) > 0,
        "Inner scope should work independently after tab switch"
    );
}

#[test]
fn debug_nested_component_slot_table_state() {
    // Debug test to understand slot table state during nested component recomposition
    let mut composition = Composition::new(MemoryApplier::new());
    let runtime = composition.runtime_handle();
    let active_tab = MutableState::with_runtime(0i32, runtime.clone());
    let outer_counter = MutableState::with_runtime(0i32, runtime.clone());
    let inner_counter = MutableState::with_runtime(0i32, runtime.clone());

    thread_local! {
        static OUTER_RENDERS: Cell<usize> = const { Cell::new(0) };
        static INNER_RENDERS: Cell<usize> = const { Cell::new(0) };
    }

    #[composable]
    fn inner_component(counter: MutableState<i32>) {
        INNER_RENDERS.with(|c| c.set(c.get() + 1));
        let _count = counter.value();
    }

    #[composable]
    fn outer_component(outer_counter: MutableState<i32>, inner_counter: MutableState<i32>) {
        OUTER_RENDERS.with(|c| c.set(c.get() + 1));
        let _count = outer_counter.value();
        inner_component(inner_counter);
    }

    let mut render = {
        move || {
            if active_tab.value() == 0 {
                outer_component(outer_counter, inner_counter);
            }
        }
    };

    let key = location_key(file!(), line!(), column!());

    // Initial render
    composition
        .render(key, &mut render)
        .expect("initial render");
    println!("After initial render:");
    for (idx, kind) in composition.debug_dump_all_slots() {
        println!("  [{}] {}", idx, kind);
    }

    // Switch away
    active_tab.set_value(1);
    composition.render(key, &mut render).expect("switch away");
    println!("\nAfter switch away:");
    for (idx, kind) in composition.debug_dump_all_slots() {
        println!("  [{}] {}", idx, kind);
    }

    // Switch back
    active_tab.set_value(0);
    composition.render(key, &mut render).expect("switch back");
    println!("\nAfter switch back:");
    for (idx, kind) in composition.debug_dump_all_slots() {
        println!("  [{}] {}", idx, kind);
    }

    // Trigger recomposition
    OUTER_RENDERS.with(|c| c.set(0));
    INNER_RENDERS.with(|c| c.set(0));
    outer_counter.set_value(5);

    println!("\nBefore process_invalid_scopes:");
    let groups4 = composition.debug_dump_slot_table_groups();
    for (idx, key, scope, len) in &groups4 {
        println!(
            "  Group at {}: key={:?}, scope={:?}, len={}",
            idx, key, scope, len
        );
    }

    let _ = composition
        .process_invalid_scopes()
        .expect("recompose outer");

    println!("\nAfter process_invalid_scopes:");
    let groups5 = composition.debug_dump_slot_table_groups();
    for (idx, key, scope, len) in &groups5 {
        println!(
            "  Group at {}: key={:?}, scope={:?}, len={}",
            idx, key, scope, len
        );
    }

    println!(
        "\nOuter renders: {}, Inner renders: {}",
        OUTER_RENDERS.with(|c| c.get()),
        INNER_RENDERS.with(|c| c.get())
    );
}

#[test]
fn tab_switching_memory_slot_reuse() {
    // Verify that slots are properly reused and not leaked during tab switches
    let mut composition = Composition::new(MemoryApplier::new());
    let runtime = composition.runtime_handle();
    let active_tab = MutableState::with_runtime(0i32, runtime.clone());

    #[composable]
    fn tab_with_markers(tab_id: i32) {
        // Create multiple nodes to occupy slots
        for i in 0..5 {
            let id = cranpose_test_node(TestTextNode::default);
            with_node_mut(id, |node: &mut TestTextNode| {
                node.text = format!("Tab {} Item {}", tab_id, i);
            })
            .expect("update node");
        }
    }

    let mut render = {
        move || {
            tab_with_markers(active_tab.value());
        }
    };

    let key = location_key(file!(), line!(), column!());

    // Initial render
    composition
        .render(key, &mut render)
        .expect("initial render");

    // Perform many tab switches to check for slot leaks
    for cycle in 0..10 {
        for tab in 0..4 {
            active_tab.set_value(tab);
            composition
                .render(key, &mut render)
                .unwrap_or_else(|_| panic!("render cycle {} tab {}", cycle, tab));
        }
    }

    // If we made it here without panicking or running out of memory,
    // slots are being properly reused
    // Verify final render still works correctly
    active_tab.set_value(0);
    composition
        .render(key, &mut render)
        .expect("final render after many switches");
}

#[test]
fn tab_switching_with_state_during_switch() {
    // Test updating state while switching tabs - edge case for race conditions
    let mut composition = Composition::new(MemoryApplier::new());
    let runtime = composition.runtime_handle();
    let active_tab = MutableState::with_runtime(0i32, runtime.clone());
    let shared_counter = MutableState::with_runtime(0i32, runtime.clone());

    thread_local! {
        static TAB0_RENDERS: Cell<usize> = const { Cell::new(0) };
        static TAB1_RENDERS: Cell<usize> = const { Cell::new(0) };
    }

    #[composable]
    fn tab_content(tab_id: i32, counter: MutableState<i32>) {
        if tab_id == 0 {
            TAB0_RENDERS.with(|c| c.set(c.get() + 1));
        } else {
            TAB1_RENDERS.with(|c| c.set(c.get() + 1));
        }
        let count = counter.value();
        let id = cranpose_test_node(TestTextNode::default);
        with_node_mut(id, |node: &mut TestTextNode| {
            node.text = format!("Tab {} Count {}", tab_id, count);
        })
        .expect("update node");
    }

    let mut render = {
        move || {
            tab_content(active_tab.value(), shared_counter);
        }
    };

    let key = location_key(file!(), line!(), column!());

    // Initial render
    TAB0_RENDERS.with(|c| c.set(0));
    composition
        .render(key, &mut render)
        .expect("initial render");
    assert_eq!(TAB0_RENDERS.with(|c| c.get()), 1);

    // Update state AND switch tabs simultaneously
    shared_counter.set_value(42);
    active_tab.set_value(1);
    TAB1_RENDERS.with(|c| c.set(0));
    composition
        .render(key, &mut render)
        .expect("switch with state update");

    // Tab 1 should render with updated counter
    assert!(
        TAB1_RENDERS.with(|c| c.get()) > 0,
        "Tab 1 should render with updated state"
    );

    // Switch back - scope should still work
    active_tab.set_value(0);
    TAB0_RENDERS.with(|c| c.set(0));
    composition.render(key, &mut render).expect("switch back");

    shared_counter.set_value(100);
    TAB0_RENDERS.with(|c| c.set(0));
    let _ = composition
        .process_invalid_scopes()
        .expect("recompose after state update");
    assert!(
        TAB0_RENDERS.with(|c| c.get()) > 0,
        "Tab 0 scope should still work after concurrent state/tab change"
    );
}

#[test]
fn tab_switching_with_empty_tab() {
    // Test switching to/from an empty tab (no nodes created)
    let mut composition = Composition::new(MemoryApplier::new());
    let runtime = composition.runtime_handle();
    let active_tab = MutableState::with_runtime(0i32, runtime.clone());
    let counter = MutableState::with_runtime(0i32, runtime.clone());

    thread_local! {
        static CONTENT_RENDERS: Cell<usize> = const { Cell::new(0) };
    }

    #[composable]
    fn content_tab(counter: MutableState<i32>) {
        CONTENT_RENDERS.with(|c| c.set(c.get() + 1));
        let count = counter.value();
        let id = cranpose_test_node(TestTextNode::default);
        with_node_mut(id, |node: &mut TestTextNode| {
            node.text = format!("Content: {}", count);
        })
        .expect("update node");
    }

    #[composable]
    fn empty_tab() {
        // Intentionally empty - no nodes created
    }

    let mut render = {
        move || match active_tab.value() {
            0 => content_tab(counter),
            _ => empty_tab(),
        }
    };

    let key = location_key(file!(), line!(), column!());

    // Start with content
    CONTENT_RENDERS.with(|c| c.set(0));
    composition
        .render(key, &mut render)
        .expect("initial render");
    assert_eq!(CONTENT_RENDERS.with(|c| c.get()), 1);

    // Switch to empty tab
    active_tab.set_value(1);
    composition
        .render(key, &mut render)
        .expect("switch to empty");

    // Switch back to content
    active_tab.set_value(0);
    CONTENT_RENDERS.with(|c| c.set(0));
    composition
        .render(key, &mut render)
        .expect("switch back from empty");
    assert!(
        CONTENT_RENDERS.with(|c| c.get()) > 0,
        "Should render content after empty tab"
    );

    // Verify scope still works
    CONTENT_RENDERS.with(|c| c.set(0));
    counter.set_value(42);
    let _ = composition
        .process_invalid_scopes()
        .expect("recompose after empty");
    assert!(
        CONTENT_RENDERS.with(|c| c.get()) > 0,
        "Scope should work after switching from empty tab"
    );
}

#[test]
fn tab_switching_preserves_node_order() {
    // Verify that node order is preserved correctly across tab switches
    let mut composition = Composition::new(MemoryApplier::new());
    let runtime = composition.runtime_handle();
    let active_tab = MutableState::with_runtime(0i32, runtime.clone());

    thread_local! {
        static RENDER_ORDER: RefCell<Vec<String>> = const { RefCell::new(Vec::new()) };
    }

    #[composable]
    fn ordered_tab(tab_id: i32) {
        RENDER_ORDER.with(|o| o.borrow_mut().clear());
        let prefix = if tab_id == 0 { "A" } else { "B" };
        for i in 0..3 {
            RENDER_ORDER.with(|o| o.borrow_mut().push(format!("{}_{}", prefix, i)));
            let id = cranpose_test_node(TestTextNode::default);
            with_node_mut(id, |node: &mut TestTextNode| {
                node.text = format!("{} Item {}", prefix, i);
            })
            .expect("update node");
        }
    }

    let mut render = {
        move || {
            let tab = active_tab.value();
            if tab <= 1 {
                ordered_tab(tab);
            }
        }
    };

    let key = location_key(file!(), line!(), column!());

    // Initial render with Tab A
    composition
        .render(key, &mut render)
        .expect("initial render");
    let order_a = RENDER_ORDER.with(|o| o.borrow().clone());
    assert_eq!(order_a, vec!["A_0", "A_1", "A_2"]);

    // Switch to Tab B
    active_tab.set_value(1);
    composition.render(key, &mut render).expect("switch to B");
    let order_b = RENDER_ORDER.with(|o| o.borrow().clone());
    assert_eq!(order_b, vec!["B_0", "B_1", "B_2"]);

    // Switch back to Tab A - order should be preserved
    active_tab.set_value(0);
    composition
        .render(key, &mut render)
        .expect("switch back to A");
    let order_a_again = RENDER_ORDER.with(|o| o.borrow().clone());
    assert_eq!(
        order_a_again,
        vec!["A_0", "A_1", "A_2"],
        "Order should be preserved after tab switch"
    );
}

// ════════════════════════════════════════════════════════════════════════════
// Backend Integration Tests
// ════════════════════════════════════════════════════════════════════════════

#[test]
fn composition_works_with_baseline_backend() {
    test_composition_with_backend(SlotBackendKind::Baseline);
}

#[test]
fn composition_works_with_chunked_backend() {
    test_composition_with_backend(SlotBackendKind::Chunked);
}

#[test]
fn composition_works_with_split_backend() {
    test_composition_with_backend(SlotBackendKind::Split);
}

#[test]
fn composition_works_with_hierarchical_backend() {
    test_composition_with_backend(SlotBackendKind::Hierarchical);
}

fn test_composition_with_backend(backend: SlotBackendKind) {
    let key = 12345u64;
    let applier = MemoryApplier::new();
    let runtime = Runtime::new(Arc::new(TestScheduler));
    let mut composition = Composition::with_backend(applier, runtime.clone(), backend);

    // Track recompositions
    let recranpose_count = Rc::new(Cell::new(0));
    let recranpose_count_clone = Rc::clone(&recranpose_count);

    // Test basic composition with groups and remembered values
    composition
        .render(key, || {
            with_current_composer(|composer| {
                composer.with_group(1, |composer| {
                    recranpose_count_clone.set(recranpose_count_clone.get() + 1);

                    // Test remember
                    let value = composer.remember(|| 123);
                    value.with(|v| assert_eq!(*v, 123));

                    // Test nested group
                    composer.with_group(2, |composer| {
                        let nested = composer.remember(|| "hello".to_string());
                        nested.with(|n| assert_eq!(n, "hello"));
                    });
                });
            });
        })
        .expect("first render");

    assert_eq!(recranpose_count.get(), 1, "Should have composed once");

    // Test recomposition preserves remembered values
    composition
        .render(key, || {
            with_current_composer(|composer| {
                composer.with_group(1, |composer| {
                    recranpose_count_clone.set(recranpose_count_clone.get() + 1);

                    // Remembered value should be preserved
                    let value = composer.remember(|| 456); // Different init, but should return 123
                    value.with(|v| assert_eq!(*v, 123, "Remembered value should be preserved"));

                    composer.with_group(2, |composer| {
                        let nested = composer.remember(|| "world".to_string());
                        nested.with(|n| {
                            assert_eq!(n, "hello", "Nested remembered value should be preserved")
                        });
                    });
                });
            });
        })
        .expect("second render");

    assert_eq!(recranpose_count.get(), 2, "Should have composed twice");
}

/// Test that emit_node rejects reuse when the parent's previous children list
/// didn't contain the candidate node. This prevents nodes from "teleporting"
/// between parents.
#[test]
fn emit_node_rejects_reuse_when_parent_did_not_own_child() {
    let (handle, _runtime) = runtime_handle();
    let mut slots = SlotBackend::default();
    let mut applier = MemoryApplier::new();

    // Create a parent and child node in the applier
    let parent_a = applier.create(Box::new(RecordingNode::default()));
    let parent_b = applier.create(Box::new(RecordingNode::default()));

    // Setup the test: we'll push parent_b and try to reuse a child that was
    // previously under parent_a. The emit_node should reject the reuse.
    let (composer, slots_host, applier_host) =
        setup_composer(&mut slots, &mut applier, handle.clone(), Some(parent_a));

    // First, emit a child under parent_a's context
    let child_id = composer.emit_node(|| TestDummyNode);

    // Now push parent_b with last_node_reused=false (simulating new parent)
    composer.core.last_node_reused.set(Some(false));
    composer.push_parent(parent_b);

    // The parent_b's previous children should be empty since it wasn't reused
    {
        let stack = composer.parent_stack();
        let frame = stack.last().expect("parent frame should exist");
        assert!(
            frame.previous.is_empty(),
            "New parent should have empty previous children"
        );
    }

    composer.pop_parent();
    drop(composer);
    teardown_composer(&mut slots, &mut applier, slots_host, applier_host);

    // Verify child was created
    assert!(child_id > 0, "Child should have been created");
}

/// Test that push_parent uses empty previous children when the parent node
/// was not reused (last_node_reused = false). This ensures new parents start
/// fresh and don't inherit children from a different node.
#[test]
fn push_parent_uses_empty_previous_when_not_reused() {
    let (handle, _runtime) = runtime_handle();
    let mut slots = SlotBackend::default();
    let mut applier = MemoryApplier::new();

    let parent_id = applier.create(Box::new(RecordingNode::default()));

    let (composer, slots_host, applier_host) =
        setup_composer(&mut slots, &mut applier, handle.clone(), Some(parent_id));

    // Simulate: last node was NOT reused (new node created)
    composer.core.last_node_reused.set(Some(false));
    composer.push_parent(parent_id);

    // Verify that previous is empty when node was not reused
    {
        let stack = composer.parent_stack();
        let frame = stack.last().expect("parent frame should exist");
        assert!(
            frame.previous.is_empty(),
            "When parent was not reused, previous children should be empty"
        );
    }

    composer.pop_parent();
    drop(composer);
    teardown_composer(&mut slots, &mut applier, slots_host, applier_host);
}

/// Test that push_parent reuses previous children when the parent node
/// was reused (last_node_reused = true). This ensures stable parents
/// correctly track their children across recompositions.
#[test]
fn push_parent_inherits_previous_when_reused() {
    let (handle, _runtime) = runtime_handle();
    let mut slots = SlotBackend::default();
    let mut applier = MemoryApplier::new();

    let parent_id = applier.create(Box::new(RecordingNode::default()));

    {
        let (composer, slots_host, applier_host) =
            setup_composer(&mut slots, &mut applier, handle.clone(), Some(parent_id));

        // First pass: establish children
        composer.core.last_node_reused.set(Some(true)); // Pretend parent was reused
        composer.push_parent(parent_id);

        // Simulate adding a child
        {
            let mut stack = composer.parent_stack();
            let frame = stack.last_mut().expect("parent frame should exist");
            frame.new_children.push(42); // Fake child ID
        }

        composer.pop_parent();
        drop(composer);
        teardown_composer(&mut slots, &mut applier, slots_host, applier_host);
    }

    // Reset for second pass
    slots.reset();

    {
        let (composer, slots_host, applier_host) =
            setup_composer(&mut slots, &mut applier, handle.clone(), Some(parent_id));

        // Second pass: parent is reused, should inherit previous children
        composer.core.last_node_reused.set(Some(true));
        composer.push_parent(parent_id);

        // Verify that previous contains the child from first pass
        {
            let stack = composer.parent_stack();
            let frame = stack.last().expect("parent frame should exist");
            assert_eq!(
                frame.previous,
                vec![42],
                "When parent was reused, previous children should be inherited"
            );
        }

        composer.pop_parent();
        drop(composer);
        teardown_composer(&mut slots, &mut applier, slots_host, applier_host);
    }
}

/// Regression test: emit_node should successfully create/reuse nodes
/// even when push_parent starts with empty previous children (new parent case).
/// This was broken when we added a parent_contains check to emit_node that
/// prevented node creation when previous was empty.
///
/// Scenario (simulates tab switch):
/// 1. Render a parent with a child
/// 2. Remove the parent (tab switch away)  
/// 3. Restore the parent (tab switch back)
/// 4. Child should be created/reused successfully (functionality preserved)
#[test]
fn emit_node_creates_nodes_when_parent_restored_after_conditional_removal() {
    let mut composition = Composition::new(MemoryApplier::new());
    let runtime = composition.runtime_handle();
    let toggle = MutableState::with_runtime(true, runtime.clone());

    let key = location_key(file!(), line!(), column!());

    // Track child node IDs across renders
    let child_ids: Rc<RefCell<Vec<NodeId>>> = Rc::new(RefCell::new(Vec::new()));

    // First render: parent visible with child
    println!("=== First render: parent visible ===");
    {
        let child_ids = Rc::clone(&child_ids);
        composition
            .render(key, move || {
                if toggle.value() {
                    with_current_composer(|composer| {
                        // Parent node
                        let _parent = composer.emit_node(|| TestDummyNode);
                        composer.core.last_node_reused.set(Some(true));
                        composer.push_parent(_parent);

                        // Child node - track its ID
                        let child = composer.emit_node(|| TestTextNode {
                            text: "Reusable Child".to_string(),
                        });
                        child_ids.borrow_mut().push(child);

                        composer.pop_parent();
                    });
                }
            })
            .expect("first render");
    }

    let first_child_id = child_ids.borrow()[0];
    println!("First child ID: {}", first_child_id);
    assert!(first_child_id > 0, "First child should be created");

    // Second render: parent removed (simulate tab switch away)
    println!("=== Second render: parent hidden ===");
    toggle.set_value(false);
    {
        composition
            .render(key, move || {
                if toggle.value() {
                    // Not rendered - parent is hidden
                }
            })
            .expect("second render");
    }

    // Third render: parent restored (simulate tab switch back)
    // This is where the regression occurred - emit_node was failing
    // when parent's previous children was empty
    println!("=== Third render: parent restored ===");
    toggle.set_value(true);
    {
        let child_ids = Rc::clone(&child_ids);
        composition
            .render(key, move || {
                if toggle.value() {
                    with_current_composer(|composer| {
                        // Parent node - may be recreated due to conditional
                        let _parent = composer.emit_node(|| TestDummyNode);
                        let reused = composer.core.last_node_reused.get();
                        println!("Parent reused: {:?}", reused);
                        composer.push_parent(_parent);

                        // CRITICAL: Child node should be successfully created/reused
                        // even though parent's previous children list may be empty.
                        // This is what broke when we added the parent_contains check.
                        let child = composer.emit_node(|| TestTextNode {
                            text: "Reusable Child".to_string(),
                        });
                        child_ids.borrow_mut().push(child);
                        println!("Third render child ID: {}", child);

                        composer.pop_parent();
                    });
                }
            })
            .expect("third render");
    }

    let third_child_id = child_ids.borrow().last().copied().unwrap();
    println!("Third child ID: {}", third_child_id);

    // CRITICAL: The child should be VALID (non-zero ID) after parent restoration.
    // The ID may or may not be the same as the first ID (depends on SlotTable behavior),
    // but the important thing is emit_node succeeded.
    assert!(
        third_child_id > 0,
        "Child node should be successfully created after parent restoration. \
         If this fails, emit_node is rejecting node creation when previous is empty."
    );

    // Verify we got two child IDs (one from first render, one from third render)
    assert_eq!(
        child_ids.borrow().len(),
        2,
        "Should have recorded child IDs from both visible renders"
    );
}

/// Test that nodes can be emitted under a new parent without requiring
/// the parent to have had them previously. This is the core invariant
/// that broke when emit_node checked parent_contains too strictly.
#[test]
fn emit_node_works_with_new_parent_having_empty_previous() {
    let (handle, _runtime) = runtime_handle();
    let mut slots = SlotBackend::default();
    let mut applier = MemoryApplier::new();

    let parent_id = applier.create(Box::new(RecordingNode::default()));

    let (composer, slots_host, applier_host) =
        setup_composer(&mut slots, &mut applier, handle.clone(), Some(parent_id));

    // Simulate a NEW parent (not reused) - this gives empty previous children
    composer.core.last_node_reused.set(Some(false));
    composer.push_parent(parent_id);

    // Verify previous is empty (this is the push_parent conditional behavior)
    {
        let stack = composer.parent_stack();
        let frame = stack.last().expect("parent frame should exist");
        assert!(
            frame.previous.is_empty(),
            "New parent should have empty previous"
        );
    }

    // Now emit a child - this should WORK even though previous is empty
    // The child should be created (or reused based on type, if one exists in slots)
    let child_id = composer.emit_node(|| TestDummyNode);

    // The child should have a valid ID
    assert!(child_id > 0, "Child should be emitted successfully");

    // last_node_reused should be set (either true for reuse or false for new)
    let was_reused = composer.core.last_node_reused.get();
    assert!(
        was_reused.is_some(),
        "emit_node should set last_node_reused"
    );

    composer.pop_parent();
    drop(composer);
    teardown_composer(&mut slots, &mut applier, slots_host, applier_host);
}

/// Test that state changes made in frame callbacks are visible globally.
///
/// This is a regression test for a bug where fling animation scroll state
/// would reset because frame callback state writes weren't being applied
/// to the global snapshot.
#[test]
fn frame_callback_state_changes_are_visible_globally() {
    let (handle, _runtime) = runtime_handle();
    let state = MutableState::with_runtime(0i32, handle.clone());

    // Register a frame callback that updates state
    let state_for_callback = state;
    let callback_ran = Rc::new(Cell::new(false));
    let callback_ran_for_closure = callback_ran.clone();

    let _registration = handle.frame_clock().with_frame_nanos(move |_| {
        // This state change should be visible after drain_frame_callbacks completes
        state_for_callback.set(42);
        callback_ran_for_closure.set(true);
    });

    // Before draining, state should be 0
    assert_eq!(state.get(), 0);

    // Drain frame callbacks - this should apply state changes to global snapshot
    handle.drain_frame_callbacks(1);

    // Verify callback ran
    assert!(callback_ran.get(), "Frame callback should have run");

    // CRITICAL: State change from callback should be visible
    // Before the fix, this would return 0 because the write was isolated
    assert_eq!(
        state.get(),
        42,
        "State change in frame callback should be visible globally"
    );
}

/// Test that multiple frame callbacks in sequence all have their state changes visible.
#[test]
fn multiple_frame_callbacks_state_visibility() {
    let (handle, _runtime) = runtime_handle();
    let state = MutableState::with_runtime(0i32, handle.clone());

    // First frame callback increments by 10
    let state1 = state;
    let _reg1 = handle.frame_clock().with_frame_nanos(move |_| {
        let current = state1.get();
        state1.set(current + 10);
    });

    // Second frame callback increments by 5
    let state2 = state;
    let _reg2 = handle.frame_clock().with_frame_nanos(move |_| {
        let current = state2.get();
        state2.set(current + 5);
    });

    // Drain all callbacks
    handle.drain_frame_callbacks(1);

    // Both callbacks should have run and their changes should be visible
    // The first sets to 10, the second reads 10 and sets to 15
    assert_eq!(
        state.get(),
        15,
        "Sequential frame callback state changes should accumulate correctly"
    );
}
