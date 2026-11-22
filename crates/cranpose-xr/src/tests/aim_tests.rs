use super::*;

// A Touch Plus controller of a Meta Quest 3 with its grip at the room's origin, as a desk check
// measured it: the runtime's aim pose 63.7 mm from the node `laser_begin` of the runtime's model,
// pointing 60 degrees below the grip's forward axis.
const AIM_AT: [f32; 3] = [0.0, -0.0673, -0.0735];
const LASER_BEGIN: [f32; 3] = [0.0001, -0.0122, -0.0415];

fn controller_aim() -> Aim {
    let (sin, cos) = 60_f32.to_radians().sin_cos();
    let ray = Ray {
        origin: AIM_AT,
        direction: [0.0, -sin, -cos],
    };
    Aim::pulling(ray, Pose::at(AIM_AT), 0.0)
}

fn millimeters(a: [f32; 3], b: [f32; 3]) -> f32 {
    (0..3)
        .map(|axis| (a[axis] - b[axis]).powi(2))
        .sum::<f32>()
        .sqrt()
        * 1000.0
}

#[test]
fn a_controller_ray_starts_at_the_models_laser_begin_and_meets_a_panel_where_it_did() {
    let aim = controller_aim();
    assert!((millimeters(aim.ray.origin, LASER_BEGIN) - 63.7).abs() < 0.2);
    let started = aim.starting_near(LASER_BEGIN);
    assert!(millimeters(started.ray.origin, LASER_BEGIN) < 0.2);
    assert_eq!(started.ray.direction, aim.ray.direction);
    let panel = Pose::at([0.0, -1.0, -0.6]);
    let [before, after] = [aim.ray, started.ray].map(|ray| {
        ray.plane_point(&panel)
            .expect("the ray meets the panel's plane")
    });
    assert!(
        (after.0 - before.0 - 0.0637).abs() < 2e-4,
        "{before:?} {after:?}"
    );
    assert!(
        millimeters(
            [before.1[0], before.1[1], 0.0],
            [after.1[0], after.1[1], 0.0]
        ) < 0.01
    );
}

#[test]
fn a_tracked_hand_keeps_its_ray() {
    let hand = Aim {
        source: InputSource::Hands,
        ..controller_aim()
    };
    assert_eq!(hand.starting_near(LASER_BEGIN), hand);
}
