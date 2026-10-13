use std::{rc::Rc, sync::mpsc};

use cranpose_core::{
    Composition, DisposableEffect, LaunchedEffectAsync, MemoryApplier, OwnedMutableState,
    location_key, mutableStateOf, ownedMutableStateOf, remember, rememberCoroutineScope,
    rememberEventStream, rememberMutableStateOf, snapshot_v2::register_apply_observer,
};
use cranpose_ui::{
    Modifier, TestComposition, Text, TextStyle, UnhandledKeyEvents, announce, measure_layout,
    run_test_composition,
    widgets::{
        box_widget::{Box, BoxSpec},
        column::{Column, ColumnSpec},
    },
};
use cranpose_ui_graphics::Size;

fn screen() {
    let count = rememberMutableStateOf(|| 0u32);
    let holder = remember(|| ownedMutableStateOf(String::from("held")));
    rememberCoroutineScope().launch(std::future::pending());
    let _events = rememberEventStream::<u32, _, _, _>((), |_sender| ());
    DisposableEffect((), move |scope| {
        let _ = holder.with(|state| state.value());
        scope.on_dispose(move || count.set(1))
    });
    LaunchedEffectAsync((), |_| std::boxed::Box::pin(std::future::pending()));
    Column(
        Modifier::empty().fill_max_width().clip_to_bounds(),
        ColumnSpec::default(),
        || {
            Text(
                "left behind",
                Modifier::empty().fill_max_width(),
                TextStyle::default(),
            );
            Box(
                Modifier::empty().fill_max_size().height(10.0),
                BoxSpec::default(),
                || {},
            );
        },
    );
    UnhandledKeyEvents(|_| false);
    announce("left behind");
}

fn composed_with_state_left_behind() -> TestComposition {
    let mut composition = run_test_composition(screen);
    let root = composition.root().expect("a composed root");
    let handle = composition.runtime_handle();
    {
        let mut applier = composition.applier_mut();
        applier.set_runtime_handle(handle);
        measure_layout(&mut applier, root, Size::new(200.0, 400.0)).expect("the screen measures");
        applier.clear_runtime_handle();
    }
    let observed: OwnedMutableState<u32> = ownedMutableStateOf(7);
    std::mem::forget(register_apply_observer(Rc::new(move |_, _| {
        let _ = observed.value();
    })));
    let pending = mutableStateOf(None::<OwnedMutableState<u32>>);
    pending.set(Some(ownedMutableStateOf(9)));
    composition
}

fn exit_while_another_thread_composes(body: impl FnOnce() + Send + 'static) {
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

#[test]
fn a_thread_that_drops_its_composition_exits_with_framework_state_left_behind() {
    exit_while_another_thread_composes(|| drop(composed_with_state_left_behind()));
}

#[test]
fn a_thread_exits_while_its_composition_is_still_alive() {
    exit_while_another_thread_composes(|| std::mem::forget(composed_with_state_left_behind()));
}
