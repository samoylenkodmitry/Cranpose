use std::{cell::RefCell, rc::Rc};

use cranpose::composable;
use cranpose_core::{CompositionLocalProvider, remember};
use cranpose_testing::{
    create_headless_robot_test,
    robot::{RobotTestRule, TestRenderer},
};

use super::*;
use crate::{
    AnchorCapabilities, Plane, PlaneLabel, PlaneState, PlaneType, Pose, SpatialMoveEventType,
    SpatialPanel, Subspace, SubspaceModifier, TrackingAvailability, TrackingState,
    plane::PlaneKey,
    pose::turn_degrees,
    space::{PanelMove, Space, local_space},
    spatial_store::SpatialStore,
};

// A headset with one movable panel of 400 by 300 dp whose policy is `policy`, and a room with a table
// top at y = -0.7 and a wall at z = -3.
struct Headset {
    robot: RobotTestRule<TestRenderer>,
    space: Space,
    events: Rc<RefCell<Vec<SpatialMoveEvent>>>,
}

#[composable]
fn Content(policy: MovePolicy) {
    SpatialPanel(
        SubspaceModifier::new()
            .size(400.0, 300.0)
            .movable_with(policy),
        || {},
    );
}

fn plane(
    store: &Rc<SpatialStore>,
    key: u128,
    plane_type: PlaneType,
    label: PlaneLabel,
    pose: Pose,
    extents: [f32; 2],
) {
    let state = PlaneState {
        center_pose: pose,
        extents,
        label,
        tracking_state: TrackingState::Tracking,
    };
    store.add_plane(Plane::new(PlaneKey(key), plane_type, state, store.weak()));
}

fn headset(snap: Option<(&'static [PlaneOrientation], &'static [PlaneSemantic])>) -> Headset {
    let events = Rc::new(RefCell::new(Vec::new()));
    let heard = Rc::clone(&events);
    let held = Rc::new(RefCell::new(None));
    let keep = Rc::clone(&held);
    let robot = create_headless_robot_test(500, 400, move || {
        let space = remember(Space::new).with(Clone::clone);
        let store = remember(|| {
            let store =
                SpatialStore::new(AnchorCapabilities::default(), TrackingAvailability::Active);
            plane(
                &store,
                1,
                PlaneType::HorizontalUpwardFacing,
                PlaneLabel::Table,
                Pose::at([0.0, -0.7, -1.5]),
                [1.2, 1.2],
            );
            plane(
                &store,
                2,
                PlaneType::Vertical,
                PlaneLabel::Wall,
                Pose {
                    position: [0.0, 0.5, -3.0],
                    orientation: turn_degrees(90.0, 0.0, 0.0),
                },
                [5.0, 2.5],
            );
            store
        })
        .with(Clone::clone);
        space.attach_spatial_store(&store);
        keep.replace(Some(space.clone()));
        let heard = Rc::clone(&heard);
        let on_move = move |event| heard.borrow_mut().push(event);
        let policy = match snap {
            Some((orientations, semantics)) => MovePolicy::anchor(orientations, semantics, on_move),
            None => MovePolicy::system(on_move),
        };
        CompositionLocalProvider([local_space().provides(Some(space))], move || {
            Subspace(move || Content(policy.clone()));
        });
    });
    let space = held.take().expect("the content composed");
    Headset {
        robot,
        space,
        events,
    }
}

impl Headset {
    fn step(&mut self, event_type: SpatialMoveEventType, pose: Pose) {
        self.robot.wait_for_idle();
        let panel = self.space.panels()[0].id;
        self.space.carry(&PanelMove {
            panel,
            event_type,
            pose,
        });
    }

    fn pose(&mut self) -> Pose {
        self.robot.wait_for_idle();
        self.space.panels()[0].pose
    }
}

fn near(a: [f32; 3], b: [f32; 3]) -> bool {
    a.iter().zip(b).all(|(x, y)| (x - y).abs() < 1e-3)
}

