// A press never becomes a move: a pull takes what the ray pointed at before the pull turned the
// controller, a press on a button clicks through a small stray, a press that turns off its button
// clicks nothing, and a handle moves its panel only past the slop.

use super::tests::{BUTTON_AT, Frames, MOVABLE, aim_at, clicks_of, handle_aim, hovered};
use crate::{SpatialMoveEventType, handle::HandleKind, pointer::DRAG_SLOP, surface_input::Gesture};

const PRESS: Gesture = Gesture::Press(cranpose::PointerButton::Primary);
const RELEASE: Gesture = Gesture::Release(cranpose::PointerButton::Primary);

// A place on the movable panel 1 cm above its bottom edge, where a button of its last row stands,
// and the middle of the handle 4 cm below that edge.
const ABOVE_EDGE: [f32; 3] = [0.0, -0.24, -1.0];

#[test]
fn a_pull_that_turns_the_ray_onto_the_handle_presses_the_button_it_pointed_at() {
    let handle = crate::carry::handle_center(&MOVABLE);
    let mut frames = Frames::default();
    frames.update([None, aim_at([0.0; 3], ABOVE_EDGE, 0.0)], &[MOVABLE]);
    // The pull tips the controller down onto the handle before the trigger counts as down
    for _ in 0..10 {
        frames.update([None, aim_at([0.0; 3], handle, 0.3)], &[MOVABLE]);
    }
    assert_eq!(
        frames.pointer.handles()[1],
        Some(hovered(1, HandleKind::Move))
    );
    let (events, _) = frames.update([None, aim_at([0.0; 3], handle, 1.0)], &[MOVABLE]);
    assert!(events.contains(&PRESS), "{events:?}");
    assert_eq!(
        frames.pointer.handles()[1],
        None,
        "the press shows no handle"
    );
    for _ in 0..30 {
        frames.update([None, aim_at([0.0; 3], handle, 1.0)], &[MOVABLE]);
        assert!(frames.pointer.moves().is_empty());
    }
    let (events, _) = frames.update([None, aim_at([0.0; 3], handle, 0.0)], &[MOVABLE]);
    assert_eq!(events.first(), Some(&RELEASE));
}

// The trigger of a press: up, pulled over two frames, held and let go.
const TRIGGER: [f32; 12] = [0.0, 0.0, 0.0, 0.8, 1.0, 1.0, 1.0, 1.0, 1.0, 1.0, 0.6, 0.0];

#[test]
fn a_press_that_strays_three_millimeters_on_its_button_clicks_once() {
    // While the trigger is down, the ray's point wanders 3 mm on the panel a meter away
    let clicks = clicks_of(TRIGGER.into_iter().enumerate().map(|(frame, trigger)| {
        let stray = if trigger > 0.5 {
            0.003 * (frame % 2) as f32
        } else {
            0.0
        };
        aim_at(
            [0.0; 3],
            [BUTTON_AT[0] + stray, BUTTON_AT[1], BUTTON_AT[2]],
            trigger,
        )
    }));
    assert_eq!(clicks, 1);
}

#[test]
fn a_press_that_turns_off_its_button_clicks_nothing() {
    // The button is 20 cm wide: after the pull the ray turns 30 cm to its right
    let clicks = clicks_of(TRIGGER.into_iter().enumerate().map(|(frame, trigger)| {
        let away = if frame > 4 && trigger > 0.5 { 0.3 } else { 0.0 };
        aim_at(
            [0.0; 3],
            [BUTTON_AT[0] + away, BUTTON_AT[1], BUTTON_AT[2]],
            trigger,
        )
    }));
    assert_eq!(clicks, 0);
}

#[test]
fn a_press_ticks_when_it_takes_and_when_it_clicks_and_not_when_it_drags() {
    let mut frames = Frames::default();
    let point = |x: f32, trigger: f32| aim_at([0.0; 3], [x, 0.0, -1.0], trigger);
    frames.update([None, point(0.0, 0.0)], &[MOVABLE]);
    frames.update([None, point(0.0, 1.0)], &[MOVABLE]);
    assert_eq!(frames.pointer.ticks(), [false, true], "the press took");
    frames.update([None, point(0.0, 1.0)], &[MOVABLE]);
    assert_eq!(frames.pointer.ticks(), [false, false]);
    frames.update([None, point(0.0, 0.0)], &[MOVABLE]);
    assert_eq!(frames.pointer.ticks(), [false, true], "the click");

    frames.update([None, point(0.0, 1.0)], &[MOVABLE]);
    for _ in 0..30 {
        frames.update([None, point(0.3, 1.0)], &[MOVABLE]);
    }
    frames.update([None, point(0.3, 0.0)], &[MOVABLE]);
    assert_eq!(
        frames.pointer.ticks(),
        [false, false],
        "a drag ends with no tick"
    );
}

#[test]
fn a_handle_moves_its_panel_only_past_the_slop() {
    let at_handle = |x: f32, trigger: f32| handle_aim(&MOVABLE, x, trigger);
    let mut frames = Frames::default();
    frames.update([None, at_handle(0.0, 0.0)], &[MOVABLE]);
    frames.update([None, at_handle(0.0, 1.0)], &[MOVABLE]);
    // A press that strays 3 mm, then lets go, moves nothing
    for frame in 0..30 {
        frames.update(
            [None, at_handle(0.003 * (frame % 2) as f32, 1.0)],
            &[MOVABLE],
        );
        assert!(frames.pointer.moves().is_empty());
    }
    frames.update([None, at_handle(0.0, 0.0)], &[MOVABLE]);
    assert!(frames.pointer.moves().is_empty(), "no move, so no end");
    assert_eq!(frames.pointer.ticks(), [false, false]);

    // A hand that moves just past the slop starts the move with the whole way from the pull
    let past = (DRAG_SLOP + 1.0) * crate::METERS_PER_DP;
    frames.update([None, at_handle(0.0, 1.0)], &[MOVABLE]);
    frames.update([None, at_handle(past, 1.0)], &[MOVABLE]);
    let kinds: Vec<_> = frames
        .pointer
        .moves()
        .iter()
        .map(|step| step.event_type)
        .collect();
    assert_eq!(
        kinds,
        [SpatialMoveEventType::Start, SpatialMoveEventType::Moving]
    );
    let moved = frames.pointer.moves()[1].pose.position[0] - MOVABLE.pose.position[0];
    assert!((moved - past).abs() < 1e-4, "{moved}");
    assert_eq!(frames.pointer.ticks(), [false, true], "the move started");
}
