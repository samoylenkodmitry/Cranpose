use super::*;

use cranpose_core::{
    location_key, with_current_composer, Composer, Composition, MemoryApplier, MutableState, Node,
    SnapshotStateObserver, State,
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
    let state_slot = Rc::new(RefCell::new(None::<State<f32>>));
    let mut render = {
        let state_slot = Rc::clone(&state_slot);
        move || {
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
        }
    };
    composition
        .render(root_key, &mut render)
        .expect("render succeeds");

    let observer = SnapshotStateObserver::new(|callback| callback());
    let initial = observer.observe_reads(
        (),
        |_| {},
        || state_slot.borrow().as_ref().expect("state available").get(),
    );
    assert_eq!(initial, 0.0);
    assert!(state_slot
        .borrow()
        .as_ref()
        .expect("state available")
        .has_subscribers());
    runtime.drain_ui();
    composition
        .render(root_key, &mut render)
        .expect("subscriber render succeeds");

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

#[test]
fn tween_factory_creates_tween_animation_type() {
    assert_eq!(
        tween(450, Easing::FastOutSlowInEasing),
        AnimationType::Tween(AnimationSpec::tween(450, Easing::FastOutSlowInEasing))
    );
}

#[test]
fn animation_type_delay_updates_tween_and_spring_specs() {
    let tween = tween(450, Easing::FastOutSlowInEasing).with_delay(37);
    let spring = spring(Spring::DampingRatioLowBouncy, Spring::StiffnessMedium).with_delay(83);

    assert!(matches!(
        tween,
        AnimationType::Tween(AnimationSpec {
            delay_millis: 37,
            ..
        })
    ));
    assert!(matches!(
        spring,
        AnimationType::Spring(SpringSpec {
            delay_millis: 83,
            ..
        })
    ));
}

/// Drives an [`Animatable`] against a standalone composition's frame clock at
/// ~60 FPS and samples the value after every frame. Returns (time_ns, value)
/// pairs including the initial state at t=0.
fn drive_spring(
    animatable: &Animatable<f32>,
    composition: &Composition<MemoryApplier>,
    frames: usize,
) -> Vec<(u64, f32)> {
    let runtime = composition.runtime_handle();
    let mut samples = vec![(0u64, animatable.state().get())];
    let mut frame_time = 0u64;
    for _ in 0..frames {
        frame_time += 16_666_667;
        runtime.drain_frame_callbacks(frame_time);
        samples.push((frame_time, animatable.state().get()));
    }
    samples
}

#[test]
fn spring_integrates_per_frame_delta_not_total_elapsed() {
    let composition: Composition<MemoryApplier> = Composition::new(MemoryApplier::new());
    let mut animatable = Animatable::new(0.0f32, composition.runtime_handle());
    animatable.animateTo(100.0, AnimationType::Spring(SpringSpec::default_spring()));

    let samples = drive_spring(&animatable, &composition, 30);

    // Critically damped, k=1500 (ω≈38.7): analytically x(50ms) ≈ 57.6 and
    // x(100ms) ≈ 89.8. The old integrator re-simulated the TOTAL elapsed time
    // every frame (compounding), reaching ≈90+ by the third frame.
    let at_50ms = samples
        .iter()
        .find(|(t, _)| *t >= 50_000_000)
        .expect("sample at 50ms")
        .1;
    assert!(
        (30.0..=75.0).contains(&at_50ms),
        "critically damped spring at 50ms should be mid-flight (analytic ≈57.6), got {at_50ms}"
    );

    let last = samples.last().expect("samples").1;
    assert!(
        (last - 100.0).abs() < 0.5,
        "spring should settle at the target, got {last}"
    );
}

#[test]
fn spring_retarget_preserves_value_space_velocity() {
    // A soft spring (ω = √50 ≈ 7 rad/s) keeps its momentum phase long enough
    // to observe across whole frames after the retarget.
    let soft = SpringSpec::new(1.0, 50.0);
    let composition: Composition<MemoryApplier> = Composition::new(MemoryApplier::new());
    let mut animatable = Animatable::new(0.0f32, composition.runtime_handle());
    animatable.animateTo(100.0, AnimationType::Spring(soft));

    drive_spring(&animatable, &composition, 4);
    let velocity_before = animatable.velocity();
    assert!(
        velocity_before > 100.0,
        "mid-flight spring should carry substantial velocity, got {velocity_before}"
    );

    // Retarget mid-flight: the physical velocity must carry over exactly.
    animatable.animateTo(-50.0, AnimationType::Spring(soft));
    let velocity_after = animatable.velocity();
    assert_eq!(
        velocity_before, velocity_after,
        "retargeting must not rescale the in-flight velocity"
    );

    // With positive carried velocity the value keeps rising briefly even
    // though the new target lies below — the droplet overshoot that makes
    // interrupted springs feel physical.
    let value_at_retarget = animatable.state().get();
    let runtime = composition.runtime_handle();
    runtime.drain_frame_callbacks(5 * 16_666_667);
    runtime.drain_frame_callbacks(6 * 16_666_667);
    let value_after = animatable.state().get();
    assert!(
        value_after > value_at_retarget,
        "carried velocity should keep the value moving in its direction first \
         ({value_at_retarget} -> {value_after})"
    );
}

#[test]
fn spring_retargeted_every_frame_tracks_a_moving_target() {
    // Continuous-gesture tracking: the target is retargeted before EVERY
    // frame (a pointer move per frame). The spring must keep integrating
    // real frame deltas across retargets — resetting the frame chain on
    // retarget makes every frame a dt=0 no-op and the value freezes until
    // the finger stops (the live loupe-follow bug).
    let spec = SpringSpec::new(1.0, 1050.0);
    let composition: Composition<MemoryApplier> = Composition::new(MemoryApplier::new());
    let mut animatable = Animatable::new(0.0f32, composition.runtime_handle());
    let runtime = composition.runtime_handle();

    let mut frame_time = 0u64;
    let mut target = 0.0f32;
    for _ in 0..30 {
        target += 10.0; // 600 units/s ramp
        let velocity = animatable.velocity();
        animatable.animate_to_with_velocity(target, velocity, AnimationType::Spring(spec));
        frame_time += 16_666_667;
        runtime.drain_frame_callbacks(frame_time);
    }

    let tracked = animatable.state().get();
    // Steady-state ramp lag for critical damping is 2v/ω ≈ 37 units here;
    // anything close to zero means the spring never integrated.
    assert!(
        tracked > target - 80.0,
        "spring must track a per-frame-retargeted ramp (target {target}, got {tracked})"
    );

    // And once the target stops, it must settle there.
    let velocity = animatable.velocity();
    animatable.animate_to_with_velocity(target, velocity, AnimationType::Spring(spec));
    for _ in 0..60 {
        frame_time += 16_666_667;
        runtime.drain_frame_callbacks(frame_time);
    }
    let settled = animatable.state().get();
    assert!(
        (settled - target).abs() < 0.5,
        "spring should settle at the final target, got {settled} vs {target}"
    );
}

#[test]
fn animate_to_with_velocity_seeds_gesture_handoff() {
    let composition: Composition<MemoryApplier> = Composition::new(MemoryApplier::new());
    let mut animatable = Animatable::new(0.0f32, composition.runtime_handle());

    // Fling released at 800 units/sec toward a spring anchored at 0: the value
    // must first travel past the anchor (analytic peak v₀/(ω·e) ≈ 20.8 for
    // ω = √200), then be pulled back.
    animatable.animate_to_with_velocity(
        0.0,
        800.0,
        AnimationType::Spring(SpringSpec::new(1.0, 200.0)),
    );

    let samples = drive_spring(&animatable, &composition, 90);
    let peak = samples
        .iter()
        .map(|(_, value)| *value)
        .fold(f32::MIN, f32::max);
    assert!(
        peak > 15.0,
        "seeded velocity should carry the value away from the anchor, peak {peak}"
    );
    let last = samples.last().expect("samples").1;
    assert!(
        last.abs() < 0.5,
        "spring should return to the anchor, got {last}"
    );
}

#[test]
fn exact_spring_start_integrates_from_the_event_clock_after_its_delay() {
    let composition: Composition<MemoryApplier> = Composition::new(MemoryApplier::new());
    let runtime = composition.runtime_handle();
    let mut animatable = Animatable::new(0.0f32, runtime.clone());
    let start = 100_000_000;

    animatable.animate_to_with_velocity_at(
        100.0,
        0.0,
        AnimationType::Spring(SpringSpec::new(1.0, 200.0).with_delay(20)),
        start,
    );

    runtime.drain_frame_callbacks(start + 10_000_000);
    assert_eq!(animatable.state().get(), 0.0);
    runtime.drain_frame_callbacks(start + 20_000_000);
    assert_eq!(animatable.state().get(), 0.0);
    runtime.drain_frame_callbacks(start + 36_666_667);
    assert!(
        animatable.state().get() > 0.0,
        "the first post-delay frame must integrate elapsed event-clock time"
    );
}

#[test]
fn tween_to_spring_transition_discards_the_stale_spring_clock() {
    let composition: Composition<MemoryApplier> = Composition::new(MemoryApplier::new());
    let runtime = composition.runtime_handle();
    let mut animatable = Animatable::new(0.0f32, runtime.clone());

    animatable.animate_to_with_velocity(
        100.0,
        500.0,
        AnimationType::Spring(SpringSpec::new(1.0, 200.0)),
    );
    runtime.drain_frame_callbacks(16_666_667);
    runtime.drain_frame_callbacks(33_333_334);
    assert!(animatable.velocity().abs() > 1.0);

    animatable.animateTo(50.0, AnimationType::Tween(AnimationSpec::linear(1)));
    runtime.drain_frame_callbacks(50_000_001);
    runtime.drain_frame_callbacks(51_000_001);
    assert_eq!(animatable.state().get(), 50.0);
    assert_eq!(animatable.velocity(), 0.0);

    animatable.animateTo(75.0, AnimationType::Spring(SpringSpec::new(1.0, 200.0)));
    runtime.drain_frame_callbacks(67_666_668);
    assert_eq!(
        animatable.state().get(),
        50.0,
        "a fresh spring uses its first callback as its clock origin"
    );
    runtime.drain_frame_callbacks(84_333_335);
    assert!(animatable.state().get() > 50.0);
}
