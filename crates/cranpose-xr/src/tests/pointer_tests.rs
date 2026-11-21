use std::{
    cell::{Cell, RefCell},
    rc::Rc,
    time::Duration,
};

use cranpose::{
    PointerButton, PointerEventKind,
    prelude::{Box as Layer, BoxSpec, Button, ButtonSpec, Modifier},
};
use cranpose_core::{CompositionLocalProvider, remember};
use cranpose_testing::{
    create_headless_robot_test,
    robot::{RobotTestRule, TestRenderer},
};

use super::*;
use crate::{
    Controller, HandAim, HandState, InputSource, MovePolicy, Pinch, PointerMapping, Pose, Ray,
    ResizeLimits, ResizePolicy, SpatialExternalSurface, SpatialMoveEvent, SpatialMoveEventType,
    SpatialPanel, SpatialResizeEvent, SpatialResizeEventType, SpatialVolume, StereoMode,
    SubspaceModifier, TrackedPose, TrackingState, XrFrame,
    aim::{Aim, Buttons},
    atlas_input,
    handle::HandleKind,
    pointer_aim::pointer_aim,
    scripted_hand::{PinchCycle, ScriptedHand, play},
    space::{
        Cursor, Mobility, PanelKind, PanelMove, PanelResize, PanelSlot, Ride, SUBSPACE_ORIGIN,
        Shape, Space, local_space,
    },
    surface_input::{Gesture, HandGesture},
    volume_host::VolumeScene,
    volume_pick::PickMeshes,
};

// The press and the release of the primary button, which a trigger or a pinch makes.
const PRESS: Gesture = Gesture::Press(PointerButton::Primary);
const RELEASE: Gesture = Gesture::Release(PointerButton::Primary);

// A panel of 800 by 500 dp a meter ahead, at the atlas's top.
pub(super) const PANEL: PanelSlot = PanelSlot {
    id: 1,
    kind: PanelKind::Content,
    atlas: [0.0, 0.0, 800.0, 500.0],
    pose: Pose::at([0.0, 0.0, -1.0]),
    mobility: Mobility::Fixed,
    shape: Shape::Flat,
    resize: None,
    room: [800.0, 500.0],
    priority: 0,
    ride: Ride::Free,
};

// A panel of 200 by 100 dp behind the first one and below it in the atlas.
const BEHIND: PanelSlot = PanelSlot {
    id: 2,
    kind: PanelKind::Content,
    atlas: [0.0, 508.0, 200.0, 100.0],
    pose: Pose::at([0.0, 0.0, -2.0]),
    mobility: Mobility::Fixed,
    shape: Shape::Flat,
    resize: None,
    room: [200.0, 100.0],
    priority: 0,
    ride: Ride::Free,
};

// A controller at `x` meters that points straight ahead, turned right by `yaw` degrees.
pub(super) fn aim(x: f32, yaw: f32, trigger: f32) -> Option<Aim> {
    let (sin, cos) = yaw.to_radians().sin_cos();
    let ray = Ray {
        origin: [x, 0.0, 0.0],
        direction: [sin, 0.0, -cos],
    };
    Some(Aim::pulling(ray, Pose::at([x, 0.0, 0.0]), trigger))
}

// A controller at `origin` that points at `target`, with its trigger pulled by `trigger`.
pub(super) fn aim_at(origin: [f32; 3], target: [f32; 3], trigger: f32) -> Option<Aim> {
    let to_target = [
        target[0] - origin[0],
        target[1] - origin[1],
        target[2] - origin[2],
    ];
    let length = to_target.iter().map(|v| v * v).sum::<f32>().sqrt();
    let ray = Ray {
        origin,
        direction: to_target.map(|v| v / length),
    };
    Some(Aim::pulling(ray, Pose::at(origin), trigger))
}

// A controller at `x` that points at the handle under `slot`'s panel, with its trigger pulled by
// `trigger`.
pub(super) fn handle_aim(slot: &PanelSlot, x: f32, trigger: f32) -> Option<Aim> {
    aim_at([x, 0.0, 0.0], crate::carry::handle_center(slot), trigger)
}

// A hand that the cameras track at `x` meters, pointing straight ahead turned right by `yaw`
// degrees, whose index finger pinches by `strength`.
pub(super) fn tracked_hand(x: f32, yaw: f32, strength: f32) -> HandState {
    let half = (-yaw).to_radians() / 2.0;
    HandState {
        tracking_state: TrackingState::Tracking,
        aim: Some(HandAim {
            pose: Pose {
                position: [x, 0.0, 0.0],
                orientation: [0.0, half.sin(), 0.0, half.cos()],
            },
            index: Pinch {
                strength,
                pinching: strength > 0.8,
            },
            ..HandAim::default()
        }),
        ..HandState::default()
    }
}

// The pointer's aim for `hand` when no controller is in use, in an app that asked for hand
// tracking.
pub(super) fn hand_aim_of(hand: &HandState) -> Option<Aim> {
    pointer_aim(&Controller::default(), Some(hand))
}

// A controller in use at `x` meters that points straight ahead, in the frame that the controller
// helpers `aim` build for.
fn controller_in_use(x: f32, trigger: f32) -> Controller {
    Controller {
        aim: Some(TrackedPose {
            pose: Pose::at([x, 0.0, 0.0]),
            linear_velocity: None,
            angular_velocity: None,
        }),
        trigger: crate::Trigger {
            value: trigger,
            touched: true,
            ..crate::Trigger::default()
        },
        ..Controller::default()
    }
}

// Frames 10 ms apart.
#[derive(Default)]
pub(super) struct Frames {
    pub(super) pointer: Pointers,
    frame: u64,
    picks: PickMeshes,
    pub(super) mapping: PointerMapping,
}

impl Frames {
    // One frame with `aims`, left then right. The gestures for the composition of both hands come
    // in the order that they happened, and the cursor is the right hand's, else the left's.
    pub(super) fn update(
        &mut self,
        aims: [Option<Aim>; 2],
        panels: &[PanelSlot],
    ) -> (Vec<Gesture>, Option<Cursor>) {
        let (events, cursors) = self.step(aims, panels);
        let gestures = events.into_iter().map(|event| event.gesture).collect();
        (gestures, cursors[1].or(cursors[0]))
    }

    // The same, with the gestures tagged with their hands and the cursor of each hand.
    pub(super) fn step(
        &mut self,
        aims: [Option<Aim>; 2],
        panels: &[PanelSlot],
    ) -> (Vec<HandGesture>, [Option<Cursor>; 2]) {
        self.frame += 1;
        let time = Duration::from_millis(10 * self.frame);
        self.pointer
            .update(aims, time, (panels, &self.picks), (self.mapping, true));
        (self.pointer.atlas().to_vec(), self.pointer.cursors())
    }

    fn right(&mut self, x: f32, yaw: f32, trigger: f32) -> (Vec<Gesture>, Option<Cursor>) {
        self.update([None, aim(x, yaw, trigger)], &[PANEL])
    }

    // The right hand points as a tracked hand with no controller, pinching by `strength`.
    fn right_hand(&mut self, x: f32, yaw: f32, strength: f32) -> (Vec<Gesture>, Option<Cursor>) {
        self.right_state(&tracked_hand(x, yaw, strength))
    }

    fn right_state(&mut self, hand: &HandState) -> (Vec<Gesture>, Option<Cursor>) {
        self.update([None, hand_aim_of(hand)], &[PANEL])
    }
}

#[test]
fn a_trigger_over_a_panel_moves_presses_and_releases() {
    let mut frames = Frames::default();
    let (events, cursor) = frames.right(0.0, 0.0, 0.0);
    assert_eq!(events, [Gesture::Move { x: 400.0, y: 250.0 }]);
    assert_eq!(
        cursor.map(|cursor| (cursor.panel, cursor.x, cursor.y)),
        Some((1, 400.0, 250.0))
    );
    let (events, cursor) = frames.right(0.0, 0.0, 1.0);
    assert_eq!(events, [PRESS]);
    assert!(cursor.is_some_and(|cursor| cursor.pressed));
    let (events, _) = frames.right(0.0, 0.0, 0.0);
    assert_eq!(events, [RELEASE]);
}

