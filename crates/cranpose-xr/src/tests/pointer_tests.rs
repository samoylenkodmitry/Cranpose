use super::*;
use crate::Pose;

// A panel of 800 by 500 dp a meter ahead, at the atlas's top.
const PANEL: PanelSlot = PanelSlot {
    id: 1,
    atlas: [0.0, 0.0, 800.0, 500.0],
    pose: Pose::at([0.0, 0.0, -1.0]),
};

// A panel of 200 by 100 dp behind the first one and below it in the atlas.
const BEHIND: PanelSlot = PanelSlot {
    id: 2,
    atlas: [0.0, 508.0, 200.0, 100.0],
    pose: Pose::at([0.0, 0.0, -2.0]),
};

fn aim(x: f32, select: bool) -> Option<Aim> {
    Some(Aim {
        ray: Ray {
            origin: [x, 0.0, 0.0],
            direction: [0.0, 0.0, -1.0],
        },
        select,
    })
}

#[test]
fn a_trigger_over_a_panel_moves_presses_and_releases() {
    let mut pointer = Pointer::default();
    let (events, cursor) = pointer.update([None, aim(0.0, false)], &[PANEL]);
    assert_eq!(events, [PointerEvent::Move { x: 400.0, y: 250.0 }]);
    assert_eq!(
        cursor.map(|cursor| (cursor.panel, cursor.x, cursor.y)),
        Some((1, 400.0, 250.0))
    );
    let (events, cursor) = pointer.update([None, aim(0.0, true)], &[PANEL]);
    assert_eq!(events, [PointerEvent::Press]);
    assert!(cursor.is_some_and(|cursor| cursor.pressed));
    let (events, _) = pointer.update([None, aim(0.0, false)], &[PANEL]);
    assert_eq!(events, [PointerEvent::Release]);
}

#[test]
fn the_nearest_panel_takes_the_pointer() {
    let mut pointer = Pointer::default();
    let (events, cursor) = pointer.update([None, aim(0.0, false)], &[BEHIND, PANEL]);
    assert_eq!(events, [PointerEvent::Move { x: 400.0, y: 250.0 }]);
    assert_eq!(cursor.map(|cursor| cursor.panel), Some(1));
}

#[test]
fn a_press_that_leaves_the_panel_releases_when_the_trigger_rises() {
    let mut pointer = Pointer::default();
    pointer.update([None, aim(0.0, true)], &[PANEL]);
    let (events, cursor) = pointer.update([None, aim(5.0, true)], &[PANEL]);
    assert!(events.is_empty());
    assert_eq!(cursor, None);
    let (events, _) = pointer.update([None, aim(5.0, false)], &[PANEL]);
    assert_eq!(events, [PointerEvent::Release, PointerEvent::Leave]);
}

#[test]
fn the_other_hand_takes_over_when_only_it_points_at_a_panel() {
    let mut pointer = Pointer::default();
    pointer.update([None, aim(0.0, false)], &[PANEL]);
    let (events, cursor) = pointer.update([aim(0.1, false), aim(5.0, false)], &[PANEL]);
    assert_eq!(events, [PointerEvent::Move { x: 500.0, y: 250.0 }]);
    assert!(cursor.is_some());
    let (events, _) = pointer.update([aim(0.1, false), aim(0.0, true)], &[PANEL]);
    assert_eq!(
        events,
        [
            PointerEvent::Move { x: 400.0, y: 250.0 },
            PointerEvent::Press
        ]
    );
}
