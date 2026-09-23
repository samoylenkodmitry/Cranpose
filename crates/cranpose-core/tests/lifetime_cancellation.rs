#![cfg(feature = "internal")]

use std::{
    cell::{Cell, RefCell},
    rc::Rc,
};

use cranpose_core::{Composition, CoroutineScope, MemoryApplier, rememberCoroutineScope};

fn capture_scope(composition: &mut Composition<MemoryApplier>) -> CoroutineScope {
    let held_scope = Rc::new(RefCell::new(None));
    let capture = Rc::clone(&held_scope);
    composition
        .render(1, move || {
            *capture.borrow_mut() = Some(rememberCoroutineScope());
        })
        .expect("initial composition");
    held_scope.borrow_mut().take().expect("scope captured")
}

#[test]
fn cancellation_stops_a_callback_already_in_the_frame_batch() {
    let composition = Composition::new(MemoryApplier::new());
    let runtime = composition.runtime_handle();
    let clock = runtime.frame_clock();
    let later = Rc::new(RefCell::new(None));
    let ran = Rc::new(Cell::new(false));
    let cancel_later = Rc::clone(&later);
    let _first = clock.with_frame_nanos(move |_| {
        drop(cancel_later.borrow_mut().take());
    });
    let result = Rc::clone(&ran);
    *later.borrow_mut() = Some(clock.with_frame_nanos(move |_| result.set(true)));
    runtime.drain_frame_callbacks(1);
    assert!(
        !ran.get(),
        "a cancelled callback ran in the same frame batch"
    );
}

#[test]
fn cancellation_stops_a_task_already_in_the_poll_batch() {
    let composition = Composition::new(MemoryApplier::new());
    let runtime = composition.runtime_handle();
    let later = Rc::new(RefCell::new(None::<cranpose_core::TaskHandle>));
    let ran = Rc::new(Cell::new(false));
    let cancel_later = Rc::clone(&later);
    runtime.spawn_ui(async move {
        if let Some(task) = cancel_later.borrow_mut().take() {
            task.cancel();
        }
    });
    let result = Rc::clone(&ran);
    *later.borrow_mut() = runtime.spawn_ui(async move { result.set(true) });
    runtime.drain_ui();
    assert!(!ran.get(), "a cancelled task ran in the same poll batch");
}

#[test]
fn a_retained_scope_does_not_keep_removed_composition_work_alive() {
    let mut composition = Composition::new(MemoryApplier::new());
    let runtime = composition.runtime_handle();
    let scope = capture_scope(&mut composition);
    let ran = Rc::new(Cell::new(false));
    let result = Rc::clone(&ran);
    let clock = runtime.frame_clock();
    scope.launch(async move {
        clock.next_frame().await;
        result.set(true);
    });
    runtime.drain_ui();
    composition.render(1, || {}).expect("scope removed");
    runtime.drain_frame_callbacks(1);
    runtime.drain_ui();
    scope.cancel();
    assert!(
        !ran.get(),
        "work survived removal while a scope clone existed"
    );
}

#[test]
fn dropping_a_callback_can_drop_another_registration() {
    let composition = Composition::new(MemoryApplier::new());
    let runtime = composition.runtime_handle();
    let clock = runtime.frame_clock();
    let child = clock.with_frame_nanos(|_| {});
    let parent = clock.with_frame_nanos(move |_| drop(child));
    drop(parent);
    assert!(!runtime.has_frame_callbacks());
}

#[test]
fn a_removed_scope_rejects_new_work_from_a_retained_handle() {
    let mut composition = Composition::new(MemoryApplier::new());
    let runtime = composition.runtime_handle();
    let scope = capture_scope(&mut composition);
    composition.render(1, || {}).expect("scope removed");
    let ran = Rc::new(Cell::new(false));
    let result = Rc::clone(&ran);
    scope.launch(async move { result.set(true) });
    runtime.drain_ui();
    assert!(!ran.get());
}

#[test]
fn a_running_task_is_live_until_it_cancels_itself() {
    let composition = Composition::new(MemoryApplier::new());
    let runtime = composition.runtime_handle();
    let held = Rc::new(RefCell::new(None::<Rc<cranpose_core::TaskHandle>>));
    let task_handle = Rc::clone(&held);
    let polls = Rc::new(Cell::new(0));
    let poll_count = Rc::clone(&polls);
    let task = runtime
        .spawn_ui(std::future::poll_fn(move |context| {
            poll_count.set(poll_count.get() + 1);
            let held = task_handle.borrow();
            let task = held.as_ref().expect("task handle installed");
            assert!(!task.is_finished());
            task.cancel();
            assert!(task.is_finished());
            context.waker().wake_by_ref();
            std::task::Poll::<()>::Pending
        }))
        .expect("task spawned");
    let task = Rc::new(task);
    *held.borrow_mut() = Some(Rc::clone(&task));
    runtime.drain_ui();
    runtime.drain_ui();
    assert!(task.is_finished());
    assert_eq!(polls.get(), 1);
}

struct CancelTaskOnDrop(Rc<cranpose_core::TaskHandle>);

impl Drop for CancelTaskOnDrop {
    fn drop(&mut self) {
        self.0.cancel();
    }
}

#[test]
fn cancelling_a_task_can_cancel_another_task_during_cleanup() {
    let composition = Composition::new(MemoryApplier::new());
    let runtime = composition.runtime_handle();
    let child = runtime
        .spawn_ui(std::future::pending())
        .expect("child spawned");
    let child = Rc::new(child);
    let cleanup = CancelTaskOnDrop(Rc::clone(&child));
    let parent = runtime
        .spawn_ui(async move {
            let _cleanup = cleanup;
            std::future::pending::<()>().await;
        })
        .expect("parent spawned");
    parent.cancel();
    assert!(parent.is_finished());
    assert!(child.is_finished());
}

#[test]
fn callbacks_registered_during_a_frame_wait_for_the_next_frame() {
    let composition = Composition::new(MemoryApplier::new());
    let runtime = composition.runtime_handle();
    let later = Rc::new(Cell::new(false));
    let result = Rc::clone(&later);
    let register = runtime.clone();
    runtime.register_frame_callback(move |_| {
        register.register_frame_callback(move |_| result.set(true));
    });
    runtime.drain_frame_callbacks(1);
    assert!(!later.get());
    runtime.drain_frame_callbacks(2);
    assert!(later.get());
}

struct CancelContinuationOnDrop(cranpose_core::RuntimeHandle, u64);

impl Drop for CancelContinuationOnDrop {
    fn drop(&mut self) {
        self.0.cancel_ui_cont(self.1);
    }
}

#[test]
fn cancelling_a_continuation_can_cancel_another_during_cleanup() {
    let composition = Composition::new(MemoryApplier::new());
    let runtime = composition.runtime_handle();
    let child = runtime
        .register_ui_cont(|(): ()| {})
        .expect("child registered");
    let cleanup = CancelContinuationOnDrop(runtime.clone(), child);
    let parent = runtime
        .register_ui_cont(move |(): ()| {
            let _ = &cleanup;
        })
        .expect("parent registered");
    runtime.cancel_ui_cont(parent);
    assert_eq!(runtime.debug_stats().ui_conts_len, 0);
}