#[test]
fn a_half_pulled_trigger_neither_presses_nor_releases() {
    let mut frames = Frames::default();
    frames.right(0.0, 0.0, 0.0);
    assert!(frames.right(0.0, 0.0, 0.5).0.is_empty());
    assert_eq!(frames.right(0.0, 0.0, 0.7).0, [PRESS]);
    assert!(frames.right(0.0, 0.0, 0.5).0.is_empty());
    assert_eq!(frames.right(0.0, 0.0, 0.3).0, [RELEASE]);
}

#[test]
fn a_press_takes_the_point_from_before_the_pull() {
    let mut frames = Frames::default();
    frames.right(0.0, 0.0, 0.0);
    frames.right(0.05, 0.0, 0.3);
    let (events, cursor) = frames.right(0.1, 0.0, 0.8);
    assert_eq!(events, [Gesture::Move { x: 400.0, y: 250.0 }, PRESS]);
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
    assert_eq!(events.first(), Some(&RELEASE));

    frames.right(0.0, 0.0, 1.0);
    let moved = (0..60).any(|_| {
        let (events, _) = frames.right(0.0, 15.0, 1.0);
        matches!(events.first(), Some(Gesture::Move { x, .. }) if *x > 400.0)
    });
    assert!(moved);
}

#[test]
fn a_press_whose_pull_turned_the_controller_holds_its_point_while_held_long() {
    let mut frames = Frames::default();
    frames.right(0.0, 0.0, 0.0);
    // The pull turns the controller by 6 degrees before the trigger goes down
    for _ in 0..10 {
        frames.right(0.0, 6.0, 0.3);
    }
    let (events, _) = frames.right(0.0, 6.0, 1.0);
    assert_eq!(events, [Gesture::Move { x: 400.0, y: 250.0 }, PRESS]);
    for _ in 0..60 {
        let (events, cursor) = frames.right(0.0, 6.0, 1.0);
        assert!(events.is_empty());
        assert_eq!(cursor.map(|cursor| cursor.x), Some(400.0));
    }
    assert_eq!(frames.right(0.0, 6.0, 0.0).0.first(), Some(&RELEASE));
}

#[test]
fn letting_go_of_a_long_press_does_not_drag() {
    let mut frames = Frames::default();
    frames.right(0.0, 0.0, 0.0);
    frames.right(0.0, 0.0, 1.0);
    for _ in 0..50 {
        frames.right(0.0, 0.0, 1.0);
    }
    // Letting go turns the controller by 8 degrees while the trigger is still down
    for _ in 0..10 {
        let (events, cursor) = frames.right(0.0, 8.0, 0.7);
        assert!(events.is_empty());
        assert_eq!(cursor.map(|cursor| cursor.x), Some(400.0));
    }
    assert_eq!(frames.right(0.0, 8.0, 0.3).0.first(), Some(&RELEASE));
}

#[test]
fn a_pull_that_aims_anew_presses_where_the_ray_points_now() {
    let mut frames = Frames::default();
    frames.right(0.0, 0.0, 0.0);
    for _ in 0..10 {
        frames.right(0.0, 15.0, 0.3);
    }
    let (events, cursor) = frames.right(0.0, 15.0, 1.0);
    assert_eq!(events.last(), Some(&PRESS));
    assert!(cursor.is_some_and(|cursor| cursor.x > 600.0));
}

#[test]
fn the_nearest_panel_takes_the_pointer() {
    let mut frames = Frames::default();
    let (events, cursor) = frames.update([None, aim(0.0, 0.0, 0.0)], &[BEHIND, PANEL]);
    assert_eq!(events, [Gesture::Move { x: 400.0, y: 250.0 }]);
    assert_eq!(cursor.map(|cursor| cursor.panel), Some(1));
}

#[test]
fn a_trigger_held_from_outside_does_not_press_on_entry() {
    let mut frames = Frames::default();
    let (events, cursor) = frames.right(5.0, 0.0, 1.0);
    assert!(events.is_empty());
    assert_eq!(cursor, None);
    let (events, cursor) = frames.right(0.0, 0.0, 1.0);
    assert_eq!(events, [Gesture::Move { x: 400.0, y: 250.0 }]);
    assert!(cursor.is_some_and(|cursor| !cursor.pressed));
    assert!(frames.right(0.0, 0.0, 0.0).0.is_empty());
    assert_eq!(frames.right(0.0, 0.0, 1.0).0, [PRESS]);
}

#[test]
fn a_press_of_one_hand_leaves_the_other_hand_free_to_press_and_release() {
    let mut frames = Frames::default();
    frames.update([None, aim(0.0, 0.0, 1.0)], &[PANEL]);
    // The left hand points 10 cm to the right of the right hand's point and pulls its trigger
    let both = |left: f32, right: f32| [aim(0.1, 0.0, left), aim(0.0, 0.0, right)];
    let (events, _) = frames.step(both(1.0, 1.0), &[PANEL]);
    assert_eq!(
        events,
        [
            HandGesture {
                hand: Side::Left,
                gesture: Gesture::Move { x: 500.0, y: 250.0 }
            },
            HandGesture {
                hand: Side::Left,
                gesture: PRESS
            }
        ],
        "the left hand's pull presses at once, with the right hand's press still down"
    );
    let (events, _) = frames.step(both(1.0, 0.0), &[PANEL]);
    assert_eq!(
        events,
        [HandGesture {
            hand: Side::Right,
            gesture: RELEASE
        }]
    );
    let (events, _) = frames.step(both(0.0, 0.0), &[PANEL]);
    assert_eq!(
        events,
        [HandGesture {
            hand: Side::Left,
            gesture: RELEASE
        }]
    );
}

