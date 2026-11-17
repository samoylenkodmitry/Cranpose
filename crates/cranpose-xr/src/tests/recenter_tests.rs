use super::*;

fn recenter(millis: u64, pose: Option<Pose>) -> Recenter {
    Recenter {
        change_time: Duration::from_millis(millis),
        pose_in_previous_space: pose,
    }
}

#[test]
fn two_recenters_before_the_app_looks_make_one_that_ends_at_the_later_time() {
    let first = recenter(10, Some(Pose::at([1.0, 0.0, 0.0])));
    let second = recenter(20, Some(Pose::at([0.0, 2.0, 0.0])));
    let both = first.then(second);
    assert_eq!(both.change_time, Duration::from_millis(20));
    assert_eq!(both.pose_in_previous_space, Some(Pose::at([1.0, 2.0, 0.0])));
}

#[test]
fn the_second_recenter_moves_by_the_turn_of_the_first() {
    // A quarter turn about y takes the second's step along -z to a step along -x
    let quarter = Pose {
        position: [0.0; 3],
        orientation: crate::pose::turn_degrees(0.0, 90.0, 0.0),
    };
    let both = recenter(1, Some(quarter)).then(recenter(2, Some(Pose::at([0.0, 0.0, -1.0]))));
    let [x, y, z] = both
        .pose_in_previous_space
        .expect("both poses are valid")
        .position;
    assert!(
        (x + 1.0).abs() < 1e-5 && y.abs() < 1e-5 && z.abs() < 1e-5,
        "{x} {y} {z}"
    );
}

#[test]
fn a_recenter_without_a_pose_leaves_the_pair_without_one() {
    let pose = Some(Pose::at([1.0, 0.0, 0.0]));
    assert_eq!(
        recenter(1, pose)
            .then(recenter(2, None))
            .pose_in_previous_space,
        None
    );
    assert_eq!(
        recenter(1, None)
            .then(recenter(2, pose))
            .pose_in_previous_space,
        None
    );
}
