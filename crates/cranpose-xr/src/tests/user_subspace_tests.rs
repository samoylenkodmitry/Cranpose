use std::{cell::RefCell, rc::Rc, time::Duration};

use cranpose::{MutableState, composable};
use cranpose_core::{CompositionLocalProvider, remember, rememberMutableStateOf};
use cranpose_testing::{
    create_headless_robot_test,
    robot::{RobotTestRule, TestRenderer},
};

use crate::{
    LockingBehavior, Pose, Recenter, SpatialMoveEventType, SpatialPanel, SubspaceModifier,
    UserSubspace,
    pose::turn_degrees,
    space::{PanelMove, SUBSPACE_ORIGIN, Space, local_space},
};

// A headset with a user subspace that holds a movable panel, and a second one that an app shows
// later.
struct Headset {
    robot: RobotTestRule<TestRenderer>,
    space: Space,
    second: MutableState<bool>,
}

#[composable]
fn Later(shown: MutableState<bool>) {
    if shown.get() {
        UserSubspace(LockingBehavior::Static, || {
            SpatialPanel(SubspaceModifier::new().size(200.0, 100.0), || {});
        });
    }
}

fn headset() -> Headset {
    let held = Rc::new(RefCell::new(None));
    let keep = Rc::clone(&held);
    let robot = create_headless_robot_test(500, 400, move || {
        let space = remember(Space::new).with(Clone::clone);
        let second = rememberMutableStateOf(|| false);
        keep.replace(Some((space.clone(), second)));
        CompositionLocalProvider([local_space().provides(Some(space))], move || {
            UserSubspace(LockingBehavior::Static, || {
                SpatialPanel(SubspaceModifier::new().size(400.0, 300.0).movable(), || {});
            });
            Later(second);
        });
    });
    let (space, second) = held.take().expect("the content composed");
    Headset {
        robot,
        space,
        second,
    }
}

impl Headset {
    // The poses of the panels in the room, after the composition caught up.
    fn poses(&mut self) -> Vec<Pose> {
        self.robot.wait_for_idle();
        self.space.panels().iter().map(|slot| slot.pose).collect()
    }
}

// A head at `position` turned left by `yaw` degrees.
fn head(position: [f32; 3], yaw: f32) -> Pose {
    Pose {
        position,
        orientation: turn_degrees(0.0, yaw, 0.0),
    }
}

// Whether two poses are the same place and the same turn.
fn same(a: &Pose, b: &Pose) -> bool {
    let near = |x: [f32; 3], y: [f32; 3]| x.iter().zip(y).all(|(x, y)| (x - y).abs() < 1e-4);
    near(a.position, b.position)
        && [[1.0, 0.0, 0.0], [0.0, 0.0, 1.0]]
            .iter()
            .all(|axis| near(a.rotate(*axis), b.rotate(*axis)))
}

fn seconds(seconds: u64) -> Duration {
    Duration::from_secs(seconds)
}

#[test]
fn a_user_subspace_stands_in_front_of_the_head_and_again_after_a_recenter() {
    let mut headset = headset();
    // The headset tracks no head yet: the panel stands where a subspace puts it
    assert!(same(&headset.poses()[0], &SUBSPACE_ORIGIN));

    // The first tracked frame puts the panel a meter ahead of the head, turned to face it
    let first_head = head([2.0, 1.5, 3.0], 90.0);
    headset.space.track_head(Some(first_head), seconds(1), None);
    let ahead_of_first = head([1.0, 1.5, 3.0], 90.0);
    assert!(same(&headset.poses()[0], &ahead_of_first));

    // The head moves, and the panel stays: it is static
    headset
        .space
        .track_head(Some(head([0.0, 1.6, 0.0], 0.0)), seconds(2), None);
    assert!(same(&headset.poses()[0], &ahead_of_first));

    // A subspace that composes later stands ahead of the head as it is then
    headset.second.set(true);
    let poses = headset.poses();
    assert!(same(&poses[0], &ahead_of_first));
    assert!(same(&poses[1], &head([0.0, 1.6, -1.0], 0.0)));

    // A recenter takes effect at its time, not before: the head's pose is in the new room only
    // from then on
    let third_head = head([0.5, 1.0, 0.5], -90.0);
    let recenter = Recenter {
        change_time: seconds(3),
        pose_in_previous_space: None,
    };
    headset
        .space
        .track_head(Some(third_head), seconds(2), Some(recenter));
    assert!(same(&headset.poses()[0], &ahead_of_first));
    headset.space.track_head(Some(third_head), seconds(3), None);
    let ahead_of_third = head([1.5, 1.0, 0.5], -90.0);
    let poses = headset.poses();
    assert!(same(&poses[0], &ahead_of_third));
    assert!(same(&poses[1], &ahead_of_third));

    // The panel is still movable. It keeps its place relative to the origin, so the next
    // recenter takes it along.
    let id = headset.space.panels()[0].id;
    let carried = Pose::at([2.0, 1.0, 0.0]);
    headset.space.carry(&PanelMove {
        panel: id,
        event_type: SpatialMoveEventType::Moving,
        pose: carried,
    });
    assert!(same(&headset.space.panels()[0].pose, &carried));
    let again = Recenter {
        change_time: seconds(4),
        pose_in_previous_space: None,
    };
    headset
        .space
        .track_head(Some(first_head), seconds(4), Some(again));
    let relative = ahead_of_third.inverse().then(&carried);
    assert!(same(&headset.poses()[0], &ahead_of_first.then(&relative)));
}
