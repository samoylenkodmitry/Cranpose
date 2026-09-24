use super::*;

fn dynamics() -> LiquidDynamics {
    let runtime = cranpose_core::Runtime::new(std::sync::Arc::new(cranpose_core::DefaultScheduler));
    LiquidDynamics::new(runtime.handle())
}

fn settle(d: &LiquidDynamics, pos: (f32, f32), frames: usize) -> LiquidPose {
    let mut pose = d.pose();
    for _ in 0..frames {
        pose = d.advance(pos, 1.0 / 60.0);
    }
    pose
}

fn cruise(d: &LiquidDynamics, speed_dp_s: f32, frames: usize) -> LiquidPose {
    let dt = 1.0 / 60.0;
    let mut x = 0.0;
    let mut pose = d.pose();
    for _ in 0..frames {
        x += speed_dp_s * dt;
        pose = d.advance((x, 0.0), dt);
    }
    pose
}

#[test]
fn constant_speed_elongates_along_axis_and_conserves_area() {
    let d = dynamics();
    let pose = cruise(&d, 1200.0, 40);
    assert!((1.36..1.40).contains(&pose.stretch));
    assert!((pose.stretch * pose.ortho - 1.0).abs() < 1e-4);
    assert!((pose.axis.0 - 1.0).abs() < 1e-4);
}

#[test]
fn subpixel_pointer_jitter_cannot_invent_extreme_strain() {
    let d = dynamics();
    d.anchor_pointer((0.0, 0.0));
    let mut pose = d.pose();
    for sample in 1..=12 {
        pose = d.advance_pointer((sample as f32 * 0.20, 0.0), 1.0 / 60.0);
    }
    assert!(
        (pose.stretch - 1.0).abs() < 0.025,
        "subpixel travel must stay near equilibrium: {pose:?}"
    );
    assert!((pose.stretch * pose.ortho - 1.0).abs() < 1e-4);
}

#[test]
fn launch_compresses_before_cruise_stretch_wins() {
    let d = dynamics();
    d.advance((0.0, 0.0), 1.0 / 60.0);
    let dt = 1.0 / 60.0;
    let pose = d.advance((1500.0 * dt, 0.0), dt);
    assert!(pose.stretch < 1.0, "launch stretch {}", pose.stretch);
    assert!(pose.ortho > 1.0, "launch ortho {}", pose.ortho);
}

#[test]
fn launch_acceleration_leaves_a_persistent_trailing_material_wake() {
    let d = dynamics();
    let dt = 1.0 / 60.0;
    d.advance((0.0, 0.0), dt);
    let launch = d.advance((1200.0 * dt, 0.0), dt);
    assert!(
        launch.bulge_amplitude > 0.5,
        "launch must displace liquid toward the trailing edge: {launch:?}"
    );
    assert!(
        (launch.bulge_direction.abs() - std::f32::consts::PI).abs() < 0.1,
        "rightward acceleration must trail to the left: {launch:?}"
    );

    let cruise = d.advance((2400.0 * dt, 0.0), dt);
    assert!(
        cruise.bulge_amplitude > 0.25,
        "the material wake must survive beyond one pointer sample: {launch:?} -> {cruise:?}"
    );
    assert!(
        (cruise.bulge_direction.abs() - std::f32::consts::PI).abs() < 0.2,
        "the remembered wake cannot flip on the next sample: {cruise:?}"
    );
}

#[test]
fn direct_drag_cadence_remains_launch_compressed() {
    let d = dynamics();
    d.anchor_pointer((0.0, 0.0));
    d.advance_pointer((-20.0, 0.0), 0.08);
    let pose = d.advance_pointer((-40.0, 0.0), 0.03);
    assert!(
        pose.stretch <= 0.92,
        "the target's two-event launch must compress along travel: {pose:?}"
    );
    assert!(
        pose.ortho >= 1.08,
        "launch must expand across travel: {pose:?}"
    );
    assert!((pose.stretch * pose.ortho - 1.0).abs() < 1e-4);
}

#[test]
fn braking_decompresses_past_cruise_and_swells_leading_edge() {
    let d = dynamics();
    let cruise_pose = cruise(&d, 1200.0, 40);
    let dt = 1.0 / 60.0;
    let x = d.last_pos.get().unwrap().0;
    let brake = d.advance((x + 300.0 * dt, 0.0), dt);
    assert!(
        brake.stretch > cruise_pose.stretch + 0.04,
        "brake {} vs cruise {}",
        brake.stretch,
        cruise_pose.stretch
    );
    assert!(
        brake.bulge_amplitude > 0.5,
        "bulge {}",
        brake.bulge_amplitude
    );
    assert!(
        brake.ortho < cruise_pose.ortho,
        "brake ortho {}",
        brake.ortho
    );
    assert!(brake.bulge_direction.abs() < 1e-3);
}

#[test]
fn rest_decays_to_identity() {
    let d = dynamics();
    cruise(&d, 1200.0, 40);
    let pose = settle(&d, d.last_pos.get().unwrap(), 60);
    assert!(
        (pose.stretch - 1.0).abs() < 0.02,
        "stretch {}",
        pose.stretch
    );
    assert!(pose.bulge_amplitude < 0.2);
    assert!(pose.speed < 15.0);
}