#[test]
fn a_lost_controller_releases_its_press() {
    let mut frames = Frames::default();
    frames.right(0.0, 0.0, 0.0);
    frames.right(0.0, 0.0, 1.0);
    let (events, _) = frames.update([None, None], &[PANEL]);
    assert_eq!(events, [RELEASE, Gesture::Leave]);
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
fn a_ray_reaches_a_far_panel_and_shows_its_full_length_where_it_meets_nothing() {
    let far = PanelSlot {
        pose: Pose::at([0.0, 0.0, -6.0]),
        ..PANEL
    };
    let mut frames = Frames::default();
    frames.update([None, aim(0.0, 0.0, 0.0)], &[far]);
    let beam = frames.pointer.beams()[1];
    assert!(beam.is_some_and(|beam| (beam.length - 6.0).abs() < 1e-3));
    // A ray that misses the panel still shows, as long as a panel stands in its reach
    frames.update([None, aim(0.0, 60.0, 0.0)], &[PANEL]);
    let beam = frames.pointer.beams()[1];
    assert!(beam.is_some_and(|beam| (beam.length - 3.0).abs() < 1e-4));
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
    assert_eq!(frames.pointer.handles()[1], Some((1, HandleKind::Move)));
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

// A movable `PANEL`: its bottom edge is at y = -0.25.
pub(super) const MOVABLE: PanelSlot = PanelSlot {
    mobility: Mobility::Movable,
    ..PANEL
};

#[test]
fn a_pull_on_a_handle_from_behind_the_panel_carries_it() {
    let center = crate::carry::handle_center(&MOVABLE);
    // The controller stands two meters behind the panel and points back at the handle.
    let behind = |trigger: f32| aim_at([0.1, 0.3, -3.0], center, trigger);
    let mut frames = Frames::default();
    frames.update([None, behind(0.0)], &[MOVABLE]);
    assert_eq!(frames.pointer.handles()[1], Some((1, HandleKind::Move)));
    frames.update([None, behind(1.0)], &[MOVABLE]);
    assert_eq!(
        frames.pointer.moves(),
        [PanelMove {
            panel: 1,
            event_type: SpatialMoveEventType::Start,
            pose: MOVABLE.pose,
        }]
    );
}

#[test]
fn a_pull_beside_a_handle_does_not_carry() {
    let [x, y, z] = crate::carry::handle_center(&MOVABLE);
    // 10 cm to the side of the handle, in front of the panel and behind it
    for origin in [[x + 0.1, y, 0.0], [x + 0.1, y, -3.0]] {
        let beside = |trigger: f32| aim_at(origin, [x + 0.1, y, z], trigger);
        let mut frames = Frames::default();
        frames.update([None, beside(0.0)], &[MOVABLE]);
        assert_eq!(frames.pointer.handles()[1], None, "{origin:?}");
        frames.update([None, beside(1.0)], &[MOVABLE]);
        assert!(frames.pointer.moves().is_empty(), "{origin:?}");
    }
}

#[test]
fn a_pull_just_above_the_bottom_edge_of_a_movable_panel_presses() {
    let mut frames = Frames::default();
    let aim = |trigger: f32| aim_at([0.0, -0.245, 0.0], [0.0, -0.245, -1.0], trigger);
    frames.update([None, aim(0.0)], &[MOVABLE]);
    assert_eq!(frames.pointer.handles()[1], None);
    let (events, _) = frames.update([None, aim(1.0)], &[MOVABLE]);
    assert!(events.contains(&PRESS));
    assert!(frames.pointer.moves().is_empty());
}

// A resizable `PANEL` with no limits: its corner handle stands 0.44 meters to the right of its
// center and 0.29 meters below it.
const RESIZABLE: PanelSlot = PanelSlot {
    resize: Some(ResizeLimits::new()),
    ..PANEL
};

// A controller at the origin that points `shift` meters to the right and up of the middle of the
// corner handle of `slot`'s panel, with its trigger pulled by `trigger`.
fn corner_aim(slot: &PanelSlot, shift: [f32; 2], trigger: f32) -> Option<Aim> {
    let [x, y, z] = crate::resize::corner_center(slot);
    aim_at([0.0; 3], [x + shift[0], y + shift[1], z], trigger)
}

// Holds `aim` for three seconds, long enough for the filter of the ray to settle on it. Returns
// the resize steps of the frames.
fn hold(frames: &mut Frames, panel: PanelSlot, aim: Option<Aim>) -> Vec<PanelResize> {
    let mut steps = Vec::new();
    for _ in 0..300 {
        frames.update([None, aim], &[panel]);
        steps.extend_from_slice(frames.pointer.resizes());
    }
    steps
}

fn resizing(size: [f32; 2]) -> PanelResize {
    PanelResize {
        panel: 1,
        event_type: SpatialResizeEventType::Resizing,
        size,
    }
}

#[test]
fn a_pull_on_the_corner_handle_resizes_the_panel_and_reports_each_size() {
    let panel = RESIZABLE;
    let mut frames = Frames::default();
    let (events, _) = frames.update([None, corner_aim(&panel, [0.0; 2], 0.0)], &[panel]);
    assert!(events.is_empty());
    assert_eq!(frames.pointer.handles()[1], Some((1, HandleKind::Resize)));

    let (events, _) = frames.update([None, corner_aim(&panel, [0.0; 2], 1.0)], &[panel]);
    assert!(events.is_empty(), "the handle takes the pull");
    assert_eq!(
        frames.pointer.resizes(),
        [PanelResize {
            panel: 1,
            event_type: SpatialResizeEventType::Start,
            size: [800.0, 500.0],
        }]
    );
    assert!(frames.pointer.moves().is_empty());

    // The corner moves 10 cm to the right and 5 cm down: each side moves, and the center stays
    let steps = hold(&mut frames, panel, corner_aim(&panel, [0.1, -0.05], 1.0));
    assert!(steps.len() > 1, "the sizes follow the ray");
    assert_eq!(steps.last(), Some(&resizing([1000.0, 600.0])));
    assert!(
        steps.windows(2).all(|pair| {
            let [before, after] = [pair[0].size, pair[1].size];
            after[0] >= before[0]
                && after[1] >= before[1]
                && after[0] + after[1] > before[0] + before[1]
        }),
        "each step is a new size that grows: {steps:?}"
    );
    assert!(
        steps
            .iter()
            .all(|step| step.event_type == SpatialResizeEventType::Resizing)
    );
    assert!(frames.pointer.moves().is_empty(), "the center stays");

    frames.update([None, corner_aim(&panel, [0.1, -0.05], 0.0)], &[panel]);
    assert_eq!(
        frames.pointer.resizes(),
        [PanelResize {
            event_type: SpatialResizeEventType::End,
            ..resizing([1000.0, 600.0])
        }]
    );
}

#[test]
fn a_resize_stays_within_the_limits() {
    let limits = ResizeLimits::new()
        .minimum_size(400.0, 250.0)
        .maximum_size(1000.0, 600.0);
    let panel = PanelSlot {
        resize: Some(limits),
        ..PANEL
    };
    let mut frames = Frames::default();
    frames.update([None, corner_aim(&panel, [0.0; 2], 0.0)], &[panel]);
    frames.update([None, corner_aim(&panel, [0.0; 2], 1.0)], &[panel]);
    let larger = hold(&mut frames, panel, corner_aim(&panel, [0.5, -0.4], 1.0));
    assert_eq!(larger.last(), Some(&resizing([1000.0, 600.0])));
    let smaller = hold(&mut frames, panel, corner_aim(&panel, [-0.9, 0.9], 1.0));
    assert_eq!(smaller.last(), Some(&resizing([400.0, 250.0])));
    assert!(
        larger.iter().chain(&smaller).all(|step| {
            (400.0..=1000.0).contains(&step.size[0]) && (250.0..=600.0).contains(&step.size[1])
        }),
        "no step leaves the limits"
    );
}

#[test]
fn a_resize_that_maintains_the_aspect_ratio_keeps_the_ratio_of_the_start() {
    let limits = ResizeLimits::new()
        .maximum_size(1200.0, 700.0)
        .maintain_aspect_ratio(true);
    let panel = PanelSlot {
        resize: Some(limits),
        ..PANEL
    };
    let mut frames = Frames::default();
    frames.update([None, corner_aim(&panel, [0.0; 2], 0.0)], &[panel]);
    frames.update([None, corner_aim(&panel, [0.0; 2], 1.0)], &[panel]);
    // A drag to the right alone still grows the height
    let steps = hold(&mut frames, panel, corner_aim(&panel, [0.1, 0.0], 1.0));
    assert_eq!(steps.last(), Some(&resizing([944.0, 590.0])));
    // The corner goes far past the maximum, which the height reaches first
    let steps = hold(&mut frames, panel, corner_aim(&panel, [0.8, -0.8], 1.0));
    assert_eq!(steps.last(), Some(&resizing([1120.0, 700.0])));
    assert!(
        steps
            .iter()
            .all(|step| (step.size[0] / step.size[1] - 1.6).abs() < 0.01),
        "every step keeps the ratio of 800 to 500: {steps:?}"
    );
}

#[test]
fn a_pull_on_the_panel_next_to_the_corner_handle_still_presses() {
    let panel = RESIZABLE;
    // The panel's own bottom right corner is at (0.4, -0.25): the handle stands beyond it
    for target in [[0.399, -0.249], [0.3, -0.2]] {
        let aim = |trigger: f32| aim_at([0.0; 3], [target[0], target[1], -1.0], trigger);
        let mut frames = Frames::default();
        frames.update([None, aim(0.0)], &[panel]);
        assert_eq!(frames.pointer.handles()[1], None, "{target:?}");
        let (events, _) = frames.update([None, aim(1.0)], &[panel]);
        assert!(events.contains(&PRESS), "{target:?}: {events:?}");
        assert!(frames.pointer.resizes().is_empty(), "{target:?}");
    }
}

#[test]
fn a_pull_beside_the_corner_handle_neither_resizes_nor_presses() {
    let panel = RESIZABLE;
    // 10 cm to the right of the handle, off the panel
    let aim = |trigger: f32| corner_aim(&panel, [0.1, 0.0], trigger);
    let mut frames = Frames::default();
    frames.update([None, aim(0.0)], &[panel]);
    assert_eq!(frames.pointer.handles()[1], None);
    let (events, _) = frames.update([None, aim(1.0)], &[panel]);
    assert!(events.is_empty());
    assert!(frames.pointer.resizes().is_empty());
}

#[test]
fn a_trigger_on_its_way_up_leaves_the_size_as_it_was() {
    let panel = RESIZABLE;
    let mut frames = Frames::default();
    frames.update([None, corner_aim(&panel, [0.0; 2], 0.0)], &[panel]);
    frames.update([None, corner_aim(&panel, [0.0; 2], 1.0)], &[panel]);
    hold(&mut frames, panel, corner_aim(&panel, [0.05, -0.05], 1.0));
    // Letting go turns the controller, so a ray that swings away as the trigger falls resizes
    // nothing. A trigger that falls only a little is still held.
    let falling = hold(&mut frames, panel, corner_aim(&panel, [0.3, -0.3], 0.8));
    assert_eq!(falling, [], "the ray swings while the trigger falls");
    frames.update([None, corner_aim(&panel, [0.3, -0.3], 0.0)], &[panel]);
    assert_eq!(
        frames.pointer.resizes(),
        [PanelResize {
            event_type: SpatialResizeEventType::End,
            ..resizing([900.0, 600.0])
        }]
    );
}

#[test]
fn a_lost_controller_ends_the_resize_at_its_last_size() {
    let panel = RESIZABLE;
    let mut frames = Frames::default();
    frames.update([None, corner_aim(&panel, [0.0; 2], 0.0)], &[panel]);
    frames.update([None, corner_aim(&panel, [0.0; 2], 1.0)], &[panel]);
    hold(&mut frames, panel, corner_aim(&panel, [0.05, -0.05], 1.0));
    frames.update([None, None], &[panel]);
    assert_eq!(
        frames.pointer.resizes(),
        [PanelResize {
            event_type: SpatialResizeEventType::End,
            ..resizing([900.0, 600.0])
        }]
    );
}

#[test]
fn a_panel_that_moves_and_resizes_has_a_handle_for_each() {
    let both = PanelSlot {
        mobility: Mobility::Movable,
        ..RESIZABLE
    };
    let mut frames = Frames::default();
    frames.update([None, handle_aim(&both, 0.0, 0.0)], &[both]);
    assert_eq!(frames.pointer.handles()[1], Some((1, HandleKind::Move)));
    frames.update([None, handle_aim(&both, 0.0, 1.0)], &[both]);
    assert_eq!(frames.pointer.moves().len(), 1);
    assert!(frames.pointer.resizes().is_empty());

    // The ray's filter lags a jump from one handle to the other, so the other grab starts anew
    let mut frames = Frames::default();
    frames.update([None, corner_aim(&both, [0.0; 2], 0.0)], &[both]);
    assert_eq!(frames.pointer.handles()[1], Some((1, HandleKind::Resize)));
    frames.update([None, corner_aim(&both, [0.0; 2], 1.0)], &[both]);
    assert_eq!(frames.pointer.resizes().len(), 1);
    assert!(frames.pointer.moves().is_empty());
}

#[test]
fn a_panel_on_a_curve_has_no_corner_handle() {
    let curved = PanelSlot {
        shape: Shape::Curved { radius: 1.0 },
        ..RESIZABLE
    };
    let mut frames = Frames::default();
    frames.update([None, corner_aim(&RESIZABLE, [0.0; 2], 0.0)], &[curved]);
    assert_eq!(frames.pointer.handles()[1], None);
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
    let Some(Gesture::Move { x, y }) = events.first() else {
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
        resize: None,
        room: [400.0, 300.0],
        priority: 0,
        ride: Ride::Free,
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

// The phases of the carry that `carry_right` makes: the grab, two moves and the release.
const CARRY_PHASES: [SpatialMoveEventType; 4] = [
    SpatialMoveEventType::Start,
    SpatialMoveEventType::Moving,
    SpatialMoveEventType::Moving,
    SpatialMoveEventType::End,
];

fn phases(events: &[SpatialMoveEvent]) -> Vec<SpatialMoveEventType> {
    events.iter().map(|event| event.event_type).collect()
}

// The user grabs the handle, carries the panel 10 cm to the right in two frames and lets go, as
// the frame loop of the headset runs it.
fn carry_right(space: &Space) {
    let mut pointer = Pointers::default();
    for (frame, (x, trigger)) in [(0.0, 0.0), (0.0, 1.0), (0.05, 1.0), (0.1, 1.0), (0.1, 0.0)]
        .into_iter()
        .enumerate()
    {
        let aim = handle_aim(&space.panels()[0], x, trigger);
        let time = Duration::from_millis(10 * frame as u64);
        pointer.update(
            [None, aim],
            time,
            (&space.panels(), &PickMeshes::default()),
            (PointerMapping::default(), true),
        );
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
    assert_eq!(phases(&events), CARRY_PHASES);
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

// The user grabs the corner handle, drags it 10 cm to the right and 5 cm down and lets go, as the
// frame loop of the headset runs it. The ray's filter settles during the 3 s of the drag.
fn resize_right(space: &Space) {
    let mut pointer = Pointers::default();
    let frames = [([0.0, 0.0], 0.0), ([0.0, 0.0], 1.0)]
        .into_iter()
        .chain((0..300).map(|_| ([0.1, -0.05], 1.0)))
        .chain([([0.1, -0.05], 0.0)]);
    for (frame, (shift, trigger)) in frames.enumerate() {
        let aim = corner_aim(&space.panels()[0], shift, trigger);
        let time = Duration::from_millis(10 * frame as u64);
        pointer.update(
            [None, aim],
            time,
            (&space.panels(), &PickMeshes::default()),
            (PointerMapping::default(), true),
        );
        for step in pointer.resizes() {
            space.resize(step);
        }
    }
}

#[test]
fn a_resized_panel_tells_the_app_each_size_and_leaves_the_size_to_it() {
    let events = Rc::new(RefCell::new(Vec::<SpatialResizeEvent>::new()));
    let heard = Rc::clone(&events);
    let policy = ResizePolicy::custom(move |event| heard.borrow_mut().push(event));
    let (_robot, space) = headset(SubspaceModifier::new().size(400.0, 300.0).resizable(policy));
    resize_right(&space);

    let events = events.borrow();
    let [start, resizing @ .., end] = events.as_slice() else {
        panic!("a resize has a start, steps and an end: {events:?}");
    };
    assert_eq!(start.event_type, SpatialResizeEventType::Start);
    assert_eq!(start.size, [400.0, 300.0]);
    assert!(!resizing.is_empty());
    assert!(
        resizing
            .iter()
            .all(|event| event.event_type == SpatialResizeEventType::Resizing)
    );
    assert_eq!(end.event_type, SpatialResizeEventType::End);
    assert_eq!(end.size, [600.0, 400.0]);
    assert_eq!(resizing.last().map(|event| event.size), Some(end.size));
    assert_eq!(
        space.panels()[0].size(),
        [400.0, 300.0],
        "the modifier still sizes the panel"
    );
}

#[test]
fn a_resizable_surface_reports_its_sizes_like_a_panel() {
    let events = Rc::new(RefCell::new(Vec::<SpatialResizeEvent>::new()));
    let heard = Rc::clone(&events);
    let limits = ResizeLimits::new().maintain_aspect_ratio(true);
    let modifier = SubspaceModifier::new().size(400.0, 300.0).resizable_with(
        limits,
        ResizePolicy::custom(move |event| heard.borrow_mut().push(event)),
    );
    let (_robot, space) = surface_headset(modifier, &Rc::default());
    resize_right(&space);

    let events = events.borrow();
    assert_eq!(events.first().map(|event| event.size), Some([400.0, 300.0]));
    // The drag to the right and down is nearest to 4:3 along its own line
    let last = events.last().expect("the resize ends");
    assert_eq!(last.event_type, SpatialResizeEventType::End);
    assert!((last.size[0] / last.size[1] - 4.0 / 3.0).abs() < 0.01);
    assert!(last.size[0] > 400.0);
}

// A pointer event of a surface as the test sees it: its kind, its place in dp and whether the
// primary button is down.
type Seen = (PointerEventKind, [f32; 2], bool);

// A headset whose content is a surface that `modifier` places. Its pointer callback writes the
// events into `seen`.
fn surface_headset(
    modifier: SubspaceModifier,
    seen: &Rc<RefCell<Vec<Seen>>>,
) -> (RobotTestRule<TestRenderer>, Space) {
    let held = Rc::new(RefCell::new(None));
    let (keep, seen) = (Rc::clone(&held), Rc::clone(seen));
    let mut robot = create_headless_robot_test(500, 400, move || {
        let space = remember(Space::new).with(Clone::clone);
        keep.replace(Some(space.clone()));
        let (seen, modifier) = (Rc::clone(&seen), modifier.clone());
        CompositionLocalProvider([local_space().provides(Some(space))], move || {
            let seen = Rc::clone(&seen);
            SpatialExternalSurface(StereoMode::Mono, modifier, move |scope| {
                let seen = Rc::clone(&seen);
                scope.on_pointer_event(move |event| {
                    let primary = event.buttons.contains(PointerButton::Primary);
                    let place = [event.position.x, event.position.y];
                    seen.borrow_mut().push((event.kind, place, primary));
                });
            });
        });
    });
    robot.wait_for_idle();
    let space = held.take().expect("the content composed");
    (robot, space)
}

#[test]
fn a_ray_on_a_surface_gives_the_app_pointer_events_in_the_surfaces_dp() {
    let seen = Rc::new(RefCell::new(Vec::new()));
    let (_robot, space) = surface_headset(SubspaceModifier::new().size(960.0, 540.0), &seen);
    let mut frames = Frames::default();
    // The frame loop's way: the pointer's gestures reach the surface through the space
    let mut frame = |x: f32, yaw: f32, trigger: f32| {
        let (atlas, _) = frames.update([None, aim(x, yaw, trigger)], &space.panels());
        assert!(
            atlas.is_empty(),
            "the composition hears nothing of a surface"
        );
        for gesture in frames.pointer.surface_events() {
            space.deliver_to_surface(gesture);
        }
        seen.take()
    };
    let at = |kind, x, y, down| vec![(kind, [x, y], down)];
    let close = |seen: &[Seen], expected: &[Seen]| {
        assert_eq!(seen.len(), expected.len(), "{seen:?}");
        for (seen, expected) in seen.iter().zip(expected) {
            assert_eq!((seen.0, seen.2), (expected.0, expected.2));
            assert!(
                seen.1
                    .iter()
                    .zip(expected.1)
                    .all(|(a, b)| (a - b).abs() < 0.1)
            );
        }
    };

    // The surface is a meter ahead, and a dp is a millimeter: the middle is at 480 by 270 dp
    close(
        &frame(0.0, 0.0, 0.0),
        &at(PointerEventKind::Enter, 480.0, 270.0, false),
    );
    close(
        &frame(0.1, 0.0, 0.0),
        &at(PointerEventKind::Move, 580.0, 270.0, false),
    );
    close(
        &frame(0.1, 0.0, 1.0),
        &at(PointerEventKind::Down, 580.0, 270.0, true),
    );
    // The ray leaves the surface during the press. The release still reaches the surface, at the
    // last place on it.
    assert!(frame(0.1, 90.0, 1.0).is_empty());
    let released = frame(0.1, 90.0, 0.0);
    close(
        &released,
        &[
            (PointerEventKind::Up, [580.0, 270.0], false),
            (PointerEventKind::Exit, [580.0, 270.0], false),
        ],
    );
}

#[test]
fn a_movable_surface_is_carried_and_reports_its_pose_like_a_panel() {
    let events = Rc::new(RefCell::new(Vec::<SpatialMoveEvent>::new()));
    let heard = Rc::clone(&events);
    let policy = MovePolicy::system(move |event| heard.borrow_mut().push(event));
    let modifier = SubspaceModifier::new()
        .size(960.0, 540.0)
        .movable_with(policy);
    let (_robot, space) = surface_headset(modifier, &Rc::default());
    carry_right(&space);

    let events = events.borrow();
    assert_eq!(phases(&events), CARRY_PHASES);
    assert!((events[3].pose.position[0] - 0.1).abs() < 1e-4);
    let carried = space.panels()[0].pose;
    assert!((carried.position[0] - 0.1).abs() < 1e-4);
}

// A headset whose content is a volume that `modifier` places. Its pointer callback writes the
// events into `seen`.
fn volume_headset(
    modifier: SubspaceModifier,
    seen: &Rc<RefCell<Vec<Seen>>>,
) -> (RobotTestRule<TestRenderer>, Space) {
    let held = Rc::new(RefCell::new(None));
    let (keep, seen) = (Rc::clone(&held), Rc::clone(seen));
    let mut robot = create_headless_robot_test(500, 400, move || {
        let space = remember(Space::new).with(Clone::clone);
        keep.replace(Some(space.clone()));
        let (seen, modifier, host) = (Rc::clone(&seen), modifier.clone(), space.clone());
        CompositionLocalProvider([local_space().provides(Some(space))], move || {
            let seen = Rc::clone(&seen);
            SpatialVolume(modifier.clone(), move |scope| {
                let seen = Rc::clone(&seen);
                scope.on_pointer_event(move |event| {
                    let primary = event.buttons.contains(PointerButton::Primary);
                    let place = [event.position.x, event.position.y];
                    seen.borrow_mut().push((event.kind, place, primary));
                });
            });
            // The scene composes the volume's content, which sets the callback
            VolumeScene(host);
        });
    });
    robot.wait_for_idle();
    let space = held.take().expect("the content composed");
    (robot, space)
}

// A volume's box is 960 by 540 dp wide and high and 300 dp deep, centered a meter ahead.
fn box_modifier() -> SubspaceModifier {
    SubspaceModifier::new().size(960.0, 540.0).depth(300.0)
}

#[test]
fn a_ray_stops_at_the_front_of_a_volume_and_gives_the_app_pointer_events_in_dp() {
    let seen = Rc::new(RefCell::new(Vec::new()));
    let (_robot, space) = volume_headset(box_modifier(), &seen);
    let mut frames = Frames::default();
    let mut frame = |x: f32, trigger: f32| {
        let (atlas, _) = frames.update([None, aim(x, 0.0, trigger)], &space.panels());
        assert!(
            atlas.is_empty(),
            "the composition hears nothing of a volume"
        );
        for gesture in frames.pointer.surface_events() {
            space.deliver_to_surface(gesture);
        }
        let beam = frames.pointer.beams()[1].expect("the right ray shows");
        (seen.take(), beam.length)
    };
    // The front face of the box is 15 cm before its center, which stands a meter ahead
    let (events, length) = frame(0.0, 0.0);
    assert!((length - 0.85).abs() < 1e-4, "the ray is {length} m long");
    assert_eq!(events.len(), 1);
    assert_eq!(events[0].0, PointerEventKind::Enter);
    assert!((events[0].1[0] - 480.0).abs() < 0.1 && (events[0].1[1] - 270.0).abs() < 0.1);
    let (events, _) = frame(0.1, 0.0);
    assert_eq!(events[0].0, PointerEventKind::Move);
    assert!((events[0].1[0] - 580.0).abs() < 0.1);
    let (events, _) = frame(0.1, 1.0);
    assert_eq!((events[0].0, events[0].2), (PointerEventKind::Down, true));
    let (events, _) = frame(0.1, 0.0);
    assert_eq!((events[0].0, events[0].2), (PointerEventKind::Up, false));
}

#[test]
fn a_ray_that_enters_a_volume_by_its_side_reports_the_place_on_its_front_face() {
    let seen = Rc::new(RefCell::new(Vec::new()));
    let (_robot, space) = volume_headset(box_modifier(), &seen);
    let mut frames = Frames::default();
    // From the right, level with the center, pointing left through the box's right face
    let aim = Some(Aim {
        ray: Ray {
            origin: [0.7, 0.0, -1.0],
            direction: [-1.0, 0.0, 0.0],
        },
        pose: Pose::at([0.7, 0.0, -1.0]),
        trigger: 0.0,
        stick: [0.0; 2],
        buttons: Buttons::default(),
        source: InputSource::Controller,
    });
    frames.update([None, aim], &space.panels());
    for gesture in frames.pointer.surface_events() {
        space.deliver_to_surface(gesture);
    }
    let seen = seen.take();
    assert_eq!(seen.len(), 1);
    // The right edge of the face, at its middle height
    assert!((seen[0].1[0] - 960.0).abs() < 0.1 && (seen[0].1[1] - 270.0).abs() < 0.1);
}

#[test]
fn a_volume_in_front_of_a_panel_takes_the_ray_and_the_panel_behind_it_does_not() {
    let seen = Rc::new(RefCell::new(Vec::new()));
    let (_robot, space) = volume_headset(box_modifier(), &seen);
    // A panel of the atlas stands a meter behind the box's center
    let behind = PanelSlot {
        pose: Pose::at([0.0, 0.0, -2.0]),
        ..PANEL
    };
    let mut panels = space.panels();
    panels.push(behind);
    let mut frames = Frames::default();
    let (atlas, cursor) = frames.update([None, aim(0.0, 0.0, 1.0)], &panels);
    assert!(atlas.is_empty());
    assert_eq!(cursor, None, "the panel behind shows no cursor");
}

// A volume of 960 by 540 dp and 300 dp deep, centered a meter ahead, that holds a square mesh of
// 200 dp in its center plane.
fn square_volume() -> (PanelSlot, PickMeshes) {
    let slot = PanelSlot {
        id: 9,
        kind: PanelKind::Volume { depth: 300.0 },
        atlas: [0.0, 0.0, 960.0, 540.0],
        pose: Pose::at([0.0, 0.0, -1.0]),
        mobility: Mobility::Fixed,
        shape: Shape::Flat,
        resize: None,
        room: [960.0, 540.0],
        priority: 0,
        ride: Ride::Free,
    };
    let square = std::sync::Arc::new(cranpose::space::Mesh::plane());
    let mut picks = PickMeshes::default();
    picks.replace(slot.id, |visit| {
        let surface = cranpose::space::MeshSurface {
            mesh: square,
            transform: cranpose::space::Mat4::IDENTITY,
            material: cranpose::space::Material::Unlit,
            casts_shadow: false,
            solid: true,
        };
        visit(&cranpose::MeshVisit {
            node: Some(cranpose_core::NodeId(4)),
            mesh: 0,
            surface: &surface,
            // The plane is the unit square from the corner: 200 dp wide around the center
            to_frame: cranpose::space::Mat4::scaling(cranpose::space::Vec3::new(200.0, 200.0, 1.0))
                .then(&cranpose::space::Mat4::translation(
                    cranpose::space::Vec3::new(-100.0, -100.0, 0.0),
                )),
        });
    });
    (slot, picks)
}

#[test]
fn a_ray_stops_at_the_mesh_inside_a_volume_and_passes_through_its_empty_box() {
    let (volume, picks) = square_volume();
    let mut frames = Frames {
        picks,
        ..Frames::default()
    };
    // A panel of the atlas stands a meter behind the volume's center
    let behind = PanelSlot {
        pose: Pose::at([0.0, 0.0, -2.0]),
        ..PANEL
    };
    let panels = [volume, behind];
    // A ray at the mesh stops at its plane, a meter away, beyond the box's front face at 85 cm.
    // The gesture's place is the hit's, 5 cm right of the center, from the face's corner.
    frames.update([None, aim(0.05, 0.0, 0.0)], &panels);
    let beam = frames.pointer.beams()[1].expect("the ray shows");
    assert!((beam.length - 1.0).abs() < 1e-4, "{}", beam.length);
    let [gesture] = frames.pointer.surface_events() else {
        panic!("one gesture on the volume");
    };
    assert_eq!(gesture.surface, volume.id);
    let Gesture::Move { x, y } = gesture.gesture else {
        panic!("a move");
    };
    assert!(
        (x - 530.0).abs() < 0.1 && (y - 270.0).abs() < 0.1,
        "{x} {y}"
    );
    let hit = gesture
        .ray
        .and_then(|ray| ray.hit)
        .expect("the ray names its mesh");
    assert_eq!(hit.mesh.node, cranpose_core::NodeId(4));

    // A ray inside the box and beside the mesh passes through: it leaves the volume and meets the
    // panel behind it
    let (atlas, _) = frames.update([None, aim(0.25, 0.0, 0.0)], &panels);
    assert_eq!(
        frames.pointer.surface_events()[0].gesture,
        Gesture::Leave,
        "the ray left the volume"
    );
    assert_eq!(atlas.len(), 1, "the panel behind hears the ray");
}

#[test]
fn a_movable_volume_is_carried_and_the_scene_reads_its_new_pose() {
    let events = Rc::new(RefCell::new(Vec::<SpatialMoveEvent>::new()));
    let heard = Rc::clone(&events);
    let policy = MovePolicy::system(move |event| heard.borrow_mut().push(event));
    let (_robot, space) = volume_headset(box_modifier().movable_with(policy), &Rc::default());
    carry_right(&space);

    assert_eq!(phases(&events.borrow()), CARRY_PHASES);
    // The scene draws the volume where the user left it, with nothing composed
    let content = space.volumes().contents().get().remove(0);
    assert!((content.pose.get().position[0] - 0.1).abs() < 1e-4);
}

#[test]
fn a_resizable_volume_reports_the_size_of_its_front_face() {
    let sizes = Rc::new(RefCell::new(Vec::<SpatialResizeEvent>::new()));
    let heard = Rc::clone(&sizes);
    let modifier = box_modifier().resizable(ResizePolicy::custom(move |event| {
        heard.borrow_mut().push(event);
    }));
    let (_robot, space) = volume_headset(modifier, &Rc::default());
    let volume = space.panels()[0];
    let mut frames = Frames::default();
    let mut frame = |shift: [f32; 2], trigger: f32| {
        frames.update([None, corner_aim(&volume, shift, trigger)], &space.panels());
        for step in frames.pointer.resizes() {
            space.resize(step);
        }
    };
    frame([0.0, 0.0], 0.0);
    frame([0.0, 0.0], 1.0);
    frame([0.05, -0.05], 1.0);
    frame([0.05, -0.05], 0.0);
    let sizes = sizes.borrow();
    assert_eq!(
        sizes
            .iter()
            .map(|event| event.event_type)
            .collect::<Vec<_>>(),
        [
            SpatialResizeEventType::Start,
            SpatialResizeEventType::Resizing,
            SpatialResizeEventType::End
        ]
    );
    assert_eq!(sizes[0].size, [960.0, 540.0]);
    assert!(sizes[1].size[0] > 960.0 && sizes[1].size[1] > 540.0);
}

// A ball of 40 by 40 dp at the controllers' height, half a meter ahead of `PANEL`, which `ride`
// places.
fn ball(ride: Ride) -> PanelSlot {
    PanelSlot {
        id: 3,
        atlas: [0.0, 1000.0, 40.0, 40.0],
        pose: Pose::at([0.0, 0.0, -0.5]),
        ride,
        ..PANEL
    }
}

#[test]
fn a_controllers_own_ray_ignores_the_panels_that_ride_it() {
    let panels = [ball(Ride::On(Side::Left)), PANEL];
    let mut frames = Frames::default();
    // The left ray passes through the ball above the left controller and meets the panel behind it
    let (events, cursor) = frames.update([aim(0.0, 0.0, 0.0), None], &panels);
    assert_eq!(cursor.map(|cursor| cursor.panel), Some(1));
    assert_eq!(events, [Gesture::Move { x: 400.0, y: 250.0 }]);
    assert!(
        frames.pointer.beams()[0].is_some_and(|beam| (beam.length - 1.0).abs() < 1e-4),
        "the ray ends at the panel behind the ball"
    );
    let (events, _) = frames.update([aim(0.0, 0.0, 1.0), None], &panels);
    assert_eq!(events, [PRESS]);

    // The right ray hits the ball and presses it as it does any panel
    let mut frames = Frames::default();
    let (_, cursor) = frames.update([None, aim(0.0, 0.0, 0.0)], &panels);
    assert_eq!(cursor.map(|cursor| cursor.panel), Some(3));
    assert!(
        frames.pointer.beams()[1].is_some_and(|beam| (beam.length - 0.5).abs() < 1e-4),
        "the ray ends at the ball"
    );
    let (events, cursor) = frames.update([None, aim(0.0, 0.0, 1.0)], &panels);
    assert_eq!(events, [PRESS]);
    assert!(cursor.is_some_and(|cursor| cursor.pressed && cursor.panel == 3));
}

#[test]
fn a_ball_of_the_right_controller_is_out_of_reach_of_the_right_ray_only() {
    let panels = [ball(Ride::On(Side::Right)), PANEL];
    let mut frames = Frames::default();
    let (_, cursor) = frames.update([None, aim(0.0, 0.0, 0.0)], &panels);
    assert_eq!(cursor.map(|cursor| cursor.panel), Some(1));
    let (_, cursor) = frames.update([aim(0.0, 0.0, 0.0), None], &panels);
    assert_eq!(cursor.map(|cursor| cursor.panel), Some(3));
}

#[test]
fn a_hidden_panel_takes_no_hits_from_either_ray() {
    let panels = [ball(Ride::Hidden), PANEL];
    let mut frames = Frames::default();
    for aims in [[aim(0.0, 0.0, 0.0), None], [None, aim(0.0, 0.0, 0.0)]] {
        let (_, cursor) = frames.update(aims, &panels);
        assert_eq!(cursor.map(|cursor| cursor.panel), Some(1));
    }
}

#[test]
fn a_pinch_over_a_panel_moves_presses_and_releases() {
    let mut frames = Frames::default();
    let (events, cursor) = frames.right_hand(0.0, 0.0, 0.0);
    assert_eq!(events, [Gesture::Move { x: 400.0, y: 250.0 }]);
    assert_eq!(
        cursor.map(|cursor| (cursor.panel, cursor.x, cursor.y)),
        Some((1, 400.0, 250.0))
    );
    // The hand's aim shows as the ray, which ends at the panel
    let beam = frames.pointer.beams()[1].expect("the hand's ray shows");
    assert_eq!(beam.ray.origin, [0.0, 0.0, 0.0]);
    assert!((beam.length - 1.0).abs() < 1e-4);
    assert_eq!(frames.pointer.beams()[0], None);
    let (events, cursor) = frames.right_hand(0.0, 0.0, 1.0);
    assert_eq!(events, [PRESS]);
    assert!(cursor.is_some_and(|cursor| cursor.pressed));
    let (events, _) = frames.right_hand(0.0, 0.0, 0.0);
    assert_eq!(events, [RELEASE]);
}

#[test]
fn a_half_closed_pinch_neither_presses_nor_releases() {
    let mut frames = Frames::default();
    frames.right_hand(0.0, 0.0, 0.0);
    assert!(frames.right_hand(0.0, 0.0, 0.5).0.is_empty());
    assert_eq!(frames.right_hand(0.0, 0.0, 0.7).0, [PRESS]);
    assert!(frames.right_hand(0.0, 0.0, 0.5).0.is_empty());
    assert_eq!(frames.right_hand(0.0, 0.0, 0.3).0, [RELEASE]);
}

#[test]
fn a_pinching_hand_clicks_through_a_shake_and_drags_when_it_turns() {
    let mut frames = Frames::default();
    frames.right_hand(0.0, 0.0, 0.0);
    frames.right_hand(0.0, 0.0, 1.0);
    for frame in 0..60 {
        let yaw = if frame % 2 == 0 { 3.0 } else { -3.0 };
        let (events, cursor) = frames.right_hand(0.0, yaw, 1.0);
        assert!(events.is_empty());
        assert_eq!(cursor.map(|cursor| cursor.x), Some(400.0));
    }
    assert_eq!(frames.right_hand(0.0, 0.0, 0.0).0.first(), Some(&RELEASE));

    frames.right_hand(0.0, 0.0, 1.0);
    let moved = (0..60).any(|_| {
        let (events, _) = frames.right_hand(0.0, 15.0, 1.0);
        matches!(events.first(), Some(Gesture::Move { x, .. }) if *x > 400.0)
    });
    assert!(moved);
}

#[test]
fn closing_the_pinch_turns_the_hand_and_the_press_keeps_the_point_before_it() {
    let mut frames = Frames::default();
    frames.right_hand(0.0, 0.0, 0.0);
    // The pinch's own motion turns the aim by 6 degrees before the fingers meet
    frames.right_hand(0.0, 3.0, 0.3);
    let (events, cursor) = frames.right_hand(0.0, 6.0, 1.0);
    assert_eq!(events, [Gesture::Move { x: 400.0, y: 250.0 }, PRESS]);
    assert_eq!(cursor.map(|cursor| cursor.x), Some(400.0));
}

#[test]
fn a_hand_that_makes_the_system_gesture_does_not_press_and_lets_a_press_go() {
    let mut frames = Frames::default();
    frames.right_hand(0.0, 0.0, 0.0);
    assert_eq!(frames.right_hand(0.0, 0.0, 1.0).0, [PRESS]);
    let mut gesture = tracked_hand(0.0, 0.0, 1.0);
    if let Some(aim) = &mut gesture.aim {
        aim.system_gesture = true;
    }
    // The palm turns up and the pinch is the menu's. The press ends and its ray goes away
    let (events, cursor) = frames.right_state(&gesture);
    assert_eq!(events, [RELEASE, Gesture::Leave]);
    assert_eq!(cursor, None);
    assert_eq!(frames.pointer.beams(), [None, None]);
    for _ in 0..5 {
        let (events, cursor) = frames.right_state(&gesture);
        assert!(events.is_empty());
        assert_eq!(cursor, None);
    }
    // The palm turns away with the fingers still together. That pinch began as the menu's
    let (events, _) = frames.right_hand(0.0, 0.0, 1.0);
    assert_eq!(events, [Gesture::Move { x: 400.0, y: 250.0 }]);
    assert!(frames.right_hand(0.0, 0.0, 1.0).0.is_empty());
    assert!(frames.right_hand(0.0, 0.0, 0.0).0.is_empty());
    assert_eq!(frames.right_hand(0.0, 0.0, 1.0).0, [PRESS]);
}

#[test]
fn a_pinch_that_is_closed_when_the_hand_appears_does_not_press() {
    let mut frames = Frames::default();
    let (events, cursor) = frames.right_hand(0.0, 0.0, 1.0);
    assert_eq!(events, [Gesture::Move { x: 400.0, y: 250.0 }]);
    assert!(cursor.is_some_and(|cursor| !cursor.pressed));
    assert!(frames.right_hand(0.0, 0.0, 1.0).0.is_empty());
    assert!(frames.right_hand(0.0, 0.0, 0.0).0.is_empty());
    assert_eq!(frames.right_hand(0.0, 0.0, 1.0).0, [PRESS]);
}

#[test]
fn a_hand_that_the_cameras_lose_lets_its_press_go_and_presses_again_after_a_new_pinch() {
    let mut frames = Frames::default();
    frames.right_hand(0.0, 0.0, 0.0);
    assert_eq!(frames.right_hand(0.0, 0.0, 1.0).0, [PRESS]);
    let lost = HandState {
        tracking_state: TrackingState::Paused,
        ..HandState::default()
    };
    assert_eq!(frames.right_state(&lost).0, [RELEASE, Gesture::Leave]);
    // The cameras find the hand with its fingers still together
    assert!(
        frames
            .right_hand(0.0, 0.0, 1.0)
            .0
            .contains(&Gesture::Move { x: 400.0, y: 250.0 })
    );
    assert!(!frames.right_hand(0.0, 0.0, 1.0).0.contains(&PRESS));
    frames.right_hand(0.0, 0.0, 0.0);
    assert_eq!(frames.right_hand(0.0, 0.0, 1.0).0, [PRESS]);
}

#[test]
fn a_switch_from_the_controller_to_the_hand_in_a_press_releases_it_cleanly() {
    let mut frames = Frames::default();
    frames.update([None, aim(0.0, 0.0, 0.0)], &[PANEL]);
    assert_eq!(
        frames.update([None, aim(0.0, 0.0, 1.0)], &[PANEL]).0,
        [PRESS]
    );
    // The user lets go of the controller and the cameras find the hand, with its fingers open
    let (events, cursor) = frames.right_hand(0.0, 0.0, 0.0);
    assert_eq!(events, [RELEASE, Gesture::Leave]);
    assert_eq!(cursor, None);
    assert_eq!(
        frames.right_hand(0.0, 0.0, 0.0).0,
        [Gesture::Move { x: 400.0, y: 250.0 }]
    );
    assert_eq!(frames.right_hand(0.0, 0.0, 1.0).0, [PRESS]);
    assert_eq!(frames.right_hand(0.0, 0.0, 0.0).0, [RELEASE]);
}

#[test]
fn a_switch_from_the_hand_to_the_controller_in_a_press_releases_it_cleanly() {
    let mut frames = Frames::default();
    frames.right_hand(0.0, 0.0, 0.0);
    assert_eq!(frames.right_hand(0.0, 0.0, 1.0).0, [PRESS]);
    // The controller's trigger is down as the controller takes the side over
    let (events, _) = frames.update([None, aim(0.0, 0.0, 1.0)], &[PANEL]);
    assert_eq!(events, [RELEASE, Gesture::Leave]);
    assert!(
        frames
            .update([None, aim(0.0, 0.0, 1.0)], &[PANEL])
            .0
            .contains(&Gesture::Move { x: 400.0, y: 250.0 })
    );
    // That trigger was held from before the switch, so it presses only after it went up
    assert!(
        frames
            .update([None, aim(0.0, 0.0, 1.0)], &[PANEL])
            .0
            .is_empty()
    );
    assert_eq!(frames.update([None, aim(0.0, 0.0, 0.0)], &[PANEL]).0, []);
    assert_eq!(
        frames.update([None, aim(0.0, 0.0, 1.0)], &[PANEL]).0,
        [PRESS]
    );
}

#[test]
fn a_controller_in_one_hand_and_a_hand_on_the_other_side_each_point() {
    let mut frames = Frames::default();
    // The left hand has a controller in use. The cameras track both hands, and the right one has
    // no controller
    let left = pointer_aim(
        &controller_in_use(-0.3, 0.0),
        Some(&tracked_hand(-0.3, 0.0, 1.0)),
    );
    let right = hand_aim_of(&tracked_hand(0.3, 0.0, 0.0));
    let (events, _) = frames.update([left, right], &[PANEL]);
    assert!(!events.is_empty());
    let beams = frames.pointer.beams();
    assert_eq!(beams[0].map(|beam| beam.ray.origin), Some([-0.3, 0.0, 0.0]));
    assert_eq!(beams[1].map(|beam| beam.ray.origin), Some([0.3, 0.0, 0.0]));
    // The pinch of the right hand takes the pointer from the left controller
    let right = hand_aim_of(&tracked_hand(0.3, 0.0, 1.0));
    let (events, cursor) = frames.update([left, right], &[PANEL]);
    assert_eq!(events.last(), Some(&PRESS));
    assert!(cursor.is_some_and(|cursor| cursor.pressed));
}

#[test]
fn an_app_that_did_not_ask_for_hand_tracking_gets_no_pointer_from_a_pinch() {
    let mut frames = Frames::default();
    // The frame holds the pinch, and the backend passes the hand on only for an app that asked
    let asked = false;
    for strength in [0.0, 1.0, 0.0] {
        let hand = tracked_hand(0.0, 0.0, strength);
        let aim = pointer_aim(&Controller::default(), asked.then_some(&hand));
        let (events, cursor) = frames.update([None, aim], &[PANEL]);
        assert!(events.is_empty());
        assert_eq!(cursor, None);
    }
    assert_eq!(frames.pointer.beams(), [None, None]);
}

// A panel of 500 by 400 dp a meter ahead, as large as the headless window that shows its content.
const BUTTON_PANEL: PanelSlot = PanelSlot {
    id: 1,
    kind: PanelKind::Content,
    atlas: [0.0, 0.0, 500.0, 400.0],
    pose: Pose::at([0.0, 0.0, -1.0]),
    mobility: Mobility::Fixed,
    shape: Shape::Flat,
    resize: None,
    room: [500.0, 400.0],
    priority: 0,
    ride: Ride::Free,
};

// Where the center of the button below lies in the room: 100 dp from the panel's left edge and 50
// dp from its top.
const BUTTON_AT: [f32; 3] = [-0.15, 0.15, -1.0];

// A window with a button of 200 by 100 dp at its top left, and the count of its clicks.
fn counting_button() -> (RobotTestRule<TestRenderer>, Rc<Cell<u32>>) {
    let clicks = Rc::new(Cell::new(0));
    let counted = Rc::clone(&clicks);
    let mut robot = create_headless_robot_test(500, 400, move || {
        let counted = Rc::clone(&counted);
        Button(
            Modifier::empty().size_points(200.0, 100.0),
            ButtonSpec::default(),
            move || counted.set(counted.get() + 1),
            || {
                Layer(Modifier::empty(), BoxSpec::default(), || {});
            },
        );
    });
    robot.wait_for_idle();
    (robot, clicks)
}

// The clicks that the button counts while the pointer follows `aims` in frames 10 ms apart, with
// the events sent to the window as the frame loop of the headset sends them.
fn clicks_of(aims: impl IntoIterator<Item = Option<Aim>>) -> u32 {
    let (mut robot, clicks) = counting_button();
    let mut pointer = Pointers::default();
    for (frame, aim) in (0_u64..).zip(aims) {
        pointer.update(
            [None, aim],
            millis(10 * frame),
            (&[BUTTON_PANEL], &PickMeshes::default()),
            (PointerMapping::default(), true),
        );
        atlas_input::deliver(robot.shell_mut(), pointer.atlas());
        robot.wait_for_idle();
    }
    clicks.get()
}

fn millis(millis: u64) -> Duration {
    Duration::from_millis(millis)
}

#[test]
fn a_controller_press_on_a_button_counts_one_click() {
    let trigger = [0.0, 0.0, 0.0, 0.8, 1.0, 1.0, 1.0, 0.6, 0.0, 0.0];
    let clicks = clicks_of(
        trigger
            .into_iter()
            .map(|trigger| aim_at([0.0, 0.0, 0.0], BUTTON_AT, trigger)),
    );
    assert_eq!(clicks, 1);
}

#[test]
fn a_controller_trigger_that_never_comes_down_on_the_button_counts_no_click() {
    let beside = [0.4, 0.15, -1.0];
    let trigger = [0.0, 0.0, 1.0, 1.0, 0.0, 0.0];
    let clicks = clicks_of(
        trigger
            .into_iter()
            .map(|trigger| aim_at([0.0, 0.0, 0.0], beside, trigger)),
    );
    assert_eq!(clicks, 0);
}

#[test]
fn a_pinch_on_a_button_counts_one_click_per_pinch() {
    // Two cycles of the scripted hand: rest, close, hold and open, twice
    let script = ScriptedHand::pointing(Side::Right, [0.0; 3], BUTTON_AT, PinchCycle::default());
    let aims = (0_u64..600).map(|frame| {
        let mut xr = XrFrame {
            predicted_display_time: millis(10 * frame),
            ..XrFrame::default()
        };
        play(&script, &mut xr);
        pointer_aim(&Controller::default(), Some(&xr.right_hand))
    });
    assert_eq!(clicks_of(aims), 2);
}
