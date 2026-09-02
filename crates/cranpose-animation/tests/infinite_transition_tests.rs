use std::{cell::RefCell, rc::Rc};

use cranpose_animation::{
    AnimationSpec, RepeatMode, StartOffset, infiniteRepeatable, rememberInfiniteTransition,
};
use cranpose_core::{
    Composition, MemoryApplier, MutableState, Node, NodeError, SnapshotStateObserver, State,
    location_key, with_current_composer,
};

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
fn stepped_animation_spec_is_available_to_consumers() {
    let spec = AnimationSpec::stepped(1_000, 4);

    assert_eq!(spec.easing.transform(0.74), 0.5);
}

#[test]
fn infinite_transition_drives_state_updates() {
    let mut composition = Composition::new(MemoryApplier::new());
    let runtime = composition.runtime_handle();
    let root_key = location_key(file!(), line!(), column!());
    let group_key = location_key(file!(), line!(), column!());
    let state_slot = Rc::new(RefCell::new(None::<State<f32>>));

    let mut render = {
        let state_slot = Rc::clone(&state_slot);
        move || {
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
                    let _ = state.get();
                    state_slot.borrow_mut().replace(state);
                });
            });
        }
    };
    composition
        .render(root_key, &mut render)
        .expect("initial render");

    let observer = SnapshotStateObserver::new(|callback| callback());
    let initial = observer.observe_reads(
        (),
        |_| {},
        || state_slot.borrow().as_ref().expect("state available").get(),
    );
    assert_eq!(initial, 0.0);
    runtime.drain_ui();
    composition
        .render(root_key, &mut render)
        .expect("subscriber render");

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

    let mut render = {
        let state_slot = Rc::clone(&state_slot);
        move || {
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

                    composer.with_group(location_key(file!(), line!(), column!()), |composer| {
                        if progress > 0.0 {
                            composer.with_group(
                                location_key(file!(), line!(), column!()),
                                |composer| {
                                    composer.emit_node(|| DummyNode);
                                },
                            );
                        }
                    });
                });
            });
        }
    };
    composition
        .render(root_key, &mut render)
        .expect("initial render");

    let observer = SnapshotStateObserver::new(|callback| callback());
    observer.observe_reads(
        (),
        |_| {},
        || state_slot.borrow().as_ref().expect("state available").get(),
    );
    runtime.drain_ui();
    composition
        .render(root_key, &mut render)
        .expect("subscriber render");

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
fn infinite_transition_inserted_after_state_change_advances() {
    let mut composition = Composition::new(MemoryApplier::new());
    let runtime = composition.runtime_handle();
    let root_key = location_key(file!(), line!(), column!());
    let busy = MutableState::with_runtime(false, runtime.clone());
    let state_slot = Rc::new(RefCell::new(None::<State<f32>>));

    let mut render = {
        let state_slot = Rc::clone(&state_slot);
        move || {
            let state_slot = Rc::clone(&state_slot);
            with_current_composer(|composer| {
                composer.with_group(location_key(file!(), line!(), column!()), |composer| {
                    if busy.value() {
                        let state_slot = Rc::clone(&state_slot);
                        composer.with_group(
                            location_key(file!(), line!(), column!()),
                            |composer| {
                                let transition = rememberInfiniteTransition("inserted_busy_pulse");
                                let state = transition.animateFloat(
                                    0.0,
                                    1.0,
                                    infiniteRepeatable(
                                        AnimationSpec::linear(600),
                                        RepeatMode::Restart,
                                        StartOffset::default(),
                                    ),
                                    "inserted_busy_pulse",
                                );
                                state_slot.borrow_mut().replace(state);
                                composer.emit_node(|| DummyNode);
                            },
                        );
                    }
                });
            });
        }
    };

    composition
        .render(root_key, &mut render)
        .expect("initial render");

    drain_all(&mut composition).expect("initial drain");
    assert!(
        state_slot.borrow().is_none(),
        "transition should be absent before busy state starts"
    );

    busy.set_value(true);
    composition
        .reconcile(root_key, &mut render)
        .expect("reconcile after busy state update");

    let observer = SnapshotStateObserver::new(|callback| callback());
    let initial = observer.observe_reads(
        (),
        |_| {},
        || state_slot.borrow().as_ref().expect("state available").get(),
    );
    assert_eq!(initial, 0.0);
    runtime.drain_ui();
    composition
        .reconcile(root_key, &mut render)
        .expect("reconcile subscriber restart");

    let mut time = 0u64;
    let mut last_value = initial;
    for _ in 0..40 {
        time += 16_666_667;
        runtime.drain_frame_callbacks(time);
        composition
            .reconcile(root_key, &mut render)
            .expect("reconcile after frame");
        last_value = state_slot.borrow().as_ref().expect("state available").get();
        if (last_value - initial).abs() > 0.0001 {
            return;
        }
    }

    panic!("inserted infinite transition stayed frozen: initial={initial} last={last_value}");
}

