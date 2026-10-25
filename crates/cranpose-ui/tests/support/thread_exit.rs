use std::{
    cell::RefCell,
    future::pending,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
        mpsc,
    },
};

use cranpose_core::{
    Composition, DisposableEffect, EventSender, LaunchedEffectAsync, MemoryApplier, location_key,
    ownedMutableStateOf, remember, rememberCoroutineScope, rememberEventStream,
    rememberMutableStateOf, schedule_frame,
};

thread_local! {
    static HELD_COMPOSITION: RefCell<Option<Composition<MemoryApplier>>> =
        const { RefCell::new(None) };
}

pub(crate) fn live_state() {
    let count = rememberMutableStateOf(|| 0u32);
    let holder = remember(|| ownedMutableStateOf(String::from("held")));
    rememberCoroutineScope().launch(pending());
    let _events = rememberEventStream((), |_: EventSender<u32>| ());
    LaunchedEffectAsync((), |_| Box::pin(pending()));
    DisposableEffect((), move |scope| {
        let _ = holder.with(|state| state.value());
        scope.on_dispose(move || {
            count.set(count.get() + 1);
            schedule_frame();
        })
    });
}

fn held_composition(disposed: &Arc<AtomicBool>) -> Composition<MemoryApplier> {
    let mut composition = Composition::new(MemoryApplier::new());
    composition
        .render(location_key(file!(), line!(), column!()), &mut || {
            live_state();
            let read = rememberMutableStateOf(|| 1u32);
            let disposed = Arc::clone(disposed);
            DisposableEffect((), move |scope| {
                scope.on_dispose(move || {
                    read.set(read.get() + 1);
                    disposed.store(read.get() == 2, Ordering::Release);
                })
            });
        })
        .expect("the composition renders");
    composition
}

pub(crate) fn exit_while_another_thread_composes(body: impl FnOnce() + Send + 'static) {
    let (composing, wait_for_composing) = mpsc::channel();
    let mut worker = Some(std::thread::spawn(move || {
        body();
        wait_for_composing
            .recv()
            .expect("the joining thread composes");
    }));
    let mut joining = Composition::new(MemoryApplier::new());
    joining
        .render(location_key(file!(), line!(), column!()), &mut || {
            if let Some(worker) = worker.take() {
                composing.send(()).expect("the worker waits");
                worker.join().expect("the thread exits without a panic");
            }
        })
        .expect("the joining composition renders");
}

pub(crate) fn hold_composition_at_thread_exit(before_hold: fn()) {
    let disposed = Arc::new(AtomicBool::new(false));
    let held_disposed = Arc::clone(&disposed);
    exit_while_another_thread_composes(move || {
        before_hold();
        let composition = held_composition(&held_disposed);
        HELD_COMPOSITION.with(|held| *held.borrow_mut() = Some(composition));
    });
    assert!(
        disposed.load(Ordering::Acquire),
        "thread exit runs the held composition's dispose callbacks, which read their own writes"
    );
}

pub(crate) fn touch_the_holder_first() {
    HELD_COMPOSITION.with(|held| assert!(held.borrow().is_none()));
}

pub(crate) fn touch_the_holder_after_another_runtime() {
    drop(Composition::new(MemoryApplier::new()));
    touch_the_holder_first();
}
