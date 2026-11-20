use std::{cell::RefCell, rc::Rc, time::Duration};

use cranpose::{MutableState, composable};
use cranpose_core::{CompositionLocalProvider, remember, rememberMutableStateOf};
use cranpose_testing::{
    create_headless_robot_test,
    robot::{RobotTestRule, TestRenderer},
};

use crate::{
    FollowMode, FollowTarget, FollowingSubspace, Hand, Pose, Recenter, SpatialMoveEventType,
    SpatialPanel, SubspaceModifier,
    pose::turn_degrees,
    space::{PanelMove, Ride, SUBSPACE_ORIGIN, Space, local_space},
};

// A headset with a following subspace that holds a movable panel, and a second one that an app shows
// later.
struct Headset {
    robot: RobotTestRule<TestRenderer>,
    space: Space,
    second: MutableState<bool>,
}

#[composable]
fn Later(shown: MutableState<bool>) {
    if shown.get() {
        FollowingSubspace(FollowTarget::View(FollowMode::Snap), || {
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
            FollowingSubspace(FollowTarget::View(FollowMode::Snap), || {
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
    // Tells the space where the head is, with no controller tracked.
    fn track(&self, head: Option<Pose>, time: Duration, recenter: Option<Recenter>) {
        self.space.track(head, [None, None], time, recenter);
    }

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
fn a_following_subspace_stands_in_front_of_the_head_and_again_after_a_recenter() {
    let mut headset = headset();
    // The headset tracks no head yet: the panel stands where a subspace puts it
    assert!(same(&headset.poses()[0], &SUBSPACE_ORIGIN));

    // The first tracked frame puts the panel a meter ahead of the head, turned to face it
    let first_head = head([2.0, 1.5, 3.0], 90.0);
    headset.track(Some(first_head), seconds(1), None);
    let ahead_of_first = head([1.0, 1.5, 3.0], 90.0);
    assert!(same(&headset.poses()[0], &ahead_of_first));

    // The head moves, and the panel stays: it is static
    headset.track(Some(head([0.0, 1.6, 0.0], 0.0)), seconds(2), None);
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
    headset.track(Some(third_head), seconds(2), Some(recenter));
    assert!(same(&headset.poses()[0], &ahead_of_first));
    headset.track(Some(third_head), seconds(3), None);
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
    headset.track(Some(first_head), seconds(4), Some(again));
    let relative = ahead_of_third.inverse().then(&carried);
    assert!(same(&headset.poses()[0], &ahead_of_first.then(&relative)));
}

// A headset whose app puts a ball of `diameter` dp 10 cm above the origin of a subspace that
// follows `target`.
struct Ball {
    robot: RobotTestRule<TestRenderer>,
    space: Space,
    diameter: MutableState<f32>,
}

fn ball(target: FollowTarget) -> Ball {
    let held = Rc::new(RefCell::new(None));
    let keep = Rc::clone(&held);
    let robot = create_headless_robot_test(500, 400, move || {
        let space = remember(Space::new).with(Clone::clone);
        let diameter = rememberMutableStateOf(|| 40.0);
        keep.replace(Some((space.clone(), diameter)));
        let target = target.clone();
        CompositionLocalProvider([local_space().provides(Some(space))], move || {
            FollowingSubspace(target, move || {
                let size = diameter.get();
                SpatialPanel(
                    SubspaceModifier::new()
                        .size(size, size)
                        .offset(0.0, 100.0, 0.0),
                    || {},
                );
            });
        });
    });
    let (space, diameter) = held.take().expect("the content composed");
    Ball {
        robot,
        space,
        diameter,
    }
}

impl Ball {
    // The ball as the frame sees it, with no composition between the tracking and the read.
    fn slot(&self) -> crate::space::PanelSlot {
        self.space.panels()[0]
    }

    // Tells the space where the controllers are in a frame at `time`.
    fn track(&self, controllers: [Option<Pose>; 2], time: Duration) {
        self.space.track(None, controllers, time, None);
    }
}

// A controller at `position` that points up by `pitch` degrees, is turned left by `yaw` and
// rolled by `roll`.
fn controller(position: [f32; 3], pitch: f32, yaw: f32, roll: f32) -> Pose {
    Pose {
        position,
        orientation: turn_degrees(pitch, yaw, roll),
    }
}

// Whether the ball stands at `position` and turned left by `yaw` degrees only.
fn upright_at(slot: &crate::space::PanelSlot, position: [f32; 3], yaw: f32) -> bool {
    same(&slot.pose, &head(position, yaw))
}

#[test]
fn a_subspace_follows_its_controller_in_every_frame_and_stays_upright() {
    let mut ball = ball(FollowTarget::Controller(Hand::Left, FollowMode::Tight));
    // The runtime does not track the controller: the ball shows nowhere
    ball.robot.wait_for_idle();
    assert_eq!(ball.slot().ride, Ride::Hidden);

    // The controller is tilted and rolled. The ball stands 10 cm straight above it and turns
    // with its heading only. The read comes right after the tracking: a frame hit-tests the
    // panels before it composes.
    let tilted = controller([0.3, 1.0, -0.4], 50.0, 30.0, 20.0);
    ball.track([Some(tilted), None], seconds(1));
    assert_eq!(ball.slot().ride, Ride::On(Hand::Left));
    assert!(upright_at(&ball.slot(), [0.3, 1.1, -0.4], 30.0));

    // Each frame moves it again
    for (frame, (position, pitch, yaw, roll)) in [
        ([-0.2, 1.3, -0.6], -20.0, -45.0, -10.0),
        ([0.0, 0.8, 0.1], 70.0, 120.0, 90.0),
    ]
    .into_iter()
    .enumerate()
    {
        let moved = controller(position, pitch, yaw, roll);
        ball.track([Some(moved), None], seconds(2 + frame as u64));
        let up = [position[0], position[1] + 0.1, position[2]];
        assert!(upright_at(&ball.slot(), up, yaw));
    }

    // A composition of the ball, here because its size changed, keeps it at the controller
    ball.diameter.set(50.0);
    ball.robot.wait_for_idle();
    assert_eq!(ball.space.panels()[0].size(), [50.0, 50.0]);
    assert!(upright_at(&ball.slot(), [0.0, 0.9, 0.1], 120.0));

    // The other hand's controller does not move it
    ball.track(
        [
            Some(controller([0.0, 0.8, 0.1], 70.0, 120.0, 90.0)),
            Some(controller([5.0, 5.0, 5.0], 0.0, 0.0, 0.0)),
        ],
        seconds(5),
    );
    assert!(upright_at(&ball.slot(), [0.0, 0.9, 0.1], 120.0));

    // The runtime loses the controller and finds it again
    ball.track([None, None], seconds(6));
    assert_eq!(ball.slot().ride, Ride::Hidden);
    ball.track([Some(tilted), None], seconds(7));
    assert_eq!(ball.slot().ride, Ride::On(Hand::Left));
    assert!(upright_at(&ball.slot(), [0.3, 1.1, -0.4], 30.0));
}

#[test]
fn a_controller_that_points_straight_up_keeps_the_heading_that_it_had() {
    let ball = ball(FollowTarget::Controller(Hand::Right, FollowMode::Tight));
    ball.track(
        [None, Some(controller([0.0, 1.0, 0.0], 0.0, 60.0, 0.0))],
        seconds(1),
    );
    assert!(upright_at(&ball.slot(), [0.0, 1.1, 0.0], 60.0));
    ball.track(
        [None, Some(controller([0.0, 1.2, 0.0], 90.0, 60.0, 0.0))],
        seconds(2),
    );
    assert!(upright_at(&ball.slot(), [0.0, 1.3, 0.0], 60.0));
}

#[test]
fn a_snap_subspace_of_a_controller_places_once_and_stays_when_the_controller_is_lost() {
    let ball = ball(FollowTarget::Controller(Hand::Left, FollowMode::Snap));
    assert_eq!(ball.slot().ride, Ride::Hidden);
    ball.track(
        [Some(controller([0.5, 1.0, -0.5], 10.0, 90.0, 0.0)), None],
        seconds(1),
    );
    assert!(upright_at(&ball.slot(), [0.5, 1.1, -0.5], 90.0));
    ball.track(
        [Some(controller([2.0, 1.0, -0.5], 0.0, 0.0, 0.0)), None],
        seconds(2),
    );
    assert!(upright_at(&ball.slot(), [0.5, 1.1, -0.5], 90.0));
    ball.track([None, None], seconds(3));
    assert!(upright_at(&ball.slot(), [0.5, 1.1, -0.5], 90.0));
    assert_eq!(ball.slot().ride, Ride::On(Hand::Left));

    // A recenter places it again, at the first frame that tracks the controller from its time
    let recenter = Recenter {
        change_time: seconds(4),
        pose_in_previous_space: None,
    };
    ball.space
        .track(None, [None, None], seconds(3), Some(recenter));
    assert!(upright_at(&ball.slot(), [0.5, 1.1, -0.5], 90.0));
    ball.track(
        [Some(controller([1.0, 1.0, 1.0], 0.0, 0.0, 0.0)), None],
        seconds(4),
    );
    assert!(upright_at(&ball.slot(), [1.0, 1.1, 1.0], 0.0));
}

#[test]
fn a_tight_subspace_of_the_view_follows_the_head_in_every_frame() {
    let ball = ball(FollowTarget::View(FollowMode::Tight));
    // Until the head is tracked the ball stands where a subspace puts it
    assert!(same(
        &ball.slot().pose,
        &SUBSPACE_ORIGIN.then(&Pose::at([0.0, 0.1, 0.0]))
    ));
    assert_eq!(ball.slot().ride, Ride::Free);
    for (frame, (position, yaw)) in [([2.0, 1.5, 3.0], 90.0), ([0.0, 1.6, 0.0], 0.0)]
        .into_iter()
        .enumerate()
    {
        ball.space.track(
            Some(head(position, yaw)),
            [None, None],
            seconds(1 + frame as u64),
            None,
        );
        let ahead = head(position, yaw).then(&Pose::at([0.0, 0.0, -1.0]));
        assert!(same(
            &ball.slot().pose,
            &ahead.then(&Pose::at([0.0, 0.1, 0.0]))
        ));
    }
}

// A headset whose app holds a panel to an anchor of its own store, 10 cm above the anchor.
struct OnAnchor {
    robot: RobotTestRule<TestRenderer>,
    space: Space,
    store: Rc<crate::spatial_store::SpatialStore>,
    anchor: crate::Anchor,
}

fn on_anchor(mode: FollowMode, persisted: Option<crate::Uuid>) -> OnAnchor {
    let held = Rc::new(RefCell::new(None));
    let keep = Rc::clone(&held);
    let robot = create_headless_robot_test(500, 400, move || {
        let space = remember(Space::new).with(Clone::clone);
        let store = remember(|| {
            crate::spatial_store::SpatialStore::new(
                crate::AnchorCapabilities {
                    anchors: crate::TrackingAvailability::Active,
                    persistence: crate::TrackingAvailability::Active,
                    max_anchors: None,
                },
                crate::TrackingAvailability::Active,
            )
        })
        .with(Clone::clone);
        let anchor = remember({
            let store = Rc::clone(&store);
            move || match persisted {
                Some(uuid) => {
                    store.set_persisted(&[uuid]);
                    store
                        .load(uuid)
                        .into_anchor()
                        .expect("the storage holds it")
                }
                None => store
                    .create_anchor(Pose::at([2.0, 0.0, -1.0]))
                    .into_anchor()
                    .expect("the store makes it"),
            }
        })
        .with(Clone::clone);
        keep.replace(Some((space.clone(), Rc::clone(&store), anchor.clone())));
        CompositionLocalProvider([local_space().provides(Some(space))], move || {
            FollowingSubspace(FollowTarget::Anchor(anchor.clone(), mode), || {
                SpatialPanel(
                    SubspaceModifier::new()
                        .size(100.0, 100.0)
                        .offset(0.0, 100.0, 0.0),
                    || {},
                );
            });
        });
    });
    let (space, store, anchor) = held.take().expect("the content composed");
    OnAnchor {
        robot,
        space,
        store,
        anchor,
    }
}

impl OnAnchor {
    fn slot(&mut self) -> crate::space::PanelSlot {
        self.robot.wait_for_idle();
        self.space.panels()[0]
    }

    // A frame of the runtime: the anchor's pose, then the space's follow.
    fn frame(&self, pose: Option<Pose>, tracking: crate::TrackingState, time: Duration) {
        self.store.publish_anchor(&self.anchor, pose, tracking);
        self.space.track(None, [None, None], time, None);
    }
}

#[test]
fn a_panel_on_an_anchor_stands_at_its_full_pose_and_follows_every_refinement() {
    let mut on = on_anchor(FollowMode::Tight, None);
    // A new anchor has its place from the start: 10 cm above it, at its turn
    assert_eq!(on.slot().ride, Ride::Free);
    assert!(same(&on.slot().pose, &Pose::at([2.0, 0.1, -1.0])));

    // The runtime puts the anchor on a wall: its pose is turned about the horizontal axis too
    let on_a_wall = Pose {
        position: [2.0, 1.0, -3.0],
        orientation: turn_degrees(90.0, 0.0, 0.0),
    };
    on.frame(Some(on_a_wall), crate::TrackingState::Tracking, seconds(1));
    assert!(same(
        &on.slot().pose,
        &on_a_wall.then(&Pose::at([0.0, 0.1, 0.0]))
    ));

    // A frame in which the anchor is paused keeps the panel where it was
    on.frame(None, crate::TrackingState::Paused, seconds(2));
    assert!(same(
        &on.slot().pose,
        &on_a_wall.then(&Pose::at([0.0, 0.1, 0.0]))
    ));
    assert_eq!(on.slot().ride, Ride::Free);
}

#[test]
fn a_snap_panel_takes_the_anchors_place_once_and_again_after_a_recenter() {
    let mut on = on_anchor(FollowMode::Snap, None);
    let first = on.slot().pose;
    on.frame(
        Some(Pose::at([4.0, 0.0, 0.0])),
        crate::TrackingState::Tracking,
        seconds(1),
    );
    assert!(
        same(&on.slot().pose, &first),
        "a snap follows no later movement"
    );
    let recenter = Recenter {
        change_time: seconds(2),
        pose_in_previous_space: None,
    };
    on.space
        .track(None, [None, None], seconds(2), Some(recenter));
    assert!(same(&on.slot().pose, &Pose::at([4.0, 0.1, 0.0])));
}

#[test]
fn a_panel_on_an_anchor_from_storage_is_hidden_until_the_runtime_places_it() {
    let uuid = crate::Uuid::from_bytes([7; 16]);
    let mut on = on_anchor(FollowMode::Tight, Some(uuid));
    assert_eq!(on.slot().ride, Ride::Hidden);
    on.frame(None, crate::TrackingState::Paused, seconds(1));
    assert_eq!(on.slot().ride, Ride::Hidden);
    let found = Pose {
        position: [1.0, 0.8, -2.0],
        orientation: turn_degrees(0.0, 40.0, 0.0),
    };
    on.frame(Some(found), crate::TrackingState::Tracking, seconds(2));
    assert_eq!(on.slot().ride, Ride::Free);
    assert!(same(
        &on.slot().pose,
        &found.then(&Pose::at([0.0, 0.1, 0.0]))
    ));
}

#[test]
fn a_detached_anchor_leaves_its_panel_where_it_was() {
    let mut on = on_anchor(FollowMode::Tight, None);
    let moved = Pose::at([3.0, 0.5, -2.0]);
    on.frame(Some(moved), crate::TrackingState::Tracking, seconds(1));
    let before = on.slot().pose;
    on.anchor.detach();
    assert_eq!(
        on.anchor.state().tracking_state,
        crate::TrackingState::Stopped
    );
    on.frame(
        Some(Pose::at([9.0, 9.0, 9.0])),
        crate::TrackingState::Tracking,
        seconds(2),
    );
    assert!(same(&on.slot().pose, &before));
    assert_eq!(on.slot().ride, Ride::Free);
}
