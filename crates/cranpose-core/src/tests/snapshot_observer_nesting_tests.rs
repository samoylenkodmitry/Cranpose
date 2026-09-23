use super::*;
use crate::state::NeverEqual;

fn counting(hits: &Rc<Cell<u32>>) -> impl Fn(&&'static str) + 'static {
    let hits = Rc::clone(hits);
    move |_| hits.set(hits.get() + 1)
}

#[test]
fn an_observer_nested_in_another_observers_pass_keeps_its_reads() {
    let _guard = reset_snapshot_runtime();
    let outer_state = SnapshotMutableState::new_in_arc(0, Arc::new(NeverEqual));
    let inner_state = SnapshotMutableState::new_in_arc(0, Arc::new(NeverEqual));
    let outer = SnapshotStateObserver::new(|job| job());
    let inner = SnapshotStateObserver::new(|job| job());
    outer.start();
    inner.start();
    let outer_hits = Rc::new(Cell::new(0));
    let inner_hits = Rc::new(Cell::new(0));

    outer.observe_reads("outer", counting(&outer_hits), || {
        let _ = outer_state.get();
        inner.observe_reads("inner", counting(&inner_hits), || {
            let _ = inner_state.get();
        });
    });

    inner_state.set(1);
    assert_eq!(inner_hits.get(), 1, "the nested observer lost its read");
    assert_eq!(
        outer_hits.get(),
        1,
        "the enclosing observer sees nested reads too"
    );
    outer_state.set(1);
    assert_eq!(outer_hits.get(), 2);
    assert_eq!(inner_hits.get(), 1);
}

#[test]
fn nesting_the_same_observer_keeps_reads_on_the_innermost_scope() {
    let _guard = reset_snapshot_runtime();
    let outer_state = SnapshotMutableState::new_in_arc(0, Arc::new(NeverEqual));
    let inner_state = SnapshotMutableState::new_in_arc(0, Arc::new(NeverEqual));
    let observer = SnapshotStateObserver::new(|job| job());
    observer.start();
    let outer_hits = Rc::new(Cell::new(0));
    let inner_hits = Rc::new(Cell::new(0));

    observer.observe_reads("outer", counting(&outer_hits), || {
        let _ = outer_state.get();
        observer.observe_reads("inner", counting(&inner_hits), || {
            let _ = inner_state.get();
        });
    });

    inner_state.set(1);
    assert_eq!((outer_hits.get(), inner_hits.get()), (0, 1));
    outer_state.set(1);
    assert_eq!((outer_hits.get(), inner_hits.get()), (1, 1));
}
