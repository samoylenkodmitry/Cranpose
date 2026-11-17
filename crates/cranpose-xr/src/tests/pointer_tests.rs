use std::{cell::RefCell, rc::Rc};

use cranpose_core::{CompositionLocalProvider, remember};
use cranpose_testing::{
    create_headless_robot_test,
    robot::{RobotTestRule, TestRenderer},
};

use super::*;
use crate::{
    MovePolicy, Pose, SpatialMoveEvent, SpatialMoveEventType, SpatialPanel, StereoMode,
    SubspaceModifier,
    space::{SUBSPACE_ORIGIN, Shape, Space, local_space},
};

// A panel of 800 by 500 dp a meter ahead, at the atlas's top.
const PANEL: PanelSlot = PanelSlot {
    id: 1,
    kind: PanelKind::Content,
    atlas: [0.0, 0.0, 800.0, 500.0],
    pose: Pose::at([0.0, 0.0, -1.0]),
    mobility: Mobility::Fixed,
    shape: Shape::Flat,
};

// A panel of 200 by 100 dp behind the first one and below it in the atlas.
const BEHIND: PanelSlot = PanelSlot {
    id: 2,
    kind: PanelKind::Content,
    atlas: [0.0, 508.0, 200.0, 100.0],
    pose: Pose::at([0.0, 0.0, -2.0]),
    mobility: Mobility::Fixed,
    shape: Shape::Flat,
};

// A controller at `x` meters that points straight ahead, turned right by `yaw` degrees.
fn aim(x: f32, yaw: f32, trigger: f32) -> Option<Aim> {
    let (sin, cos) = yaw.to_radians().sin_cos();
    Some(Aim {
        ray: Ray {
            origin: [x, 0.0, 0.0],
            direction: [sin, 0.0, -cos],
        },
        pose: Pose::at([x, 0.0, 0.0]),
        trigger,
        stick: 0.0,
    })
}

// A controller at `x` that points at the handle under `slot`'s panel, with its trigger pulled by
// `trigger`.
fn handle_aim(slot: &PanelSlot, x: f32, trigger: f32) -> Option<Aim> {
    let (handle, _) = crate::carry::handle_pose(slot);
    let [hx, hy, hz] = handle.position;
    let length = (hx * hx + hy * hy + hz * hz).sqrt();
    Some(Aim {
        ray: Ray {
            origin: [x, 0.0, 0.0],
            direction: [hx / length, hy / length, hz / length],
        },
        pose: Pose::at([x, 0.0, 0.0]),
        trigger,
        stick: 0.0,
    })
}

// Frames 10 ms apart.
#[derive(Default)]
struct Frames {
    pointer: Pointer,
    frame: u64,
}

impl Frames {
    fn update(
        &mut self,
        aims: [Option<Aim>; 2],
        panels: &[PanelSlot],
    ) -> (Vec<PointerEvent>, Option<Cursor>) {
        self.frame += 1;
        let time = Duration::from_millis(10 * self.frame);
        let (events, cursor) = self.pointer.update(aims, time, panels);
        (events.to_vec(), cursor)
    }

    fn right(&mut self, x: f32, yaw: f32, trigger: f32) -> (Vec<PointerEvent>, Option<Cursor>) {
        self.update([None, aim(x, yaw, trigger)], &[PANEL])
    }
}

#[test]
fn a_trigger_over_a_panel_moves_presses_and_releases() {
    let mut frames = Frames::default();
    let (events, cursor) = frames.right(0.0, 0.0, 0.0);
    assert_eq!(events, [PointerEvent::Move { x: 400.0, y: 250.0 }]);
    assert_eq!(
        cursor.map(|cursor| (cursor.panel, cursor.x, cursor.y)),
        Some((1, 400.0, 250.0))
    );
    let (events, cursor) = frames.right(0.0, 0.0, 1.0);
    assert_eq!(events, [PointerEvent::Press]);
    assert!(cursor.is_some_and(|cursor| cursor.pressed));
    let (events, _) = frames.right(0.0, 0.0, 0.0);
    assert_eq!(events, [PointerEvent::Release]);
}

