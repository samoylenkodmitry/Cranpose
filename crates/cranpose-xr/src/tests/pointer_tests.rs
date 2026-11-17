use super::*;
use crate::{Pose, space::Surface};

// A panel of 800 by 500 dp a meter ahead, at the atlas's top.
const PANEL: PanelSlot = PanelSlot {
    id: 1,
    atlas: [0.0, 0.0, 800.0, 500.0],
    pose: Pose::at([0.0, 0.0, -1.0]),
    mobility: Mobility::Fixed,
    surface: Surface::Flat,
};

// A panel of 200 by 100 dp behind the first one and below it in the atlas.
const BEHIND: PanelSlot = PanelSlot {
    id: 2,
    atlas: [0.0, 508.0, 200.0, 100.0],
    pose: Pose::at([0.0, 0.0, -2.0]),
    mobility: Mobility::Fixed,
    surface: Surface::Flat,
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
        surface: Surface::Flat,
        ..PANEL
    };
    // A controller at `x` that points at the handle under the panel
    let at_handle = |x: f32, trigger: f32| {
        let (handle, _) = crate::carry::handle_pose(&movable);
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
    };
    let mut frames = Frames::default();
    let (events, _) = frames.update([None, at_handle(0.0, 0.0)], &[movable]);
    assert!(events.is_empty());
    assert_eq!(frames.pointer.handle(), Some(1));
    frames.update([None, at_handle(0.0, 1.0)], &[movable]);
    let (events, cursor) = frames.update([None, at_handle(0.1, 1.0)], &[movable]);
    assert!(events.is_empty());
    assert_eq!(cursor, None);
    let (panel, pose) = frames.pointer.moved().expect("the panel moves");
    assert_eq!(panel, 1);
    assert!((pose.position[0] - 0.1).abs() < 1e-4);
    frames.update([None, at_handle(0.1, 0.0)], &[movable]);
    assert_eq!(frames.pointer.moved(), None);
}

#[test]
fn a_ray_moves_the_pointer_along_the_curve_of_a_curved_panel() {
    // A meter-wide circle around the controller: 20 degrees are 349 mm of arc, where a flat
    // panel would put the pointer 364 mm from the middle
    let curved = PanelSlot {
        surface: Surface::Curved { radius: 1.0 },
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
