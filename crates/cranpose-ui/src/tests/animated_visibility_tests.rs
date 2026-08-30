use std::{
    cell::{Cell, RefCell},
    rc::Rc,
};

use cranpose_animation::{AnimationType, Easing, tween};
use cranpose_core::{DisposableEffectResult, MutableState};
use cranpose_macros::composable;

use crate::{
    AnimatedVisibility, EnterTransition, ExitTransition, TestComposition, fade_in, fade_out,
    run_test_composition, slide_in_vertically, slide_out_vertically,
};

const FRAME_NANOS: u64 = 16_666_667;
const TRANSITION_MILLIS: u64 = 160;

fn transition_spec() -> AnimationType {
    tween(TRANSITION_MILLIS, Easing::LinearEasing)
}

fn drain(composition: &mut TestComposition) {
    while composition
        .process_invalid_scopes()
        .expect("process invalid scopes")
    {}
}

fn pump_frames(composition: &mut TestComposition, frame_time: &mut u64, frames: usize) {
    let runtime = composition.runtime_handle();
    for _ in 0..frames {
        *frame_time += FRAME_NANOS;
        composition.with_app_context(|| {
            runtime.drain_frame_callbacks(*frame_time);
            runtime.drain_ui();
        });
        drain(composition);
    }
}

fn settle(composition: &mut TestComposition, frame_time: &mut u64) {
    for _ in 0..120 {
        if !composition.should_render() {
            break;
        }
        pump_frames(composition, frame_time, 1);
    }
}

#[composable]
#[allow(non_snake_case)]
fn VisibilityHost(visible: MutableState<bool>, alive: Rc<Cell<bool>>) {
    let is_visible = visible.value();
    let alive_for_content = Rc::clone(&alive);
    AnimatedVisibility(
        is_visible,
        (fade_in() + slide_in_vertically(-0.5)).with_animation(transition_spec()),
        (fade_out() + slide_out_vertically(0.5)).with_animation(transition_spec()),
        move || {
            let alive = Rc::clone(&alive_for_content);
            cranpose_core::DisposableEffect!((), move |_scope| {
                alive.set(true);
                let alive = Rc::clone(&alive);
                DisposableEffectResult::new(move || {
                    alive.set(false);
                })
            });
        },
    );
}

fn visibility_composition(initial: bool) -> (TestComposition, MutableState<bool>, Rc<Cell<bool>>) {
    let alive = Rc::new(Cell::new(false));
    let visible_slot = Rc::new(RefCell::new(None::<MutableState<bool>>));

    let composition = {
        let alive = Rc::clone(&alive);
        let visible_slot = Rc::clone(&visible_slot);
        run_test_composition(move || {
            let visible = cranpose_core::rememberMutableStateOf(move || initial);
            visible_slot.borrow_mut().replace(visible);
            VisibilityHost(visible, Rc::clone(&alive));
        })
    };

    let visible = visible_slot
        .borrow()
        .as_ref()
        .copied()
        .expect("visible state captured");
    (composition, visible, alive)
}

#[test]
fn initially_hidden_content_is_never_composed() {
    let (mut composition, _visible, alive) = visibility_composition(false);
    let mut frame_time = 0u64;

    assert!(!alive.get(), "hidden content must not be composed");
    settle(&mut composition, &mut frame_time);
    assert!(
        !alive.get(),
        "hidden content must stay out of the composition"
    );
}

#[test]
fn initially_visible_content_appears_without_animation() {
    let (mut composition, _visible, alive) = visibility_composition(true);
    let mut frame_time = 0u64;

    assert!(alive.get(), "visible content should be composed right away");
    settle(&mut composition, &mut frame_time);
    assert!(alive.get());
    assert!(!composition.should_render());
}

#[test]
fn content_stays_composed_during_exit_and_leaves_after_it_completes() {
    let (mut composition, visible, alive) = visibility_composition(true);
    let mut frame_time = 0u64;
    settle(&mut composition, &mut frame_time);

    composition.with_app_context(|| visible.set_value(false));
    drain(&mut composition);
    assert!(
        alive.get(),
        "content must stay composed when the exit transition starts"
    );

    pump_frames(&mut composition, &mut frame_time, 3);
    assert!(
        alive.get(),
        "content must stay composed while the exit transition runs"
    );

    settle(&mut composition, &mut frame_time);
    assert!(
        !alive.get(),
        "content must leave the composition after the exit transition completes"
    );
    assert!(
        !composition.should_render(),
        "exit should settle with no pending animation frames"
    );
}

#[test]
fn becoming_visible_mid_exit_keeps_content_composed() {
    let (mut composition, visible, alive) = visibility_composition(true);
    let mut frame_time = 0u64;
    settle(&mut composition, &mut frame_time);

    composition.with_app_context(|| visible.set_value(false));
    drain(&mut composition);
    pump_frames(&mut composition, &mut frame_time, 2);
    assert!(alive.get(), "content composed mid-exit");

    composition.with_app_context(|| visible.set_value(true));
    drain(&mut composition);
    settle(&mut composition, &mut frame_time);
    assert!(
        alive.get(),
        "content must remain composed after re-entering mid-exit"
    );
}

#[test]
fn hidden_then_shown_content_enters_the_composition() {
    let (mut composition, visible, alive) = visibility_composition(false);
    let mut frame_time = 0u64;
    settle(&mut composition, &mut frame_time);
    assert!(!alive.get());

    composition.with_app_context(|| visible.set_value(true));
    drain(&mut composition);
    assert!(
        alive.get(),
        "content should be composed as soon as the enter transition starts"
    );

    settle(&mut composition, &mut frame_time);
    assert!(alive.get());
}

#[test]
fn enter_and_exit_transitions_combine_with_plus() {
    let enter = fade_in() + slide_in_vertically(-0.5);
    assert!(enter.has_fade());
    assert_eq!(enter.slide_vertical_fraction(), Some(-0.5));
    assert_eq!(enter.animation(), AnimationType::default());

    let enter = enter.with_animation(transition_spec());
    assert_eq!(enter.animation(), transition_spec());

    let exit = slide_out_vertically(0.25) + fade_out();
    assert!(exit.has_fade());
    assert_eq!(exit.slide_vertical_fraction(), Some(0.25));

    let spec_a = tween(100, Easing::LinearEasing);
    let spec_b = tween(200, Easing::EaseIn);
    let combined =
        fade_in().with_animation(spec_a) + slide_in_vertically(-1.0).with_animation(spec_b);
    assert_eq!(combined.animation(), spec_a);

    let none = EnterTransition::none();
    assert!(!none.has_fade());
    assert_eq!(none.slide_vertical_fraction(), None);
    let none = ExitTransition::none();
    assert!(!none.has_fade());
    assert_eq!(none.slide_vertical_fraction(), None);
}