#[test]
fn a_half_pulled_trigger_neither_presses_nor_releases() {
    let mut frames = Frames::default();
    frames.right(0.0, 0.0, 0.0);
    assert!(frames.right(0.0, 0.0, 0.5).0.is_empty());
    assert_eq!(frames.right(0.0, 0.0, 0.7).0, [PointerEvent::Press]);
    assert!(frames.right(0.0, 0.0, 0.5).0.is_empty());
    assert_eq!(frames.right(0.0, 0.0, 0.3).0, [PointerEvent::Release]);
}

#[test]
fn a_press_takes_the_point_from_before_the_pull() {
    let mut frames = Frames::default();
    frames.right(0.0, 0.0, 0.0);
    frames.right(0.05, 0.0, 0.3);
    let (events, cursor) = frames.right(0.1, 0.0, 0.8);
    assert_eq!(
        events,
        [
            PointerEvent::Move { x: 400.0, y: 250.0 },
            PointerEvent::Press
        ]
    );
    assert_eq!(cursor.map(|cursor| cursor.x), Some(400.0));
}

#[test]
fn a_shaking_press_clicks_and_a_turning_press_drags() {
    let mut frames = Frames::default();
    frames.right(0.0, 0.0, 0.0);
    frames.right(0.0, 0.0, 1.0);
    for frame in 0..60 {
        let yaw = if frame % 2 == 0 { 3.0 } else { -3.0 };
        let (events, cursor) = frames.right(0.0, yaw, 1.0);
        assert!(events.is_empty());
        assert_eq!(cursor.map(|cursor| cursor.x), Some(400.0));
    }
    // The release lets go at the held point before the pointer follows the ray again
    let (events, _) = frames.right(0.0, 0.0, 0.0);
    assert_eq!(events.first(), Some(&PointerEvent::Release));

    frames.right(0.0, 0.0, 1.0);
    let moved = (0..60).any(|_| {
        let (events, _) = frames.right(0.0, 15.0, 1.0);
        matches!(events.first(), Some(PointerEvent::Move { x, .. }) if *x > 400.0)
    });
    assert!(moved);
}

#[test]
fn the_nearest_panel_takes_the_pointer() {
    let mut frames = Frames::default();
    let (events, cursor) = frames.update([None, aim(0.0, 0.0, 0.0)], &[BEHIND, PANEL]);
    assert_eq!(events, [PointerEvent::Move { x: 400.0, y: 250.0 }]);
    assert_eq!(cursor.map(|cursor| cursor.panel), Some(1));
}

#[test]
fn a_trigger_held_from_outside_does_not_press_on_entry() {
    let mut frames = Frames::default();
    let (events, cursor) = frames.right(5.0, 0.0, 1.0);
    assert!(events.is_empty());
    assert_eq!(cursor, None);
    let (events, cursor) = frames.right(0.0, 0.0, 1.0);
    assert_eq!(events, [PointerEvent::Move { x: 400.0, y: 250.0 }]);
    assert!(cursor.is_some_and(|cursor| !cursor.pressed));
    assert!(frames.right(0.0, 0.0, 0.0).0.is_empty());
    assert_eq!(frames.right(0.0, 0.0, 1.0).0, [PointerEvent::Press]);
}

#[test]
fn the_pointer_keeps_its_hand_while_pressed() {
    let mut frames = Frames::default();
    frames.update([None, aim(0.0, 0.0, 1.0)], &[PANEL]);
    let both = |left: f32, right: f32| [aim(0.1, 0.0, left), aim(0.0, 0.0, right)];
    let (events, cursor) = frames.update(both(1.0, 1.0), &[PANEL]);
    assert!(events.is_empty());
    assert_eq!(cursor.map(|cursor| cursor.x), Some(400.0));
    assert_eq!(
        frames.update(both(1.0, 0.0), &[PANEL]).0,
        [PointerEvent::Release]
    );
    assert!(frames.update(both(0.0, 0.0), &[PANEL]).0.is_empty());
    assert_eq!(
        frames.update(both(1.0, 0.0), &[PANEL]).0,
        [
            PointerEvent::Move { x: 500.0, y: 250.0 },
            PointerEvent::Press
        ]
    );
}

