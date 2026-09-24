use super::*;

fn axis(initial: f32) -> (cranpose_core::Runtime, LiquidDragAxis) {
    let runtime = cranpose_core::Runtime::new(std::sync::Arc::new(cranpose_core::DefaultScheduler));
    let axis = LiquidDragAxis::new(initial, runtime.handle(), None);
    (runtime, axis)
}

fn native_trace_error(follow: Option<AnimationType>) -> f32 {
    let runtime = cranpose_core::Runtime::new(std::sync::Arc::new(cranpose_core::DefaultScheduler));
    let mut axis = LiquidDragAxis::new(0.0, runtime.handle(), follow);
    let mut origin = 1_000_000_000u64;
    let mut squared_error = 0.0;
    let mut count = 0;
    for line in include_str!("../../tests/fixtures/native_tab_drag.csv").lines() {
        let fields: Vec<_> = line.split(',').collect();
        let time: f64 = fields[1].parse().expect("trace time");
        let position: f32 = fields[2].parse().expect("trace position");
        if fields[0] == "start" {
            origin += 10_000_000_000;
            axis = LiquidDragAxis::new(position, runtime.handle(), follow);
            runtime.handle().drain_frame_callbacks(origin);
            axis.begin(position, Some((origin / 1_000_000) as i64));
            continue;
        }
        let nanos = origin + (time * 1e9) as u64;
        runtime.handle().drain_frame_callbacks(nanos);
        if fields[0] == "input" {
            axis.move_to(position, Some((nanos / 1_000_000) as i64));
        } else {
            squared_error += (axis.value() - position).powi(2);
            count += 1;
        }
    }
    assert!(count > 200);
    (squared_error / count as f32).sqrt()
}

#[test]
fn tab_follow_matches_both_native_drag_velocities_and_held_settling() {
    let following = native_trace_error(Some(spring(0.85, 650.0)));
    let direct = native_trace_error(None);
    assert!(following < 3.0, "native trace RMS: {following}");
    assert!(direct > 15.0, "direct-input counterexample RMS: {direct}");
}

#[test]
fn releasing_a_following_lens_preserves_its_rendered_velocity() {
    let runtime = cranpose_core::Runtime::new(std::sync::Arc::new(cranpose_core::DefaultScheduler));
    let axis = LiquidDragAxis::new(0.0, runtime.handle(), Some(spring(0.85, 650.0)));
    runtime.handle().drain_frame_callbacks(1_000_000_000);
    axis.begin(0.0, Some(1000));
    axis.move_to(100.0, Some(1016));
    runtime.handle().drain_frame_callbacks(1_016_000_000);
    runtime.handle().drain_frame_callbacks(1_032_000_000);
    let before = axis.animation.borrow().velocity();
    assert!(before > 0.0);
    axis.release_to(100.0, Some(1200), LiquidMotion::glide());
    assert_eq!(axis.animation.borrow().velocity(), before);
    assert!(!axis.is_dragging());
}

#[test]
fn pointer_samples_are_the_visual_coordinate_without_a_chase() {
    let (_runtime, axis) = axis(10.0);
    axis.begin(20.0, Some(0));
    assert_eq!(axis.value(), 20.0);
    axis.move_to(180.0, Some(16));
    assert_eq!(axis.value(), 180.0);
}

#[test]
fn direct_manipulation_owns_visual_selection_until_the_lens_reaches_state() {
    assert_eq!(liquid_visual_index(2, 0.0, 78.0, 4, false), 2);
    assert_eq!(liquid_visual_index(0, 2.0 * 78.0, 78.0, 4, true), 2);
    assert_eq!(liquid_visual_index(0, 1.6 * 78.0, 78.0, 4, true), 2);
    assert_eq!(liquid_visual_index(0, 1.4 * 78.0, 78.0, 4, true), 1);
    assert_eq!(liquid_visual_index(0, 2.71 * 78.0, 78.0, 4, true), 3);
    assert_eq!(liquid_visual_index(0, 3.0 * 78.0, 78.0, 4, true), 3);
    assert_eq!(liquid_visual_index(0, 99.0 * 78.0, 78.0, 4, true), 3);
    assert_eq!(liquid_visual_index(9, f32::NAN, 78.0, 4, true), 3);
    assert_eq!(liquid_visual_index(0, 78.0, 0.0, 4, true), 0);

    assert!(liquid_axis_owns_visual_selection(true, 0.0, 0.0, 78.0));
    assert!(liquid_axis_owns_visual_selection(false, 78.0, 0.0, 78.0));
    assert!(!liquid_axis_owns_visual_selection(
        false,
        78.0 * 0.05,
        0.0,
        78.0,
    ));
}

