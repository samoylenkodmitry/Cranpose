//! A thread may hold several runtimes: a state finds its own runtime's
//! value, whichever runtime started its work last.

use cranpose_core::{DefaultScheduler, OwnedMutableState, Runtime, scheduler_ref};

#[test]
fn a_state_of_an_earlier_runtime_keeps_its_value_once_another_runtime_starts() {
    let first = Runtime::new(scheduler_ref(DefaultScheduler));
    let earlier = OwnedMutableState::with_runtime(1u32, first.handle());
    let second = Runtime::new(scheduler_ref(DefaultScheduler));
    let later = OwnedMutableState::with_runtime(7u32, second.handle());

    earlier.set(2);
    later.set(8);

    assert_eq!(earlier.get(), 2);
    assert_eq!(later.get(), 8);
}