#[test]
fn a_lost_controller_releases_its_press() {
    let mut frames = Frames::default();
    frames.right(0.0, 0.0, 0.0);
    frames.right(0.0, 0.0, 1.0);
    let (events, _) = frames.update([None, None], &[PANEL]);
    assert_eq!(events, [PointerEvent::Release, PointerEvent::Leave]);
}

#[test]
fn a_ray_shows_up_to_the_panel_it_meets() {
    let mut frames = Frames::default();
    frames.right(0.0, 0.0, 0.0);
    let [left, right] = frames.pointer.beams();
    assert_eq!(left, None);
    assert!(right.is_some_and(|beam| (beam.length - 1.0).abs() < 1e-4));
}

#[test]
fn a_pull_on_a_handle_carries_the_panel() {
    let movable = PanelSlot {
        mobility: Mobility::Movable,
        shape: Shape::Flat,
        ..PANEL
    };
    let at_handle = |x: f32, trigger: f32| handle_aim(&movable, x, trigger);
    let mut frames = Frames::default();
    let (events, _) = frames.update([None, at_handle(0.0, 0.0)], &[movable]);
    assert!(events.is_empty());
    assert_eq!(frames.pointer.handle(), Some(1));
    frames.update([None, at_handle(0.0, 1.0)], &[movable]);
    assert_eq!(
        frames.pointer.moves(),
        [PanelMove {
            panel: 1,
            event_type: SpatialMoveEventType::Start,
            pose: movable.pose,
        }]
    );
    let (events, cursor) = frames.update([None, at_handle(0.1, 1.0)], &[movable]);
    assert!(events.is_empty());
    assert_eq!(cursor, None);
    let [moving] = *frames.pointer.moves() else {
        panic!("the panel moves once: {:?}", frames.pointer.moves());
    };
    assert_eq!(moving.panel, 1);
    assert_eq!(moving.event_type, SpatialMoveEventType::Moving);
    assert!((moving.pose.position[0] - 0.1).abs() < 1e-4);
    frames.update([None, at_handle(0.1, 1.0)], &[movable]);
    assert!(
        frames.pointer.moves().is_empty(),
        "a still panel reports nothing"
    );
    frames.update([None, at_handle(0.1, 0.0)], &[movable]);
    assert_eq!(
        frames.pointer.moves(),
        [PanelMove {
            event_type: SpatialMoveEventType::End,
            ..moving
        }]
    );
}

#[test]
fn a_ray_moves_the_pointer_along_the_curve_of_a_curved_panel() {
    // A meter-wide circle around the controller: 20 degrees are 349 mm of arc, where a flat
    // panel would put the pointer 364 mm from the middle
    let curved = PanelSlot {
        shape: Shape::Curved { radius: 1.0 },
        ..PANEL
    };
    let mut frames = Frames::default();
    let (events, _) = frames.update([None, aim(0.0, 20.0, 0.0)], &[curved]);
    let Some(PointerEvent::Move { x, y }) = events.first() else {
        panic!("the ray meets the curved panel: {events:?}");
    };
    assert!((x - (400.0 + 349.07)).abs() < 0.1, "x {x}");
    assert!((y - 250.0).abs() < 0.1, "y {y}");
}

#[test]
fn a_surface_stops_the_ray_and_takes_no_pointer_events() {
    // A video surface of 400 by 300 dp half a meter ahead, in front of the panel
    let video = PanelSlot {
        id: 3,
        kind: PanelKind::Surface(StereoMode::Mono),
        atlas: [0.0, 0.0, 400.0, 300.0],
        pose: Pose::at([0.0, 0.0, -0.5]),
        mobility: Mobility::Fixed,
        shape: Shape::Flat,
    };
    let mut frames = Frames::default();
    let panels = [PANEL, video];
    let (events, cursor) = frames.update([None, aim(0.0, 0.0, 0.0)], &panels);
    assert!(events.is_empty());
    assert_eq!(cursor, None);
    let [_, beam] = frames.pointer.beams();
    assert!(beam.is_some_and(|beam| (beam.length - 0.5).abs() < 1e-4));
    let (events, cursor) = frames.update([None, aim(0.0, 0.0, 1.0)], &panels);
    assert!(events.is_empty());
    assert_eq!(cursor, None);
}

