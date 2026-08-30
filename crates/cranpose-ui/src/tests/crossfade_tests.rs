use std::{cell::RefCell, rc::Rc};

use cranpose_animation::{Easing, tween};
use cranpose_core::{DisposableEffectResult, MutableState};
use cranpose_macros::composable;

use crate::{Crossfade, TestComposition, run_test_composition};

const FRAME_NANOS: u64 = 16_666_667;
const CROSSFADE_MILLIS: u64 = 160;

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
fn CrossfadeHost(target: MutableState<u32>, alive: Rc<RefCell<Vec<u32>>>) {
    let current = target.value();
    let alive_for_content = Rc::clone(&alive);
    Crossfade(
        current,
        tween(CROSSFADE_MILLIS, Easing::LinearEasing),
        move |value: u32| {
            let alive = Rc::clone(&alive_for_content);
            cranpose_core::DisposableEffect(value, move |_scope| {
                alive.borrow_mut().push(value);
                let alive = Rc::clone(&alive);
                DisposableEffectResult::new(move || {
                    alive.borrow_mut().retain(|item| *item != value);
                })
            });
        },
    );
}

fn crossfade_composition(
    initial: u32,
) -> (TestComposition, MutableState<u32>, Rc<RefCell<Vec<u32>>>) {
    let alive = Rc::new(RefCell::new(Vec::<u32>::new()));
    let target_slot = Rc::new(RefCell::new(None::<MutableState<u32>>));

    let composition = {
        let alive = Rc::clone(&alive);
        let target_slot = Rc::clone(&target_slot);
        run_test_composition(move || {
            let target = cranpose_core::rememberMutableStateOf(move || initial);
            target_slot.borrow_mut().replace(target);
            CrossfadeHost(target, Rc::clone(&alive));
        })
    };

    let target = target_slot
        .borrow()
        .as_ref()
        .copied()
        .expect("target state captured");
    (composition, target, alive)
}

#[test]
fn crossfade_shows_initial_content_immediately() {
    let (mut composition, _target, alive) = crossfade_composition(1);
    let mut frame_time = 0u64;

    assert_eq!(
        alive.borrow().as_slice(),
        &[1],
        "initial content should be composed right away"
    );

    settle(&mut composition, &mut frame_time);
    assert_eq!(
        alive.borrow().as_slice(),
        &[1],
        "initial content should stay composed with no transition running"
    );
    assert!(!composition.should_render());
}

#[test]
fn crossfade_keeps_both_contents_during_transition_and_removes_old_after() {
    let (mut composition, target, alive) = crossfade_composition(1);
    let mut frame_time = 0u64;
    settle(&mut composition, &mut frame_time);

    composition.with_app_context(|| target.set_value(2));
    drain(&mut composition);
    assert_eq!(
        alive.borrow().as_slice(),
        &[1, 2],
        "both contents should be composed as soon as the transition starts"
    );

    pump_frames(&mut composition, &mut frame_time, 3);
    assert_eq!(
        alive.borrow().as_slice(),
        &[1, 2],
        "both contents should stay composed mid-transition"
    );

    settle(&mut composition, &mut frame_time);
    assert_eq!(
        alive.borrow().as_slice(),
        &[2],
        "old content should be removed after its fade-out completes"
    );
    assert!(
        !composition.should_render(),
        "crossfade should settle with no pending animation frames"
    );
}

#[test]
fn crossfade_retargeting_mid_transition_restores_previous_content() {
    let (mut composition, target, alive) = crossfade_composition(1);
    let mut frame_time = 0u64;
    settle(&mut composition, &mut frame_time);

    composition.with_app_context(|| target.set_value(2));
    drain(&mut composition);
    pump_frames(&mut composition, &mut frame_time, 2);
    assert_eq!(alive.borrow().as_slice(), &[1, 2]);

    composition.with_app_context(|| target.set_value(1));
    drain(&mut composition);
    assert_eq!(
        alive.borrow().as_slice(),
        &[1, 2],
        "both contents remain composed after retargeting mid-transition"
    );

    settle(&mut composition, &mut frame_time);
    assert_eq!(
        alive.borrow().as_slice(),
        &[1],
        "interrupted content should be removed once its fade-out completes"
    );
}
