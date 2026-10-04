use std::{
    cell::{Cell, RefCell},
    hash::{Hash, Hasher},
    rc::Rc,
};

use cranpose_core::{
    DefaultScheduler, OwnedMutableState, Runtime, SnapshotStateObserver, scheduler_ref,
};

#[derive(Clone, PartialEq, Eq)]
struct Scope(usize);

impl Hash for Scope {
    fn hash<H: Hasher>(&self, state: &mut H) {
        0usize.hash(state);
    }
}

struct Released(Rc<Cell<usize>>);

impl Drop for Released {
    fn drop(&mut self) {
        self.0.set(self.0.get() + 1);
    }
}

#[test]
fn selective_clear_preserves_colliding_scopes_and_releases_removed_callbacks() {
    let runtime = Runtime::new(scheduler_ref(DefaultScheduler));
    let state = OwnedMutableState::with_runtime(0, runtime.handle());
    let observer = SnapshotStateObserver::new(|callback| callback());
    observer.start();
    let received = Rc::new(RefCell::new(Vec::new()));
    let released = Rc::new(Cell::new(0));
    for index in 0..8 {
        let received = Rc::clone(&received);
        let release = Released(Rc::clone(&released));
        observer.observe_reads(
            Scope(index),
            move |scope| {
                let _ = &release;
                received.borrow_mut().push(scope.0);
            },
            || state.get(),
        );
    }
    observer.clear_if(|scope| scope.downcast_ref::<Scope>().is_some_and(|s| s.0 % 2 == 0));
    assert_eq!(released.get(), 4);
    observer.clear_if(<dyn std::any::Any>::is::<String>);
    state.set(1);
    assert_eq!(*received.borrow(), [1, 3, 5, 7]);

    received.borrow_mut().clear();
    let sink = Rc::clone(&received);
    observer.observe_reads(
        Scope(2),
        move |scope| sink.borrow_mut().push(scope.0),
        || state.get(),
    );
    observer.clear(&Scope(3));
    state.set(2);
    assert_eq!(*received.borrow(), [1, 5, 7, 2]);
    observer.clear_all();
    assert_eq!(released.get(), 8);
    received.borrow_mut().clear();
    state.set(3);
    assert!(received.borrow().is_empty());
}