// A headset with one panel of 400 by 300 dp that the content places with `modifier`. The panel
// leaves with the composition, so the test holds the robot that runs it.
fn headset(modifier: SubspaceModifier) -> (RobotTestRule<TestRenderer>, Space) {
    let held = Rc::new(RefCell::new(None));
    let keep = Rc::clone(&held);
    let mut robot = create_headless_robot_test(500, 400, move || {
        let space = remember(Space::new).with(Clone::clone);
        keep.replace(Some(space.clone()));
        let modifier = modifier.clone();
        CompositionLocalProvider([local_space().provides(Some(space))], move || {
            SpatialPanel(modifier, || {});
        });
    });
    robot.wait_for_idle();
    let space = held.take().expect("the content composed");
    (robot, space)
}

// The user grabs the handle, carries the panel 10 cm to the right in two frames and lets go, as
// the frame loop of the headset runs it.
fn carry_right(space: &Space) {
    let mut pointer = Pointer::default();
    for (frame, (x, trigger)) in [(0.0, 0.0), (0.0, 1.0), (0.05, 1.0), (0.1, 1.0), (0.1, 0.0)]
        .into_iter()
        .enumerate()
    {
        let aim = handle_aim(&space.panels()[0], x, trigger);
        let time = Duration::from_millis(10 * frame as u64);
        pointer.update([None, aim], time, &space.panels());
        for step in pointer.moves() {
            space.carry(step);
        }
    }
}

fn close(a: &Pose, b: &Pose) -> bool {
    let near = |x: &[f32], y: &[f32]| x.iter().zip(y).all(|(x, y)| (x - y).abs() < 1e-4);
    near(&a.position, &b.position) && near(&a.orientation, &b.orientation)
}

#[test]
fn a_carried_panel_tells_the_app_its_pose_and_takes_it_back_at_the_next_start() {
    let events = Rc::new(RefCell::new(Vec::<SpatialMoveEvent>::new()));
    let heard = Rc::clone(&events);
    let policy = MovePolicy::system(move |event| heard.borrow_mut().push(event));
    let (_robot, space) = headset(
        SubspaceModifier::new()
            .size(400.0, 300.0)
            .movable_with(policy),
    );
    carry_right(&space);

    let events = events.borrow();
    let types: Vec<_> = events.iter().map(|event| event.event_type).collect();
    assert_eq!(
        types,
        [
            SpatialMoveEventType::Start,
            SpatialMoveEventType::Moving,
            SpatialMoveEventType::Moving,
            SpatialMoveEventType::End,
        ]
    );
    // The poses are relative to the subspace's origin, where the panel started
    assert!(close(&events[0].pose, &Pose::IDENTITY));
    assert_eq!(events[0].previous_pose, events[0].pose);
    assert!((events[2].pose.position[0] - 0.1).abs() < 1e-4);
    assert_eq!(events[2].previous_pose, events[1].pose);
    assert_eq!(events[3].pose, events[2].pose);
    let carried = space.panels()[0].pose;
    assert!(close(&carried, &SUBSPACE_ORIGIN.then(&events[3].pose)));

    // The next run of the app places the panel where the user left it, from the saved pose
    let saved = events[3].pose;
    let (_second, restored) = headset(
        SubspaceModifier::new()
            .size(400.0, 300.0)
            .with_pose(saved)
            .movable(),
    );
    assert!(close(&restored.panels()[0].pose, &carried));
}

#[test]
fn a_custom_policy_leaves_the_pose_to_the_app() {
    let events = Rc::new(RefCell::new(Vec::<SpatialMoveEvent>::new()));
    let heard = Rc::clone(&events);
    let policy = MovePolicy::custom(move |event| heard.borrow_mut().push(event));
    let (_robot, space) = headset(
        SubspaceModifier::new()
            .size(400.0, 300.0)
            .movable_with(policy),
    );
    let start = space.panels()[0].pose;
    carry_right(&space);
    assert_eq!(events.borrow().len(), 4);
    assert_eq!(
        space.panels()[0].pose,
        start,
        "the modifier still places the panel"
    );
}