#[test]
fn pointer_sample_excites_the_incompressible_pose_before_render() {
    let (_runtime, axis) = axis(0.0);
    axis.begin(0.0, Some(0));
    axis.move_to(14.0, Some(16));
    let pose = axis.liquid_pose();
    let deformation = (pose.stretch - 1.0).abs();
    assert!(
        (0.08..=0.12).contains(&deformation),
        "the direct-input frame must deform visibly without treating one sample as extreme acceleration: {pose:?}"
    );
    assert!((pose.stretch * pose.ortho - 1.0).abs() < 1e-4);
    assert_eq!(axis.value(), 14.0);
}

#[test]
fn render_without_a_new_pointer_sample_preserves_velocity_continuity() {
    let (_runtime, axis) = axis(0.0);
    axis.runtime.drain_frame_callbacks(1_000_000);
    axis.begin(0.0, Some(0));
    axis.move_to(14.0, Some(16));
    let sampled = axis.liquid_pose();

    axis.runtime.drain_frame_callbacks(17_000_000);
    let next_frame = axis.liquid_pose();

    assert!(
        (next_frame.stretch - sampled.stretch).abs() < 0.08,
        "a render frame without input must not synthesize a brake impulse: {sampled:?} -> {next_frame:?}"
    );
    assert!((next_frame.stretch * next_frame.ortho - 1.0).abs() < 1e-4);
}

#[test]
fn controlled_retargets_wait_until_direct_manipulation_ends() {
    let (_runtime, axis) = axis(10.0);
    axis.begin(40.0, Some(0));
    axis.settle_to(90.0, LiquidMotion::snappy());
    assert_eq!(axis.value(), 40.0);
    axis.release_to(90.0, Some(16), LiquidMotion::snappy());
    assert!(!axis.is_dragging());
    assert_eq!(axis.animation.borrow().target(), 90.0);
}

#[test]
fn continuous_release_stops_translation_without_erasing_fluid_velocity() {
    let (_runtime, axis) = axis(0.0);
    axis.runtime.drain_frame_callbacks(1_000_000);
    axis.begin(0.0, Some(0));
    axis.move_to(80.0, Some(16));
    let moving = axis.liquid_pose();

    axis.finish_at(80.0, Some(17));
    assert!(!axis.is_dragging());
    assert_eq!(axis.value(), 80.0);
    assert!(moving.speed > 0.0);

    let mut relaxed = moving;
    for frame in 2..=12 {
        axis.runtime.drain_frame_callbacks(frame * 17_000_000);
        assert_eq!(axis.value(), 80.0, "frame {frame} backtracked");
        relaxed = axis.liquid_pose();
    }
    assert!(
        relaxed.speed < moving.speed,
        "shape velocity must relax even though translation stops"
    );
}

#[test]
fn released_flight_is_critically_damped() {
    let AnimationType::Spring(spec) = LiquidMotion::glide() else {
        panic!("released flight must use a spring");
    };
    assert_eq!(spec.damping_ratio, 1.0);
    assert_eq!(spec.stiffness, 500.0);
}

fn spring_of(animation: AnimationType) -> cranpose_animation::SpringSpec {
    match animation {
        AnimationType::Spring(spec) => spec,
        other => panic!("expected a spring, got {other:?}"),
    }
}

#[test]
fn a_stretching_blob_runs_its_leading_edge_ahead_of_its_trailing_edge() {
    let leading = spring_of(LiquidMotion::blob_leading());
    let trailing = spring_of(LiquidMotion::blob_trailing());
    assert!(
        leading.stiffness > trailing.stiffness,
        "the edge that runs ahead has to be the stiffer one: \
         leading {} vs trailing {}",
        leading.stiffness,
        trailing.stiffness
    );
    assert!(
        leading.damping_ratio < 1.0 && trailing.damping_ratio < 1.0,
        "both edges stay under-damped so the droplet keeps its elongation"
    );
}

#[test]
fn every_named_motion_is_a_settled_spring() {
    for (name, animation) in [
        ("snappy", LiquidMotion::snappy()),
        ("bouncy", LiquidMotion::bouncy()),
        ("smooth", LiquidMotion::smooth()),
        ("blob_leading", LiquidMotion::blob_leading()),
        ("blob_trailing", LiquidMotion::blob_trailing()),
        ("glide", LiquidMotion::glide()),
    ] {
        let spec = spring_of(animation);
        assert!(spec.stiffness > 0.0, "{name} needs a stiffness");
        assert!(spec.damping_ratio > 0.0, "{name} needs damping");
        assert_eq!(spec.delay_millis, 0, "{name} starts on the frame it is set");
    }
}
