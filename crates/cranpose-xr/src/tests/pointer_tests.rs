use std::{cell::RefCell, rc::Rc};

use cranpose::{PointerButton, PointerEventKind};
use cranpose_core::{CompositionLocalProvider, remember};
use cranpose_testing::{
    create_headless_robot_test,
    robot::{RobotTestRule, TestRenderer},
};

use super::*;
use crate::{
    MovePolicy, Pose, ResizeLimits, ResizePolicy, SpatialExternalSurface, SpatialMoveEvent,
    SpatialMoveEventType, SpatialPanel, SpatialResizeEvent, SpatialResizeEventType, StereoMode,
    SubspaceModifier,
    handle::HandleKind,
    space::{Mobility, SUBSPACE_ORIGIN, Shape, Space, local_space},
};

// A panel of 800 by 500 dp a meter ahead, at the atlas's top.
const PANEL: PanelSlot = PanelSlot {
    id: 1,
    kind: PanelKind::Content,
    atlas: [0.0, 0.0, 800.0, 500.0],
    pose: Pose::at([0.0, 0.0, -1.0]),
    mobility: Mobility::Fixed,
    shape: Shape::Flat,
    resize: None,
    room: [800.0, 500.0],
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
    ride: Ride::Free,
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

// A controller at `origin` that points at `target`, with its trigger pulled by `trigger`.
fn aim_at(origin: [f32; 3], target: [f32; 3], trigger: f32) -> Option<Aim> {
    let to_target = [
        target[0] - origin[0],
        target[1] - origin[1],
        target[2] - origin[2],
    ];
    let length = to_target.iter().map(|v| v * v).sum::<f32>().sqrt();
    Some(Aim {
        ray: Ray {
            origin,
            direction: to_target.map(|v| v / length),
        },
        pose: Pose::at(origin),
        trigger,
        stick: 0.0,
    })
}

// A controller at `x` that points at the handle under `slot`'s panel, with its trigger pulled by
// `trigger`.
fn handle_aim(slot: &PanelSlot, x: f32, trigger: f32) -> Option<Aim> {
    aim_at([x, 0.0, 0.0], crate::carry::handle_center(slot), trigger)
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
    ) -> (Vec<Gesture>, Option<Cursor>) {
        self.frame += 1;
        let time = Duration::from_millis(10 * self.frame);
        let (events, cursor) = self.pointer.update(aims, time, panels);
        (events.to_vec(), cursor)
    }

    fn right(&mut self, x: f32, yaw: f32, trigger: f32) -> (Vec<Gesture>, Option<Cursor>) {
        self.update([None, aim(x, yaw, trigger)], &[PANEL])
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
    assert_eq!(events, [Gesture::Press]);
    assert!(cursor.is_some_and(|cursor| cursor.pressed));
    let (events, _) = frames.right(0.0, 0.0, 0.0);
    assert_eq!(events, [Gesture::Release]);
}

#[test]
fn a_half_pulled_trigger_neither_presses_nor_releases() {
    let mut frames = Frames::default();
    frames.right(0.0, 0.0, 0.0);
    assert!(frames.right(0.0, 0.0, 0.5).0.is_empty());
    assert_eq!(frames.right(0.0, 0.0, 0.7).0, [Gesture::Press]);
    assert!(frames.right(0.0, 0.0, 0.5).0.is_empty());
    assert_eq!(frames.right(0.0, 0.0, 0.3).0, [Gesture::Release]);
}

#[test]
fn a_press_takes_the_point_from_before_the_pull() {
    let mut frames = Frames::default();
    frames.right(0.0, 0.0, 0.0);
    frames.right(0.05, 0.0, 0.3);
    let (events, cursor) = frames.right(0.1, 0.0, 0.8);
    assert_eq!(
        events,
        [Gesture::Move { x: 400.0, y: 250.0 }, Gesture::Press]
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
    assert_eq!(events.first(), Some(&Gesture::Release));

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
    assert_eq!(
        events,
        [Gesture::Move { x: 400.0, y: 250.0 }, Gesture::Press]
    );
    for _ in 0..60 {
        let (events, cursor) = frames.right(0.0, 6.0, 1.0);
        assert!(events.is_empty());
        assert_eq!(cursor.map(|cursor| cursor.x), Some(400.0));
    }
    assert_eq!(
        frames.right(0.0, 6.0, 0.0).0.first(),
        Some(&Gesture::Release)
    );
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
    assert_eq!(
        frames.right(0.0, 8.0, 0.3).0.first(),
        Some(&Gesture::Release)
    );
}

#[test]
fn a_pull_that_aims_anew_presses_where_the_ray_points_now() {
    let mut frames = Frames::default();
    frames.right(0.0, 0.0, 0.0);
    for _ in 0..10 {
        frames.right(0.0, 15.0, 0.3);
    }
    let (events, cursor) = frames.right(0.0, 15.0, 1.0);
    assert_eq!(events.last(), Some(&Gesture::Press));
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
    assert_eq!(frames.right(0.0, 0.0, 1.0).0, [Gesture::Press]);
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
        [Gesture::Release]
    );
    assert!(frames.update(both(0.0, 0.0), &[PANEL]).0.is_empty());
    assert_eq!(
        frames.update(both(1.0, 0.0), &[PANEL]).0,
        [Gesture::Move { x: 500.0, y: 250.0 }, Gesture::Press]
    );
}

#[test]
fn a_lost_controller_releases_its_press() {
    let mut frames = Frames::default();
    frames.right(0.0, 0.0, 0.0);
    frames.right(0.0, 0.0, 1.0);
    let (events, _) = frames.update([None, None], &[PANEL]);
    assert_eq!(events, [Gesture::Release, Gesture::Leave]);
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
    frames.update([None, aim(0.0, 0.0, 0.0)], &[]);
    let beam = frames.pointer.beams()[1];
    assert!(beam.is_some_and(|beam| (beam.length - RAY_LENGTH).abs() < 1e-4));
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
    assert_eq!(frames.pointer.handle(), Some((1, HandleKind::Move)));
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
const MOVABLE: PanelSlot = PanelSlot {
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
    assert_eq!(frames.pointer.handle(), Some((1, HandleKind::Move)));
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
        assert_eq!(frames.pointer.handle(), None, "{origin:?}");
        frames.update([None, beside(1.0)], &[MOVABLE]);
        assert!(frames.pointer.moves().is_empty(), "{origin:?}");
    }
}

#[test]
fn a_pull_just_above_the_bottom_edge_of_a_movable_panel_presses() {
    let mut frames = Frames::default();
    let aim = |trigger: f32| aim_at([0.0, -0.245, 0.0], [0.0, -0.245, -1.0], trigger);
    frames.update([None, aim(0.0)], &[MOVABLE]);
    assert_eq!(frames.pointer.handle(), None);
    let (events, _) = frames.update([None, aim(1.0)], &[MOVABLE]);
    assert!(events.contains(&Gesture::Press));
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
    assert_eq!(frames.pointer.handle(), Some((1, HandleKind::Resize)));

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
        assert_eq!(frames.pointer.handle(), None, "{target:?}");
        let (events, _) = frames.update([None, aim(1.0)], &[panel]);
        assert!(events.contains(&Gesture::Press), "{target:?}: {events:?}");
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
    assert_eq!(frames.pointer.handle(), None);
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
    assert_eq!(frames.pointer.handle(), Some((1, HandleKind::Move)));
    frames.update([None, handle_aim(&both, 0.0, 1.0)], &[both]);
    assert_eq!(frames.pointer.moves().len(), 1);
    assert!(frames.pointer.resizes().is_empty());

    // The ray's filter lags a jump from one handle to the other, so the other grab starts anew
    let mut frames = Frames::default();
    frames.update([None, corner_aim(&both, [0.0; 2], 0.0)], &[both]);
    assert_eq!(frames.pointer.handle(), Some((1, HandleKind::Resize)));
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
    assert_eq!(frames.pointer.handle(), None);
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
    let mut pointer = Pointer::default();
    let frames = [([0.0, 0.0], 0.0), ([0.0, 0.0], 1.0)]
        .into_iter()
        .chain((0..300).map(|_| ([0.1, -0.05], 1.0)))
        .chain([([0.1, -0.05], 0.0)]);
    for (frame, (shift, trigger)) in frames.enumerate() {
        let aim = corner_aim(&space.panels()[0], shift, trigger);
        let time = Duration::from_millis(10 * frame as u64);
        pointer.update([None, aim], time, &space.panels());
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
    assert_eq!(events, [Gesture::Press]);

    // The right ray hits the ball and presses it as it does any panel
    let mut frames = Frames::default();
    let (_, cursor) = frames.update([None, aim(0.0, 0.0, 0.0)], &panels);
    assert_eq!(cursor.map(|cursor| cursor.panel), Some(3));
    assert!(
        frames.pointer.beams()[1].is_some_and(|beam| (beam.length - 0.5).abs() < 1e-4),
        "the ray ends at the ball"
    );
    let (events, cursor) = frames.update([None, aim(0.0, 0.0, 1.0)], &panels);
    assert_eq!(events, [Gesture::Press]);
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
