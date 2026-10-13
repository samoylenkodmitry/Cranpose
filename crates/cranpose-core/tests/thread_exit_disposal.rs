use std::{
    cell::RefCell,
    future::pending,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
        mpsc,
    },
    thread,
};

use cranpose_core::{
    Composition, DisposableEffect, EventSender, LaunchedEffectAsync, MemoryApplier, location_key,
    rememberEventStream, rememberMutableStateOf, schedule_frame,
};

thread_local! {
    static HELD: RefCell<Option<Composition<MemoryApplier>>> = const { RefCell::new(None) };
}

#[cranpose_macros::composable]
fn HeldContent() {
    let count = rememberMutableStateOf(|| 0u32);
    let _events = rememberEventStream((), |_: EventSender<u32>| ());
    LaunchedEffectAsync((), |_| Box::pin(pending()));
    DisposableEffect((), move |scope| {
        scope.on_dispose(move || {
            count.set(1);
            schedule_frame();
        })
    });
}

fn hold_composition(disposed: &Arc<AtomicBool>) {
    let mut composition = Composition::new(MemoryApplier::new());
    composition
        .render(location_key(file!(), line!(), column!()), &mut || {
            HeldContent();
            let disposed = Arc::clone(disposed);
            DisposableEffect((), move |scope| {
                scope.on_dispose(move || disposed.store(true, Ordering::Release))
            });
        })
        .expect("the composition renders");
    HELD.with(|held| *held.borrow_mut() = Some(composition));
}

fn exit_thread_while_composing(before_hold: fn()) {
    let disposed = Arc::new(AtomicBool::new(false));
    let held_disposed = Arc::clone(&disposed);
    let (composing, wait_for_composing) = mpsc::channel();
    let mut worker = Some(thread::spawn(move || {
        before_hold();
        hold_composition(&held_disposed);
        wait_for_composing
            .recv()
            .expect("the joining thread composes");
    }));
    let mut joining = Composition::new(MemoryApplier::new());
    joining
        .render(location_key(file!(), line!(), column!()), &mut || {
            if let Some(worker) = worker.take() {
                composing.send(()).expect("the worker waits");
                worker
                    .join()
                    .expect("thread exit drops the held composition without a panic");
            }
        })
        .expect("the joining composition renders");
    assert!(
        disposed.load(Ordering::Acquire),
        "thread exit runs the held composition's dispose callbacks"
    );
}

#[test]
fn a_composition_held_in_a_thread_local_touched_first_drops_at_thread_exit() {
    exit_thread_while_composing(|| HELD.with(|held| assert!(held.borrow().is_none())));
}

#[test]
fn a_composition_held_in_a_thread_local_touched_after_another_runtime_drops_at_thread_exit() {
    exit_thread_while_composing(|| {
        drop(Composition::new(MemoryApplier::new()));
        HELD.with(|held| assert!(held.borrow().is_none()));
    });
}