#[test]
fn a_panel_dropped_near_a_table_that_the_policy_admits_comes_to_rest_on_it() {
    let mut headset = headset(Some((
        &[PlaneOrientation::Horizontal],
        &[PlaneSemantic::Table],
    )));
    let held = Pose::at([0.1, -0.55, -1.5]);
    headset.step(SpatialMoveEventType::Start, held);
    headset.step(SpatialMoveEventType::Moving, held);
    // The panel lies flat on the table top, its front up
    assert!(
        near(headset.pose().position, [0.1, -0.7, -1.5]),
        "{:?}",
        headset.pose()
    );
    assert!(near(
        headset.pose().rotate([0.0, 0.0, 1.0]),
        [0.0, 1.0, 0.0]
    ));
    headset.step(SpatialMoveEventType::End, held);
    let events = headset.events.borrow();
    let end = events.last().expect("the callback heard the end");
    assert_eq!(end.event_type, SpatialMoveEventType::End);
    assert!(near(end.room_pose.position, [0.1, -0.7, -1.5]), "{end:?}");
    // The subspace's origin is a meter ahead of the start: poses are relative to it
    assert!(near(end.pose.position, [0.1, -0.7, -0.5]), "{end:?}");
}

#[test]
fn the_panel_follows_the_controller_again_when_it_leaves_the_snap_distance() {
    let mut headset = headset(Some((&[], &[])));
    headset.step(SpatialMoveEventType::Start, Pose::at([0.0, 0.3, -1.5]));
    headset.step(SpatialMoveEventType::Moving, Pose::at([0.0, -0.55, -1.5]));
    assert!(near(headset.pose().position, [0.0, -0.7, -1.5]));
    let free = Pose::at([0.0, 0.6, -1.5]);
    headset.step(SpatialMoveEventType::Moving, free);
    assert!(
        near(headset.pose().position, free.position),
        "{:?}",
        headset.pose()
    );
}

#[test]
fn a_panel_near_a_wall_lies_against_it_when_the_policy_asks_for_walls() {
    let mut headset = headset(Some((&[PlaneOrientation::Vertical], &[])));
    headset.step(SpatialMoveEventType::Start, Pose::at([0.5, 0.5, -2.8]));
    headset.step(SpatialMoveEventType::Moving, Pose::at([0.5, 0.5, -2.8]));
    let pose = headset.pose();
    assert!(near(pose.position, [0.5, 0.5, -3.0]), "{pose:?}");
    assert!(
        near(pose.rotate([0.0, 0.0, 1.0]), [0.0, 0.0, 1.0]),
        "{pose:?}"
    );
}

#[test]
fn a_plane_that_the_policy_does_not_admit_leaves_the_panel_where_the_user_holds_it() {
    let mut headset = headset(Some((
        &[PlaneOrientation::Vertical],
        &[PlaneSemantic::Wall],
    )));
    let near_the_table = Pose::at([0.0, -0.55, -1.5]);
    headset.step(SpatialMoveEventType::Start, near_the_table);
    headset.step(SpatialMoveEventType::Moving, near_the_table);
    assert!(near(headset.pose().position, near_the_table.position));
}

#[test]
fn a_policy_that_does_not_ask_for_planes_moves_the_panel_freely() {
    let mut headset = headset(None);
    let near_the_table = Pose::at([0.0, -0.55, -1.5]);
    headset.step(SpatialMoveEventType::Start, near_the_table);
    headset.step(SpatialMoveEventType::Moving, near_the_table);
    assert!(near(headset.pose().position, near_the_table.position));
}

#[test]
fn policies_compare_by_what_they_snap_to_and_who_they_call() {
    let on_move = |_: SpatialMoveEvent| {};
    let walls = MovePolicy::anchor(&[PlaneOrientation::Vertical], &[], on_move);
    assert_eq!(walls, walls.clone());
    let tables = MovePolicy::anchor(&[PlaneOrientation::Horizontal], &[], on_move);
    assert_ne!(walls, tables);
    assert_ne!(walls, MovePolicy::system(on_move));
    assert!(format!("{walls:?}").contains("snaps: true"));
}
