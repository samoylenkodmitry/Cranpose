use super::*;

#[test]
fn oversized_parked_tail_is_dropped_instead_of_recycled() {
    // Drain any existing thread-local spares so we inspect only this test's.
    crate::thread_state::runtimes(|state| {
        state.spare_parked_tails.borrow_mut().clear();
    });

    let oversized = ParkedTailStorage {
        groups: Vec::with_capacity(SPARE_PARKED_TAIL_CAPACITY + 1),
        nodes: Vec::new(),
        payload_order: Vec::new(),
    };
    recycle_parked_tail(oversized);
    let kept = crate::thread_state::runtimes(|state| state.spare_parked_tails.borrow().len());
    assert_eq!(kept, 0, "tail buffer past capacity bound must not be kept");

    let small = ParkedTailStorage {
        groups: Vec::with_capacity(16),
        nodes: Vec::with_capacity(16),
        payload_order: Vec::with_capacity(16),
    };
    recycle_parked_tail(small);
    let kept = crate::thread_state::runtimes(|state| state.spare_parked_tails.borrow().len());
    assert_eq!(kept, 1, "small tail buffer is kept for reuse");
    drop(spare_parked_tail());
}
