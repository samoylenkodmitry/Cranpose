use super::*;

use cranpose_core::{
    location_key, with_current_composer, Composer, Composition, MemoryApplier, MutableState, Node,
    State,
};
use std::cell::RefCell;
use std::rc::Rc;

#[derive(Default)]
struct DummyNode;

impl Node for DummyNode {}

#[test]
fn animate_float_as_state_interpolates_over_time() {
    let mut composition = Composition::new(MemoryApplier::new());
    let runtime = composition.runtime_handle();
    let root_key = location_key(file!(), line!(), column!());
    let group_key = location_key(file!(), line!(), column!());
    let state_slot = Rc::new(RefCell::new(None::<State<f32>>));
    let target = Rc::new(RefCell::new(0.0f32));

    {
        let state_slot = Rc::clone(&state_slot);
        let target = Rc::clone(&target);
        composition
            .render(root_key, move || {
                let state_slot = Rc::clone(&state_slot);
                let target = Rc::clone(&target);
                with_current_composer(|composer| {
                    composer.with_group(group_key, |_| {
                        let state = animateFloatAsState(
                            *target.borrow(),
                            AnimationType::default(),
                            "alpha",
                        );
                        state_slot.borrow_mut().replace(state);
                    });
                });
            })
            .expect("render succeeds");
    }

    let mut samples = Vec::new();
    let initial = state_slot.borrow().as_ref().expect("state available").get();
    samples.push(initial);
    assert_eq!(samples.as_slice(), &[0.0]);
    assert!(!composition.should_render());

    *target.borrow_mut() = 1.0;

    {
        let state_slot = Rc::clone(&state_slot);
        let target = Rc::clone(&target);
        composition
            .render(root_key, move || {
                let state_slot = Rc::clone(&state_slot);
                let target = Rc::clone(&target);
                with_current_composer(|composer| {
                    composer.with_group(group_key, |_| {
                        let state = animateFloatAsState(
                            *target.borrow(),
                            AnimationType::default(),
                            "alpha",
                        );
                        state_slot.borrow_mut().replace(state);
                    });
                });
            })
            .expect("render succeeds");
    }

    let immediate = state_slot.borrow().as_ref().expect("state available").get();
    samples.push(immediate);
    assert_eq!(samples[1], 0.0);
    assert!(composition.should_render());

    let mut frame_time = 0u64;
    let mut saw_midpoint = false;
    for _ in 0..32 {
        if !composition.should_render() {
            break;
        }
        frame_time += 16_666_667; // ~60 FPS
        runtime.drain_frame_callbacks(frame_time);
        let _ = composition
            .process_invalid_scopes()
            .expect("process invalid scopes succeeds");
        if let Some(state) = state_slot.borrow().as_ref() {
            let value = state.get();
            if value > 0.0 && value < 1.0 {
                saw_midpoint = true;
            }
            samples.push(value);
        }
    }

    let last = *samples.last().expect("at least one value recorded");
    assert!(saw_midpoint, "animation should report intermediate values");
    assert!(
        (last - 1.0).abs() < f32::EPSILON,
        "animation should end at target"
    );
    assert!(!composition.should_render());
}

#[test]
fn animate_float_as_state_invalidates_composition_time_readers() {
    fn render_animation_reader(
        composer: &Composer,
        target: MutableState<f32>,
        rendered_values: Rc<RefCell<Vec<f32>>>,
    ) {
        {
            let rendered_values = Rc::clone(&rendered_values);
            composer.set_recranpose_callback(move |composer| {
                render_animation_reader(composer, target, Rc::clone(&rendered_values));
            });
        }

        let value = animateFloatAsState(
            target.value(),
            AnimationType::Tween(AnimationSpec::linear(240)),
            "alpha",
        )
        .value();
        rendered_values.borrow_mut().push(value);
        composer.emit_node(|| DummyNode);
    }

    let mut composition = Composition::new(MemoryApplier::new());
    let runtime = composition.runtime_handle();
    let root_key = location_key(file!(), line!(), column!());
    let group_key = location_key(file!(), line!(), column!());
    let target = MutableState::with_runtime(0.0f32, runtime.clone());
    let rendered_values = Rc::new(RefCell::new(Vec::<f32>::new()));

    {
        let rendered_values = Rc::clone(&rendered_values);
        composition
            .render(root_key, move || {
                let rendered_values = Rc::clone(&rendered_values);
                with_current_composer(|composer| {
                    composer.with_group(group_key, |composer| {
                        render_animation_reader(composer, target, rendered_values);
                    });
                });
            })
            .expect("initial render succeeds");
    }

    assert_eq!(rendered_values.borrow().as_slice(), &[0.0]);

    target.set_value(1.0);
    while composition
        .process_invalid_scopes()
        .expect("process target invalidation")
    {}

    assert!(
        runtime.has_frame_callbacks(),
        "target change should enqueue animation frames"
    );
    assert!(
        composition.should_render(),
        "queued animation frames should keep the composition active"
    );

    let mut frame_time = 0u64;
    for _ in 0..32 {
        if !composition.should_render() {
            break;
        }
        frame_time += 16_666_667;
        runtime.drain_frame_callbacks(frame_time);
        runtime.drain_ui();
        while composition
            .process_invalid_scopes()
            .expect("process invalid scopes succeeds")
        {}
    }

    let rendered = rendered_values.borrow();
    assert!(
        rendered.iter().any(|value| *value > 0.0 && *value < 1.0),
        "composition-time readers should observe intermediate values, got {rendered:?}",
    );
    assert!(
        rendered.len() > 3,
        "animation should invalidate composition readers across frames, got {rendered:?}",
    );
    assert!(
        (*rendered.last().expect("rendered values") - 1.0).abs() < f32::EPSILON,
        "animation should finish at target, got {rendered:?}",
    );
}

