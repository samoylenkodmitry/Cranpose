use super::*;

use crate::animation::{AnimationSpec, AnimationType, Lerp, SpringSpec};
use cranpose_core::{location_key, with_current_composer, Composition, MemoryApplier, State};
use std::cell::RefCell;
use std::rc::Rc;

fn assert_color_near(actual: Color, expected: Color, tolerance: f32) {
    let channels = [
        (actual.0, expected.0, "r"),
        (actual.1, expected.1, "g"),
        (actual.2, expected.2, "b"),
        (actual.3, expected.3, "a"),
    ];
    for (actual, expected, name) in channels {
        assert!(
            (actual - expected).abs() <= tolerance,
            "channel {name} should be near {expected}, got {actual}"
        );
    }
}

#[test]
fn color_lerp_interpolates_each_channel_including_alpha() {
    let start = Color(0.0, 0.2, 0.4, 0.0);
    let end = Color(1.0, 0.6, 0.8, 1.0);

    assert_color_near(start.lerp(&end, 0.5), Color(0.5, 0.4, 0.6, 0.5), 1e-6);
    assert_color_near(start.lerp(&end, 0.25), Color(0.25, 0.3, 0.5, 0.25), 1e-6);
}

#[test]
fn color_lerp_endpoints_match_inputs() {
    let start = Color(0.1, 0.2, 0.3, 0.4);
    let end = Color(0.9, 0.8, 0.7, 0.6);

    assert_color_near(start.lerp(&end, 0.0), start, 1e-6);
    assert_color_near(start.lerp(&end, 1.0), end, 1e-6);
}

#[test]
fn color_lerp_clamps_overshoot_to_valid_range() {
    let start = Color(0.0, 0.0, 0.0, 0.0);
    let end = Color(1.0, 1.0, 1.0, 1.0);

    // Springs can overshoot the target fraction; the result must stay a
    // valid color.
    assert_color_near(start.lerp(&end, 1.5), Color(1.0, 1.0, 1.0, 1.0), 1e-6);
    assert_color_near(end.lerp(&start, 1.5), Color(0.0, 0.0, 0.0, 0.0), 1e-6);
}

#[test]
fn color_spring_progress_projects_across_all_channels() {
    let start = Color(0.0, 0.0, 0.0, 0.0);
    let target = Color(1.0, 1.0, 1.0, 1.0);
    let halfway = Color(0.5, 0.5, 0.5, 0.5);

    let progress = <Color as SpringScalar>::spring_progress(&start, &target, &halfway);
    assert!(
        (progress - 0.5).abs() < 1e-6,
        "expected 0.5, got {progress}"
    );

    // Degenerate span reports finished.
    let same = <Color as SpringScalar>::spring_progress(&start, &start, &start);
    assert!((same - 1.0).abs() < 1e-6, "expected 1.0, got {same}");
}

#[test]
fn animate_color_as_state_interpolates_over_time() {
    let mut composition = Composition::new(MemoryApplier::new());
    let runtime = composition.runtime_handle();
    let root_key = location_key(file!(), line!(), column!());
    let group_key = location_key(file!(), line!(), column!());
    let state_slot = Rc::new(RefCell::new(None::<State<Color>>));
    let target = Rc::new(RefCell::new(Color(0.0, 0.0, 0.0, 0.0)));

    let render = |composition: &mut Composition<MemoryApplier>,
                  state_slot: &Rc<RefCell<Option<State<Color>>>>,
                  target: &Rc<RefCell<Color>>| {
        let state_slot = Rc::clone(state_slot);
        let target = Rc::clone(target);
        composition
            .render(root_key, move || {
                let state_slot = Rc::clone(&state_slot);
                let target = Rc::clone(&target);
                with_current_composer(|composer| {
                    composer.with_group(group_key, |_| {
                        let state = animateColorAsState(
                            *target.borrow(),
                            AnimationType::Tween(AnimationSpec::linear(300)),
                            "background_color",
                        );
                        state_slot.borrow_mut().replace(state);
                    });
                });
            })
            .expect("render succeeds");
    };

    render(&mut composition, &state_slot, &target);
    let initial = state_slot.borrow().as_ref().expect("state available").get();
    assert_color_near(initial, Color(0.0, 0.0, 0.0, 0.0), 1e-6);

    *target.borrow_mut() = Color(1.0, 0.5, 0.25, 1.0);
    render(&mut composition, &state_slot, &target);
    assert!(composition.should_render());

    let mut frame_time = 0u64;
    let mut saw_midpoint = false;
    for _ in 0..64 {
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
            if value.0 > 0.0 && value.0 < 1.0 && value.3 > 0.0 && value.3 < 1.0 {
                saw_midpoint = true;
            }
        }
    }

    assert!(
        saw_midpoint,
        "color animation should report intermediate values incl. alpha"
    );
    let last = state_slot.borrow().as_ref().expect("state available").get();
    assert_color_near(last, Color(1.0, 0.5, 0.25, 1.0), 1e-4);
    assert!(!composition.should_render());
}

#[test]
fn animatable_color_spring_settles_at_target() {
    let composition = Composition::new(MemoryApplier::new());
    let runtime = composition.runtime_handle();
    let mut animatable = Animatable::new(Color(0.0, 0.0, 0.0, 0.0), runtime.clone());
    let state = animatable.state();

    animatable.animateTo(
        Color(1.0, 0.5, 0.25, 1.0),
        AnimationType::Spring(SpringSpec::default_spring()),
    );

    let mut frame_time = 0u64;
    for _ in 0..600 {
        frame_time += 16_666_667;
        runtime.drain_frame_callbacks(frame_time);
        if !runtime.has_frame_callbacks() {
            break;
        }
    }

    assert!(
        !runtime.has_frame_callbacks(),
        "spring should settle within the frame budget"
    );
    assert_color_near(state.get(), Color(1.0, 0.5, 0.25, 1.0), 1e-3);
}