#[test]
fn rest_reaches_an_exact_fixed_point_and_stops_producing_poses() {
    let d = dynamics();
    cruise(&d, 1200.0, 40);
    let resting = d.last_pos.get().unwrap();

    let mut frames_to_rest = None;
    for frame in 1..=600 {
        let pose = d.advance(resting, 1.0 / 60.0);
        if pose == d.advance(resting, 1.0 / 60.0) {
            frames_to_rest = Some(frame);
            break;
        }
    }
    let frames_to_rest = frames_to_rest.expect("a stationary lens must stop producing new poses");
    assert!(
        frames_to_rest <= 120,
        "rest took {frames_to_rest} frames of stationary travel"
    );

    let pose = d.pose();
    assert_eq!(pose.stretch, 1.0, "resting stretch {pose:?}");
    assert_eq!(pose.ortho, 1.0, "resting ortho {pose:?}");
    assert_eq!(pose.bulge_amplitude, 0.0, "resting bulge {pose:?}");
    assert_eq!(pose.speed, 0.0, "resting speed {pose:?}");

    for _ in 0..600 {
        assert_eq!(
            d.advance(resting, 1.0 / 60.0),
            pose,
            "a lens at rest must keep returning the identical pose"
        );
    }
}

#[test]
fn rest_snap_is_invisible_next_to_the_pose_it_replaces() {
    let d = dynamics();
    cruise(&d, 1200.0, 40);
    let resting = d.last_pos.get().unwrap();
    let dt = 1.0f32 / 60.0;
    let undecay = 1.0 / (-dt / RELEASE_TAU).exp();
    let mut previous = d.pose();
    for _ in 0..600 {
        let pose = d.advance(resting, dt);
        if pose.stretch == 1.0 && pose.bulge_amplitude == 0.0 && pose.speed == 0.0 {
            assert!(
                (previous.stretch - 1.0).abs() <= REST_STRETCH * undecay,
                "snap jumped stretch from {previous:?}"
            );
            assert!(
                previous.bulge_amplitude <= REST_BULGE * undecay,
                "snap jumped bulge from {previous:?}"
            );
            assert!(
                previous.speed <= REST_SPEED * undecay,
                "snap jumped speed from {previous:?}"
            );
            return;
        }
        previous = pose;
    }
    panic!("a stationary lens never reached rest: {previous:?}");
}

#[test]
fn cruising_never_snaps_to_rest() {
    let d = dynamics();
    let pose = cruise(&d, 1200.0, 40);
    assert!(pose.speed > REST_SPEED, "cruise speed {pose:?}");
    assert!(
        (pose.stretch - 1.0).abs() > REST_STRETCH,
        "cruise stretch {pose:?}"
    );

    let slow = dynamics();
    let pose = cruise(&slow, 40.0, 40);
    assert!(
        pose.speed > REST_SPEED,
        "a slow but real drag must keep its motion: {pose:?}"
    );
}

#[test]
fn axis_follows_motion_direction_and_holds_at_rest() {
    let d = dynamics();
    let dt = 1.0 / 60.0;
    d.advance((0.0, 0.0), dt);
    let mut x = 0.0;
    let mut pose = d.pose();
    for _ in 0..10 {
        x -= 900.0 * dt;
        pose = d.advance((x, 0.0), dt);
    }
    assert!((pose.axis.0 + 1.0).abs() < 1e-4, "axis {:?}", pose.axis);
    assert!(
        pose.bulge_direction.abs() < 0.1,
        "leftward launch inertia must trail toward +x: {pose:?}"
    );
    let held = settle(&d, (x, 0.0), 30);
    assert!((held.axis.0 + 1.0).abs() < 1e-4);
}

#[test]
fn frame_rate_independent_cruise() {
    let d60 = dynamics();
    let d120 = dynamics();
    let cruise60 = cruise(&d60, 1000.0, 30);
    let dt = 1.0 / 120.0;
    let mut x = 0.0;
    let mut cruise120 = d120.pose();
    for _ in 0..60 {
        x += 1000.0 * dt;
        cruise120 = d120.advance((x, 0.0), dt);
    }
    assert!(
        (cruise60.stretch - cruise120.stretch).abs() < 0.02,
        "60Hz {} vs 120Hz {}",
        cruise60.stretch,
        cruise120.stretch
    );
}

#[test]
fn teleport_re_anchors_without_energy() {
    let d = dynamics();
    d.advance((0.0, 0.0), 1.0 / 60.0);
    let pose = d.advance((4000.0, 0.0), 1.0 / 60.0);
    assert_eq!(pose, d.pose());
    assert!(
        (pose.stretch - 1.0).abs() < 1e-4,
        "teleport {}",
        pose.stretch
    );
    let resumed = cruise(&d, 800.0, 30);
    assert!(resumed.stretch > 1.01);
}

#[test]
fn vertical_travel_stretches_height_not_width() {
    let d = dynamics();
    let dt = 1.0 / 60.0;
    let mut y = 0.0;
    d.advance((0.0, 0.0), dt);
    let mut pose = d.pose();
    for _ in 0..30 {
        y += 1000.0 * dt;
        pose = d.advance((0.0, y), dt);
    }
    assert!(
        (1.30..1.34).contains(&pose.stretch),
        "stretch {}",
        pose.stretch
    );
    assert!((pose.stretch * pose.ortho - 1.0).abs() < 1e-4);
}

#[test]
fn reset_forgets_motion() {
    let d = dynamics();
    cruise(&d, 1200.0, 40);
    d.reset();
    assert_eq!(d.pose().stretch, 1.0);
    let pose = d.advance((500.0, 0.0), 1.0 / 60.0);
    assert!((pose.stretch - 1.0).abs() < 1e-4);
}