#[test]
fn infinite_repeatable_spec_stores_config() {
    let spec = infiniteRepeatable::<f32>(
        AnimationSpec::linear(1200),
        RepeatMode::Reverse,
        StartOffset::default(),
    );
    assert_eq!(spec.animation.duration_millis, 1200);
    assert_eq!(spec.repeat_mode, RepeatMode::Reverse);
    assert_eq!(spec.initial_start_offset, StartOffset::default());
}

#[test]
fn remember_infinite_transition_retains_label() {
    let mut composition = Composition::new(MemoryApplier::new());
    let root_key = location_key(file!(), line!(), column!());
    let group_key = location_key(file!(), line!(), column!());
    let transition_slot = Rc::new(RefCell::new(None::<InfiniteTransition>));

    {
        let transition_slot = Rc::clone(&transition_slot);
        composition
            .render(root_key, move || {
                let transition_slot = Rc::clone(&transition_slot);
                with_current_composer(|composer| {
                    composer.with_group(group_key, |_| {
                        let transition = rememberInfiniteTransition("demo_label");
                        transition_slot.borrow_mut().replace(transition);
                    });
                });
            })
            .expect("render succeeds");
    }

    let label = {
        let borrowed = transition_slot.borrow();
        borrowed
            .as_ref()
            .expect("transition available")
            .label()
            .to_string()
    };
    assert_eq!(label, "demo_label");
}

#[test]
fn infinite_transition_animates_float_over_time() {
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
                        let transition = rememberInfiniteTransition("pulse");
                        let state = transition.animateFloat(
                            0.0,
                            1.0,
                            infiniteRepeatable(
                                AnimationSpec::linear(1000),
                                RepeatMode::Reverse,
                                StartOffset::default(),
                            ),
                            "pulse",
                        );
                        state_slot.borrow_mut().replace(state);
                    });
                });
            })
            .expect("render succeeds");
    }

    let initial = state_slot.borrow().as_ref().expect("state available").get();
    assert_eq!(initial, 0.0);

    let mut time = 0u64;
    let mut saw_change = false;
    for _ in 0..32 {
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

    assert!(saw_change, "infinite transition should animate over time");
}

#[test]
fn easing_linear_is_identity() {
    assert_eq!(Easing::LinearEasing.transform(0.0), 0.0);
    assert_eq!(Easing::LinearEasing.transform(0.5), 0.5);
    assert_eq!(Easing::LinearEasing.transform(1.0), 1.0);
}

#[test]
fn easing_bounds_are_correct() {
    let easings = [
        Easing::LinearEasing,
        Easing::EaseIn,
        Easing::EaseOut,
        Easing::EaseInOut,
        Easing::FastOutSlowInEasing,
    ];

    for easing in easings {
        let start = easing.transform(0.0);
        let end = easing.transform(1.0);
        assert!(
            (start - 0.0).abs() < 0.01,
            "Start should be ~0 for {:?}",
            easing
        );
        assert!(
            (end - 1.0).abs() < 0.01,
            "End should be ~1 for {:?}",
            easing
        );
    }
}

#[test]
fn animation_spec_default_has_reasonable_values() {
    let spec = AnimationSpec::default();
    assert_eq!(spec.duration_millis, 300);
    assert_eq!(spec.easing, Easing::FastOutSlowInEasing);
    assert_eq!(spec.delay_millis, 0);
}

#[test]
fn spring_spec_default_is_critically_damped() {
    let spec = SpringSpec::default();
    assert_eq!(spec.damping_ratio, 1.0);
}

#[test]
fn spring_spec_bouncy_has_low_damping() {
    let spec = SpringSpec::bouncy();
    assert_eq!(spec.damping_ratio, 0.5);
    assert!(
        spec.damping_ratio < 1.0,
        "Bouncy spring should be under-damped"
    );
}

#[test]
fn spring_spec_stiff_has_high_stiffness() {
    let spec = SpringSpec::stiff();
    assert_eq!(spec.stiffness, 3000.0);
    assert!(spec.stiffness > SpringSpec::default().stiffness);
}