#[test]
fn infinite_transition_restarts_when_first_animation_is_inserted_later() {
    let mut composition = Composition::new(MemoryApplier::new());
    let runtime = composition.runtime_handle();
    let root_key = location_key(file!(), line!(), column!());
    let busy = MutableState::with_runtime(false, runtime.clone());
    let state_slot = Rc::new(RefCell::new(None::<State<f32>>));

    let mut render = {
        let state_slot = Rc::clone(&state_slot);
        move || {
            with_current_composer(|composer| {
                composer.with_group(location_key(file!(), line!(), column!()), |composer| {
                    let transition = rememberInfiniteTransition("late_child_pulse");
                    if busy.value() {
                        let state_slot = Rc::clone(&state_slot);
                        composer.with_group(location_key(file!(), line!(), column!()), |_| {
                            let state = transition.animateFloat(
                                0.0,
                                1.0,
                                infiniteRepeatable(
                                    AnimationSpec::linear(600),
                                    RepeatMode::Restart,
                                    StartOffset::default(),
                                ),
                                "late_child_pulse",
                            );
                            state_slot.borrow_mut().replace(state);
                        });
                    }
                });
            });
        }
    };

    composition
        .render(root_key, &mut render)
        .expect("initial render");
    drain_all(&mut composition).expect("initial drain");

    runtime.drain_frame_callbacks(16_666_667);
    composition
        .reconcile(root_key, &mut render)
        .expect("reconcile empty transition frame");

    busy.set_value(true);
    composition
        .reconcile(root_key, &mut render)
        .expect("reconcile after animation insertion");

    let observer = SnapshotStateObserver::new(|callback| callback());
    let initial = observer.observe_reads(
        (),
        |_| {},
        || state_slot.borrow().as_ref().expect("state available").get(),
    );
    assert_eq!(initial, 0.0);
    runtime.drain_ui();
    composition
        .reconcile(root_key, &mut render)
        .expect("reconcile subscriber restart");

    let mut time = 16_666_667u64;
    let mut last_value = initial;
    for _ in 0..40 {
        time += 16_666_667;
        runtime.drain_frame_callbacks(time);
        composition
            .reconcile(root_key, &mut render)
            .expect("reconcile after frame");
        last_value = state_slot.borrow().as_ref().expect("state available").get();
        if (last_value - initial).abs() > 0.0001 {
            return;
        }
    }

    panic!(
        "infinite transition did not restart after first animation insertion: initial={initial} last={last_value}"
    );
}

#[test]
fn infinite_transition_conditional_cycle_does_not_leak_slots() {
    let mut composition = Composition::new(MemoryApplier::new());
    let runtime = composition.runtime_handle();
    let root_key = location_key(file!(), line!(), column!());
    let state_slot = Rc::new(RefCell::new(None::<State<f32>>));

    let mut render = {
        let state_slot = Rc::clone(&state_slot);
        move || {
            let state_slot = Rc::clone(&state_slot);
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
                    state_slot.borrow_mut().replace(state);

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
        }
    };
    composition
        .render(root_key, &mut render)
        .expect("initial render");

    let observer = SnapshotStateObserver::new(|callback| callback());
    observer.observe_reads(
        (),
        |_| {},
        || state_slot.borrow().as_ref().expect("state available").get(),
    );
    runtime.drain_ui();
    composition
        .render(root_key, &mut render)
        .expect("subscriber render");

    drain_all(&mut composition).expect("initial drain");

    let baseline_slots = composition.debug_dump_slot_entries().len();
    let mut time = 0u64;
    for _ in 0..240 {
        time += 16_666_667;
        runtime.drain_frame_callbacks(time);
        drain_all(&mut composition).expect("drain after frame");
    }

    let final_slots = composition.debug_dump_slot_entries();
    assert!(
        final_slots.len() <= baseline_slots + 16,
        "infinite transition conditional cycle leaked slots: baseline={} final={} slots={final_slots:#?}",
        baseline_slots,
        final_slots.len(),
    );
}
