use std::{cell::RefCell, rc::Rc};

use cranpose_core::{
    DisposableEffect, OwnedMutableState, mutableStateOf, ownedMutableStateOf,
    snapshot_v2::register_apply_observer,
};
use cranpose_foundation::{
    lazy::{LazyListScope, rememberLazyListState},
    text::TextFieldState,
};
use cranpose_ui::{
    Modifier, TestComposition, Text, TextStyle, UnhandledKeyEvents, announce, measure_layout,
    run_test_composition,
    widgets::{
        animated_visibility::{AnimatedVisibility, EnterTransition, ExitTransition},
        basic_text_field::BasicTextField,
        icon::IconButton,
        lazy_list::{LazyColumn, LazyColumnSpec},
        scaffold::Scaffold,
        slider::{Slider, SliderSpec},
        spacer::Spacer,
    },
};
use cranpose_ui_graphics::Size;

use crate::thread_exit::{
    exit_while_another_thread_composes, hold_composition_at_thread_exit, live_state,
    touch_the_holder_after_another_runtime, touch_the_holder_first,
};

thread_local! {
    static HELD_SCREEN: RefCell<Option<TestComposition>> = const { RefCell::new(None) };
}

fn ui_calls_on_dispose() {
    DisposableEffect((), |scope| {
        scope.on_dispose(|| {
            announce("closed");
            drop(Modifier::empty().fill_max_width());
        })
    });
}

fn screen() {
    live_state();
    ui_calls_on_dispose();
    UnhandledKeyEvents(|_| false);
    announce("left behind");
    Scaffold(
        Modifier::empty().clip_to_bounds(),
        || {
            IconButton(Modifier::empty(), "Close", || {}, || {});
        },
        || {
            Slider(
                Modifier::empty().size(Size::new(200.0, 48.0)),
                0.5,
                |_value| {},
                || {},
                SliderSpec::default(),
                |_scope| {},
            );
        },
        |_padding| {
            Text(
                "left behind",
                Modifier::empty().fill_max_width(),
                TextStyle::default(),
            );
            BasicTextField(
                TextFieldState::new("hello world"),
                Modifier::empty(),
                TextStyle::default(),
            );
            AnimatedVisibility(
                true,
                EnterTransition::none(),
                ExitTransition::none(),
                || {
                    Spacer(Modifier::empty().size_points(5.0, 5.0));
                },
            );
            LazyColumn(
                Modifier::empty().fill_max_size(),
                rememberLazyListState(),
                LazyColumnSpec::default(),
                |scope| {
                    scope.items(20, |_index| {
                        Spacer(Modifier::empty().size_points(50.0, 20.0));
                    });
                },
            );
        },
    );
}

fn composed_screen() -> TestComposition {
    let mut composition = run_test_composition(screen);
    let root = composition.root().expect("a composed root");
    let handle = composition.runtime_handle();
    {
        let mut applier = composition.applier_mut();
        applier.set_runtime_handle(handle);
        measure_layout(&mut applier, root, Size::new(400.0, 800.0)).expect("the screen measures");
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

fn hold_screen_at_thread_exit(before_hold: fn()) {
    exit_while_another_thread_composes(move || {
        before_hold();
        let screen = composed_screen();
        HELD_SCREEN.with(|held| *held.borrow_mut() = Some(screen));
    });
}

#[test]
fn a_thread_that_drops_its_screen_exits_with_framework_state_left_behind() {
    exit_while_another_thread_composes(|| drop(composed_screen()));
}

#[test]
fn a_thread_exits_while_its_screen_is_still_alive() {
    exit_while_another_thread_composes(|| std::mem::forget(composed_screen()));
}

#[test]
fn a_composition_held_in_a_thread_local_touched_first_drops_at_thread_exit() {
    hold_composition_at_thread_exit(touch_the_holder_first);
}

#[test]
fn a_composition_held_in_a_thread_local_touched_after_another_runtime_drops_at_thread_exit() {
    hold_composition_at_thread_exit(touch_the_holder_after_another_runtime);
}

#[test]
fn a_screen_held_in_a_thread_local_touched_first_drops_at_thread_exit() {
    hold_screen_at_thread_exit(|| HELD_SCREEN.with(|held| assert!(held.borrow().is_none())));
}

#[test]
fn a_screen_held_in_a_thread_local_touched_after_another_screen_drops_at_thread_exit() {
    hold_screen_at_thread_exit(|| {
        drop(run_test_composition(|| {}));
        HELD_SCREEN.with(|held| assert!(held.borrow().is_none()));
    });
}
