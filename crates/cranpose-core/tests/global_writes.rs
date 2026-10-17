//! A global write stores its value in place in the record the global
//! snapshot wrote at its current id. A snapshot taken from the global keeps
//! the values it saw, and each write still reaches the apply observers.

use std::{
    cell::RefCell,
    hash::{Hash, Hasher},
    rc::Rc,
};

use cranpose_core::{
    DefaultScheduler, OwnedMutableState, Runtime, SnapshotStateObserver, scheduler_ref,
    snapshot_v2::GlobalSnapshot,
};

#[derive(Clone, PartialEq, Eq)]
struct Scope;

impl Hash for Scope {
    fn hash<H: Hasher>(&self, state: &mut H) {
        0usize.hash(state);
    }
}

#[test]
fn a_snapshot_taken_between_global_writes_keeps_the_value_it_saw() {
    let runtime = Runtime::new(scheduler_ref(DefaultScheduler));
    let state = OwnedMutableState::with_runtime(0u32, runtime.handle());
    state.set(1);
    state.set(2);
    let snapshot = GlobalSnapshot::get_or_create().take_nested_snapshot(None);
    state.set(3);
    state.set(4);

    assert_eq!(snapshot.enter(|| state.get()), 2);
    assert_eq!(state.get(), 4);
    snapshot.dispose();
}

#[test]
fn every_global_write_reaches_the_apply_observers() {
    let runtime = Runtime::new(scheduler_ref(DefaultScheduler));
    let state = OwnedMutableState::with_runtime(0u32, runtime.handle());
    let observer = SnapshotStateObserver::new(|callback| callback());
    observer.start();
    let seen = Rc::new(RefCell::new(Vec::new()));
    for value in 1..=3 {
        let sink = Rc::clone(&seen);
        let handle = state.handle();
        observer.observe_reads(
            Scope,
            move |_| sink.borrow_mut().push(handle.get()),
            || state.get(),
        );
        state.set(value);
    }
    assert_eq!(*seen.borrow(), [1, 2, 3]);
}
