use std::{cell::RefCell, thread};

use cranpose_foundation::{
    lazy::{LazyListScope, rememberLazyListState},
    text::TextFieldState,
};
use cranpose_ui::{
    Modifier, TestComposition, TextStyle, UnhandledKeyEvents, measure_layout, run_test_composition,
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

thread_local! {
    static HELD: RefCell<Option<TestComposition>> = const { RefCell::new(None) };
}

fn screen() {
    UnhandledKeyEvents(|_| false);
    Scaffold(
        Modifier::empty(),
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
                Modifier::empty(),
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

fn hold_screen() {
    let mut composition = run_test_composition(screen);
    let root = composition.root().expect("the screen has a root node");
    let handle = composition.runtime_handle();
    {
        let mut applier = composition.applier_mut();
        applier.set_runtime_handle(handle);
        measure_layout(&mut applier, root, Size::new(400.0, 800.0)).expect("the screen measures");
        applier.clear_runtime_handle();
    }
    HELD.with(|held| *held.borrow_mut() = Some(composition));
}

fn exit_thread(before_hold: fn()) {
    thread::spawn(move || {
        before_hold();
        hold_screen();
    })
    .join()
    .expect("thread exit drops the held screen without a panic");
}

#[test]
fn a_screen_held_in_a_thread_local_touched_first_drops_at_thread_exit() {
    exit_thread(|| HELD.with(|held| assert!(held.borrow().is_none())));
}

#[test]
fn a_screen_held_in_a_thread_local_touched_after_another_screen_drops_at_thread_exit() {
    exit_thread(|| {
        drop(run_test_composition(|| {}));
        HELD.with(|held| assert!(held.borrow().is_none()));
    });
}
