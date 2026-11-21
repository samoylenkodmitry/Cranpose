use cranpose::PointerButton;

use super::*;
use crate::{
    Controller, PointerMapping, Ray,
    pointer_aim::pointer_aim,
    pointers::Pointers,
    space::{Mobility, PanelKind, PanelSlot, Ride, Shape},
    surface_input::Gesture,
    volume_pick::PickMeshes,
};

fn millis(millis: u64) -> Duration {
    Duration::from_millis(millis)
}

fn script() -> ScriptedHand {
    ScriptedHand::pointing(
        Hand::Right,
        [0.15, -0.25, -0.3],
        [-0.24, 0.17, -1.0],
        PinchCycle::default(),
    )
}

fn strength(script: &ScriptedHand, at: Duration) -> f32 {
    script
        .state_at(at)
        .aim
        .expect("a scripted hand has an aim")
        .index
        .strength
}

#[test]
fn the_pinch_rests_closes_holds_opens_and_repeats() {
    let script = script();
    let expected = [
        (0, 0.0),
        (1999, 0.0),
        (2100, 0.5),
        (2200, 1.0),
        (2500, 1.0),
        (2800, 1.0),
        (2900, 0.5),
        (3000, 0.0),
        (3000 + 2100, 0.5),
        (3000 * 5000 + 2500, 1.0),
    ];
    for (at, want) in expected {
        let got = strength(&script, millis(at));
        assert!((got - want).abs() < 0.01, "at {at} ms: {got}, not {want}");
    }
}

#[test]
fn a_cycle_of_no_length_never_pinches() {
    let cycle = PinchCycle {
        rest: Duration::ZERO,
        closing: Duration::ZERO,
        held: Duration::ZERO,
        opening: Duration::ZERO,
    };
    let script = ScriptedHand::pointing(Hand::Left, [0.0; 3], [0.0, 0.0, -1.0], cycle);
    assert_eq!(strength(&script, millis(1234)), 0.0);
}

#[test]
fn the_aim_pose_points_at_the_place() {
    let aim = script().state_at(Duration::ZERO).aim.expect("an aim");
    assert_eq!(aim.pose.position, [0.15, -0.25, -0.3]);
    let panel = Pose::at([0.0, 0.0, -1.0]);
    let hit = aim
        .ray()
        .hit_panel(&panel, [1.0, 1.0])
        .expect("the aim hits the panel");
    assert!((hit.u - 0.26).abs() < 1e-4, "{hit:?}");
    assert!((hit.v - 0.33).abs() < 1e-4, "{hit:?}");
    // The pose is level: its x axis lies in the horizontal plane
    let x = aim.pose.rotate([1.0, 0.0, 0.0]);
    assert!(x[1].abs() < 1e-5, "{x:?}");
}

#[test]
fn a_pose_straight_ahead_has_no_turn() {
    let script = ScriptedHand::pointing(
        Hand::Right,
        [0.0; 3],
        [0.0, 0.0, -1.0],
        PinchCycle::default(),
    );
    let ray = Ray::of_aim(&script.state_at(Duration::ZERO).aim.expect("an aim").pose);
    assert!(ray.direction[2] < -0.9999, "{ray:?}");
}

#[test]
fn the_script_fills_its_own_hand_of_the_frame_as_a_tracked_hand() {
    // The script starts at its first frame
    let script = script();
    play(&script, &mut XrFrame::default());
    let mut frame = XrFrame {
        predicted_display_time: millis(2300),
        ..XrFrame::default()
    };
    play(&script, &mut frame);
    assert_eq!(frame.right_hand.tracking_state, TrackingState::Tracking);
    let aim = frame.right_hand.aim.expect("an aim");
    assert_eq!(aim.index.strength, 1.0);
    assert!(aim.index.pinching);
    assert_eq!(frame.left_hand, HandState::default());
    let left = ScriptedHand::pointing(
        Hand::Left,
        [0.0; 3],
        [0.0, 0.0, -1.0],
        PinchCycle::default(),
    );
    let mut frame = XrFrame::default();
    play(&left, &mut frame);
    assert_eq!(frame.left_hand.tracking_state, TrackingState::Tracking);
    assert_eq!(frame.right_hand, HandState::default());
}

// A panel of 800 by 800 dp a meter ahead.
const PANEL: PanelSlot = PanelSlot {
    id: 1,
    kind: PanelKind::Content,
    atlas: [0.0, 0.0, 800.0, 800.0],
    pose: Pose::at([0.0, 0.0, -1.0]),
    mobility: Mobility::Fixed,
    shape: Shape::Flat,
    resize: None,
    room: [800.0, 800.0],
    priority: 0,
    ride: Ride::Free,
};

#[test]
fn a_scripted_pinch_moves_to_the_place_presses_once_and_releases_once() {
    // The place is 160 dp from the panel's left edge and 230 dp from its top
    let script = ScriptedHand::pointing(
        Hand::Right,
        [0.15, -0.25, -0.3],
        [-0.24, 0.17, -1.0],
        PinchCycle::default(),
    );
    let mut pointer = Pointers::default();
    let mut gestures = Vec::new();
    for frame in 0..400_u64 {
        let time = millis(10 * frame);
        let mut xr = XrFrame {
            predicted_display_time: time,
            ..XrFrame::default()
        };
        play(&script, &mut xr);
        let aim = pointer_aim(&Controller::default(), Some(&xr.right_hand));
        pointer.update(
            [None, aim],
            time,
            (&[PANEL], &PickMeshes::default()),
            (PointerMapping::default(), true),
        );
        gestures.extend(pointer.atlas().iter().map(|event| (frame, event.gesture)));
    }
    let moves: Vec<_> = gestures
        .iter()
        .filter(|(_, gesture)| matches!(gesture, Gesture::Move { .. }))
        .collect();
    assert_eq!(moves.len(), 1, "{gestures:?}");
    let Gesture::Move { x, y } = moves[0].1 else {
        panic!("a move");
    };
    assert!(
        (x - 160.0).abs() < 1.0 && (y - 230.0).abs() < 1.0,
        "{x} {y}"
    );
    let presses: Vec<u64> = gestures
        .iter()
        .filter(|(_, gesture)| *gesture == Gesture::Press(PointerButton::Primary))
        .map(|(frame, _)| *frame)
        .collect();
    let releases: Vec<u64> = gestures
        .iter()
        .filter(|(_, gesture)| *gesture == Gesture::Release(PointerButton::Primary))
        .map(|(frame, _)| *frame)
        .collect();
    // The strength passes 0.6 at 2120 ms and falls below 0.4 at 2920 ms
    assert_eq!(presses, [213], "{gestures:?}");
    assert_eq!(releases, [293], "{gestures:?}");
}
