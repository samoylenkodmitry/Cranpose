use std::{cell::RefCell, rc::Rc};

use cranpose_core::{Composition, MemoryApplier, State, location_key, with_current_composer};

use super::*;
use crate::animation::AnimationSpec;

fn drain_frames(
    composition: &mut Composition<MemoryApplier>,
    frame_time: &mut u64,
    until_nanos: u64,
) {
    let runtime = composition.runtime_handle();
    while *frame_time < until_nanos {
        *frame_time += 16_666_667;
        runtime.drain_frame_callbacks(*frame_time);
        let _ = composition.process_invalid_scopes();
    }
}

#[test]
fn transition_with_three_children_reports_running_until_the_last_settles() {
    let mut composition = Composition::new(MemoryApplier::new());
    let root_key = location_key(file!(), line!(), column!());
    let group_key = location_key(file!(), line!(), column!());
    let transition_slot = Rc::new(RefCell::new(None::<Transition<bool>>));
    let visible = Rc::new(RefCell::new(false));

    let mut pass = {
        let transition_slot = Rc::clone(&transition_slot);
        let visible = Rc::clone(&visible);
        move || {
            let transition_slot = Rc::clone(&transition_slot);
            let target = if *visible.borrow() { 1.0 } else { 0.0 };
            with_current_composer(|composer| {
                composer.with_group(group_key, |_| {
                    let transition = updateTransition(target > 0.0, "visibility");
                    let _fast = transition.animateFloat(
                        target,
                        AnimationType::Tween(AnimationSpec::linear(100)),
                        "fast",
                    );
                    let _medium = transition.animateFloat(
                        target,
                        AnimationType::Tween(AnimationSpec::linear(300)),
                        "medium",
                    );
                    let _slow = transition.animateFloat(
                        target,
                        AnimationType::Tween(AnimationSpec::linear(500)),
                        "slow",
                    );
                    transition_slot.borrow_mut().replace(transition);
                });
            });
        }
    };

    composition
        .render(root_key, &mut pass)
        .expect("initial render succeeds");
    let is_running = || {
        transition_slot
            .borrow()
            .as_ref()
            .expect("transition available")
            .is_running()
    };
    assert!(
        !is_running(),
        "a freshly created transition with no target change yet should not be running"
    );

    *visible.borrow_mut() = true;
    composition
        .render(root_key, &mut pass)
        .expect("target change render succeeds");
    assert!(
        is_running(),
        "changing the target state should start every child animating"
    );

    let mut frame_time = 0u64;
    drain_frames(&mut composition, &mut frame_time, 150_000_000);
    assert!(
        is_running(),
        "should still report running while the slower children are mid-flight"
    );

    drain_frames(&mut composition, &mut frame_time, 600_000_000);
    assert!(
        !is_running(),
        "should stop running only once every child has settled"
    );
}

#[test]
fn transition_retargets_mid_flight_instead_of_snapping() {
    let mut composition = Composition::new(MemoryApplier::new());
    let root_key = location_key(file!(), line!(), column!());
    let group_key = location_key(file!(), line!(), column!());
    let state_slot = Rc::new(RefCell::new(None::<State<f32>>));
    let stage = Rc::new(RefCell::new(0u8));

    let mut pass = {
        let state_slot = Rc::clone(&state_slot);
        let stage = Rc::clone(&stage);
        move || {
            let state_slot = Rc::clone(&state_slot);
            let stage_value = *stage.borrow();
            with_current_composer(|composer| {
                composer.with_group(group_key, |_| {
                    let transition = updateTransition(stage_value, "stage");
                    let target = match stage_value {
                        0 => 0.0,
                        1 => 100.0,
                        _ => -60.0,
                    };
                    let state = transition.animateFloat(
                        target,
                        AnimationType::Tween(AnimationSpec::linear(400)),
                        "value",
                    );
                    state_slot.borrow_mut().replace(state);
                });
            });
        }
    };

    composition
        .render(root_key, &mut pass)
        .expect("initial render succeeds");
    let value = || state_slot.borrow().as_ref().expect("state available").get();
    assert_eq!(value(), 0.0);

    *stage.borrow_mut() = 1;
    composition
        .render(root_key, &mut pass)
        .expect("first target render succeeds");

    let mut frame_time = 0u64;
    drain_frames(&mut composition, &mut frame_time, 100_000_000);
    let mid_flight = value();
    assert!(
        mid_flight > 0.0 && mid_flight < 100.0,
        "should be mid-flight before retargeting, got {mid_flight}"
    );

    *stage.borrow_mut() = 2;
    composition
        .render(root_key, &mut pass)
        .expect("retarget render succeeds");
    assert_eq!(
        value(),
        mid_flight,
        "changing the transition's target state mid-flight must not snap the child's value"
    );

    drain_frames(&mut composition, &mut frame_time, 600_000_000);
    assert_eq!(
        value(),
        -60.0,
        "should go on to settle at the new target state's value"
    );
}
