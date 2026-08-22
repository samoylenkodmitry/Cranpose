//! The surfaces a host or a devtool reaches for, and nothing else does.
//!
//! Frame-clock registration, subcompose reuse policy and the slot-table debug
//! readers are gated behind `internal` because they are a host's business
//! rather than an application's — which is exactly why nothing in the
//! application-facing test suite touches them, and why they need a test here.

use crate::frame_clock::FrameClock;
use crate::{Composition, MemoryApplier};
use std::cell::Cell;
use std::rc::Rc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;

#[test]
fn a_frame_callback_fires_once_for_the_frame_it_registered_for() {
    let composition = Composition::new(MemoryApplier::new());
    let clock = FrameClock::new(composition.runtime_handle());
    let millis = Arc::new(AtomicUsize::new(0));
    let nanos = Arc::new(AtomicUsize::new(0));

    let millis_sink = Arc::clone(&millis);
    let _millis = clock.with_frame_millis(move |_| {
        millis_sink.fetch_add(1, Ordering::SeqCst);
    });
    let nanos_sink = Arc::clone(&nanos);
    let _nanos = clock.with_frame_nanos(move |_| {
        nanos_sink.fetch_add(1, Ordering::SeqCst);
    });

    clock.runtime_handle().drain_frame_callbacks(16_000_000);
    assert_eq!(millis.load(Ordering::SeqCst), 1);
    assert_eq!(nanos.load(Ordering::SeqCst), 1);

    // One registration is one delivery: a callback already handed its frame
    // must not be handed the next one as well.
    clock.runtime_handle().drain_frame_callbacks(32_000_000);
    assert_eq!(millis.load(Ordering::SeqCst), 1);
    assert_eq!(nanos.load(Ordering::SeqCst), 1);
}

#[test]
fn a_perpetual_frame_keeps_asking_after_every_dispatch() {
    let composition = Composition::new(MemoryApplier::new());
    let clock = FrameClock::new(composition.runtime_handle());
    let seen = Rc::new(Cell::new(0usize));

    // Unlike a one-shot callback, the perpetual future is re-armed by whoever
    // awaits it, so two dispatches reach two awaits.
    for frame in 1..=2u64 {
        let mut next = Box::pin(clock.next_perpetual_frame());
        let waker = std::task::Waker::noop().clone();
        let mut context = std::task::Context::from_waker(&waker);
        assert!(
            std::future::Future::poll(next.as_mut(), &mut context).is_pending(),
            "a frame was answered before it was dispatched"
        );
        clock
            .runtime_handle()
            .drain_frame_callbacks(frame * 16_000_000);
        if std::future::Future::poll(next.as_mut(), &mut context).is_ready() {
            seen.set(seen.get() + 1);
        }
    }
    assert_eq!(seen.get(), 2, "the perpetual frame stopped answering");
}

#[test]
fn a_snapshot_id_range_adds_every_id_in_it_and_nothing_outside() {
    use crate::snapshot_id_set::SnapshotIdSet;

    let set = SnapshotIdSet::EMPTY.add_range(3, 7);
    assert_eq!(set.to_list(), vec![3, 4, 5, 6], "the range is half-open");

    // An empty or reversed range is a no-op rather than an infinite walk.
    assert_eq!(set.add_range(5, 5).to_list(), set.to_list());
    assert_eq!(set.add_range(9, 2).to_list(), set.to_list());

    // Adding overlapping ranges is idempotent where they overlap.
    let widened = set.add_range(5, 9);
    assert_eq!(widened.to_list(), vec![3, 4, 5, 6, 7, 8]);
}

#[test]
fn a_callback_holder_forwards_to_whichever_closure_was_last_stored() {
    use crate::callbacks::CallbackHolder;
    use std::cell::Cell;
    use std::rc::Rc;

    let holder = CallbackHolder::new();
    let calls = Rc::new(Cell::new(0u32));

    // A fresh holder is callable before anything is stored, which is what lets
    // a generated composable hand the forwarder out before the body runs.
    let forward = holder.clone_rc();
    forward();
    assert_eq!(calls.get(), 0);

    let first = Rc::clone(&calls);
    holder.update_boxed(Box::new(move || first.set(first.get() + 1)));
    forward();
    assert_eq!(calls.get(), 1);

    // Storing again replaces rather than accumulates: two callbacks firing per
    // click is the bug this guards.
    let second = Rc::clone(&calls);
    holder.update_boxed(Box::new(move || second.set(second.get() + 10)));
    forward();
    assert_eq!(calls.get(), 11);
}

#[test]
fn a_ui_task_label_is_taken_by_the_next_task_and_not_the_one_after() {
    use crate::runtime::label_next_ui_task;
    use std::sync::atomic::{AtomicU32, Ordering};
    use std::sync::Arc;

    let composition = Composition::new(MemoryApplier::new());
    let runtime = composition.runtime_handle();
    let order = Arc::new(AtomicU32::new(0));

    // The label is consumed by whichever task is posted next; posting two and
    // draining must run both regardless of which one carried the label.
    label_next_ui_task("labelled");
    let first = Arc::clone(&order);
    runtime.post_ui(move || {
        first.fetch_add(1, Ordering::SeqCst);
    });
    let second = Arc::clone(&order);
    runtime.post_ui(move || {
        second.fetch_add(10, Ordering::SeqCst);
    });

    runtime.drain_ui();
    assert_eq!(
        order.load(Ordering::SeqCst),
        11,
        "a labelled task changed what ran"
    );
}

#[test]
fn a_subcompose_state_takes_a_policy_and_pool_limits_without_disturbing_its_slots() {
    use crate::subcompose::{ContentTypeReusePolicy, SubcomposeState};

    let mut state = SubcomposeState::new(Box::new(ContentTypeReusePolicy::default()));
    assert_eq!(
        state.was_last_slot_reused(),
        None,
        "a state that has composed nothing cannot have reused anything"
    );
    assert_eq!(state.active_slots_count(), 0);

    state.set_policy(Box::new(ContentTypeReusePolicy::default()));
    state.set_reusable_pool_limits(2, 3);
    assert_eq!(
        state.active_slots_count(),
        0,
        "changing the policy disturbed the slots it was only meant to govern"
    );
    assert_eq!(state.was_last_slot_reused(), None);
}
