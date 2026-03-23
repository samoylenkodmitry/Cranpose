use cranpose_animation::{
    infiniteRepeatable, rememberInfiniteTransition, AnimationSpec, RepeatMode, StartOffset,
};
use cranpose_core::{
    location_key, with_current_composer, Composition, MemoryApplier, Node, NodeError, State,
};
use std::cell::RefCell;
use std::rc::Rc;

#[derive(Default)]
struct DummyNode;

impl Node for DummyNode {}

fn drain_all(composition: &mut Composition<MemoryApplier>) -> Result<(), NodeError> {
    loop {
        if !composition.process_invalid_scopes()? {
            break;
        }
    }
    Ok(())
}

#[test]
fn infinite_transition_drives_state_updates() {
    let mut composition = Composition::new(MemoryApplier::new());
    let runtime = composition.runtime_handle();
    let root_key = location_key(file!(), line!(), column!());
    let group_key = location_key(file!(), line!(), column!());
    let state_slot = Rc::new(RefCell::new(None::<State<f32>>));

    {
        let state_slot = Rc::clone(&state_slot);
        composition
            .render(root_key, move || {
                let state_slot = Rc::clone(&state_slot);
                with_current_composer(|composer| {
                    composer.with_group(group_key, |_| {
                        let transition = rememberInfiniteTransition("integration_pulse");
                        let state = transition.animateFloat(
                            0.0,
                            1.0,
                            infiniteRepeatable(
                                AnimationSpec::linear(800),
                                RepeatMode::Reverse,
                                StartOffset::default(),
                            ),
                            "integration_pulse",
                        );
                        state_slot.borrow_mut().replace(state);
                    });
                });
            })
            .expect("initial render");
    }

    let initial = state_slot.borrow().as_ref().expect("state available").get();
    assert_eq!(initial, 0.0);

    let mut time = 0u64;
    let mut saw_change = false;
    for _ in 0..40 {
        time += 16_666_667;
        runtime.drain_frame_callbacks(time);
        let _ = composition
            .process_invalid_scopes()
            .expect("process invalid scopes succeeds");
        let value = state_slot.borrow().as_ref().expect("state available").get();
        if (value - initial).abs() > 0.0001 {
            saw_change = true;
            break;
        }
    }

    assert!(saw_change, "state should change as frames advance");
}

#[test]
fn infinite_transition_survives_conditional_cycle() {
    let mut composition = Composition::new(MemoryApplier::new());
    let runtime = composition.runtime_handle();
    let root_key = location_key(file!(), line!(), column!());
    let state_slot = Rc::new(RefCell::new(None::<State<f32>>));

    {
        let state_slot = Rc::clone(&state_slot);
        composition
            .render(root_key, move || {
                let state_slot = Rc::clone(&state_slot);
                with_current_composer(|composer| {
                    composer.with_group(location_key(file!(), line!(), column!()), |composer| {
                        let transition = rememberInfiniteTransition("conditional_pulse");
                        let state = transition.animateFloat(
                            0.0,
                            1.0,
                            infiniteRepeatable(
                                AnimationSpec::linear(600),
                                RepeatMode::Reverse,
                                StartOffset::default(),
                            ),
                            "conditional_pulse",
                        );
                        let progress = state.get();
                        state_slot.borrow_mut().replace(state);

                        composer.with_group(
                            location_key(file!(), line!(), column!()),
                            |composer| {
                                if progress > 0.0 {
                                    composer.with_group(
                                        location_key(file!(), line!(), column!()),
                                        |composer| {
                                            composer.emit_node(|| DummyNode);
                                        },
                                    );
                                }
                            },
                        );
                    });
                });
            })
            .expect("initial render");
    }

    drain_all(&mut composition).expect("initial drain");

    let mut last_value = state_slot.borrow().as_ref().expect("state available").get();
    let mut saw_reverse = false;
    let mut saw_forward_after_reverse = false;
    let mut time = 0u64;

    for _ in 0..800 {
        time += 16_666_667;
        runtime.drain_frame_callbacks(time);
        drain_all(&mut composition).expect("drain after frame");

        let value = state_slot.borrow().as_ref().expect("state available").get();
        if value < last_value - 0.0001 {
            saw_reverse = true;
        }
        if saw_reverse && value > last_value + 0.0001 {
            saw_forward_after_reverse = true;
            break;
        }
        last_value = value;
    }

    assert!(
        saw_forward_after_reverse,
        "transition should keep cycling when a conditional child collapses and restores",
    );
}

#[test]
fn infinite_transition_conditional_cycle_does_not_leak_slots() {
    let mut composition = Composition::new(MemoryApplier::new());
    let runtime = composition.runtime_handle();
    let root_key = location_key(file!(), line!(), column!());

    composition
        .render(root_key, move || {
            with_current_composer(|composer| {
                composer.with_group(location_key(file!(), line!(), column!()), |composer| {
                    let transition = rememberInfiniteTransition("conditional_slot_budget");
                    let state = transition.animateFloat(
                        0.0,
                        1.0,
                        infiniteRepeatable(
                            AnimationSpec::linear(600),
                            RepeatMode::Reverse,
                            StartOffset::default(),
                        ),
                        "conditional_slot_budget",
                    );
                    let progress = state.get();

                    composer.with_group(location_key(file!(), line!(), column!()), |composer| {
                        composer.emit_node(|| DummyNode);

                        composer.with_group(
                            location_key(file!(), line!(), column!()),
                            |composer| {
                                if progress > 0.0 {
                                    composer.with_group(
                                        location_key(file!(), line!(), column!()),
                                        |composer| {
                                            composer.emit_node(|| DummyNode);
                                        },
                                    );
                                }
                            },
                        );
                    });
                });
            });
        })
        .expect("initial render");

    drain_all(&mut composition).expect("initial drain");

    let baseline_slots = composition.debug_dump_all_slots().len();
    let mut time = 0u64;
    for _ in 0..240 {
        time += 16_666_667;
        runtime.drain_frame_callbacks(time);
        drain_all(&mut composition).expect("drain after frame");
    }

    let final_slots = composition.debug_dump_all_slots();
    assert!(
        final_slots.len() <= baseline_slots + 16,
        "infinite transition conditional cycle leaked slots: baseline={} final={} slots={final_slots:#?}",
        baseline_slots,
        final_slots.len(),
    );
}
