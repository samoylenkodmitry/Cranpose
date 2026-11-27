use cranpose::PointerButton;

use super::*;
use crate::{
    carry::handle_center,
    pointer_aim::pointer_aim,
    pointer_settings::PointerChoice,
    pointers::Pointers,
    space::{Mobility, PanelKind, PanelSlot, Ride, Shape},
    surface_input::Gesture as Shown,
    volume_pick::PickMeshes as Meshes,
};

fn millis(millis: u64) -> Duration {
    Duration::from_millis(millis)
}

// The frame at `time` after the first one, with the scripts played into it.
fn frame_at(scripts: &[&ScriptedController], time: u64) -> XrFrame {
    let mut frame = XrFrame {
        predicted_display_time: millis(time),
        ..XrFrame::default()
    };
    for script in scripts {
        play(script, &mut frame);
    }
    frame
}

const PULLED: ScriptedButtons = ScriptedButtons {
    trigger: 1.0,
    squeeze: 0.0,
    primary: false,
    secondary: false,
    thumbstick: [0.0; 2],
    thumbstick_click: false,
    menu: false,
};

fn walker() -> ScriptedController {
    ScriptedController::new(Hand::Right, [0.0; 3], [0.0, 0.0, -1.0])
        .holding(millis(1000), ScriptedButtons::default())
        .moving_to(millis(1000), ([0.2, 0.0, 0.0], [0.2, 0.0, -1.0]), PULLED)
        .holding(millis(500), ScriptedButtons::default())
}

#[test]
fn a_step_moves_the_controller_and_its_aim_in_a_straight_line_and_holds_its_buttons() {
    let script = walker();
    // The script starts at its first frame, at 0 ms
    frame_at(&[&script], 0);
    let at = |time| frame_at(&[&script], time).right_controller;
    let rest = at(500);
    assert_eq!(rest.aim.expect("an aim").pose.position, [0.0; 3]);
    assert_eq!(rest.trigger.value, 0.0);
    let middle = at(1500);
    let pose = middle.aim.expect("an aim").pose;
    assert!((pose.position[0] - 0.1).abs() < 1e-4, "{pose:?}");
    assert_eq!(middle.trigger.value, 1.0);
    assert!(middle.trigger.pressed);
    assert!(middle.trigger.touched);
    // The ray keeps pointing straight ahead, because the place that it points at moves with the
    // controller
    let ray = crate::Ray::of_aim(&pose);
    assert!(ray.direction[2] < -0.9999, "{ray:?}");
}

#[test]
fn a_script_holds_the_end_of_its_last_step_and_a_repeating_one_starts_again() {
    let once = walker();
    frame_at(&[&once], 0);
    let end = frame_at(&[&once], 60_000).right_controller;
    assert_eq!(end.aim.expect("an aim").pose.position, [0.2, 0.0, 0.0]);
    assert_eq!(
        end.trigger.value, 0.0,
        "the last step's buttons are at rest"
    );

    let again = walker().repeating();
    frame_at(&[&again], 0);
    // 2500 ms is a whole cycle and 500 ms into the next one
    let start = frame_at(&[&again], 2500 + 500).right_controller;
    assert_eq!(start.aim.expect("an aim").pose.position, [0.0; 3]);
    let moving = frame_at(&[&again], 2500 + 1500).right_controller;
    assert_eq!(moving.trigger.value, 1.0);
}

#[test]
fn a_script_fills_its_own_controller_as_a_touch_controller_and_leaves_the_other() {
    let left = ScriptedController::new(Hand::Left, [-0.2, 0.0, 0.0], [-0.2, 0.0, -1.0]).holding(
        millis(1000),
        ScriptedButtons {
            primary: true,
            squeeze: 0.9,
            thumbstick: [0.5, -1.0],
            thumbstick_click: true,
            ..ScriptedButtons::default()
        },
    );
    frame_at(&[&left], 0);
    let frame = frame_at(&[&left], 100);
    assert_eq!(frame.right_controller, Controller::default());
    let controller = frame.left_controller;
    assert_eq!(controller.profile, Some(InteractionProfile::MetaTouchPlus));
    assert!(controller.primary.pressed && controller.primary.touched);
    assert!(!controller.secondary.pressed);
    assert!(controller.squeeze.pressed);
    assert_eq!(
        (controller.thumbstick.x, controller.thumbstick.y),
        (0.5, -1.0)
    );
    assert!(controller.thumbstick.pressed && controller.thumbstick.touched);
    assert_eq!(controller.grip, controller.aim);
}

// A panel of 400 by 300 dp a meter ahead.
const PANEL: PanelSlot = PanelSlot {
    id: 1,
    kind: PanelKind::Content,
    atlas: [0.0, 0.0, 400.0, 300.0],
    pose: Pose::at([0.0, 0.0, -1.0]),
    mobility: Mobility::Movable,
    shape: Shape::Flat,
    resize: None,
    room: [400.0, 300.0],
    priority: 0,
    ride: Ride::Free,
    tracked: None,
};

#[test]
fn two_scripted_controllers_press_scroll_and_carry_through_the_pointer() {
    // The left controller takes the panel's handle, and carries the panel 20 cm to the right. The
    // right controller points at the panel's middle, presses its first face button and leans its
    // thumbstick back, all while the panel moves.
    let handle = ScriptedController::handle_of(&PANEL.pose, PANEL.size());
    assert_eq!(handle, handle_center(&PANEL));
    let left = ScriptedController::new(Hand::Left, [-0.3, 0.0, 0.0], handle)
        .holding(millis(500), ScriptedButtons::default())
        .holding(millis(300), PULLED)
        .moving_to(
            millis(1000),
            ([-0.1, 0.0, 0.0], [handle[0] + 0.2, handle[1], handle[2]]),
            PULLED,
        )
        .holding(millis(300), ScriptedButtons::default());
    let middle = [0.0, 0.0, -1.0];
    let right = ScriptedController::new(Hand::Right, [0.3, 0.0, 0.0], middle)
        .holding(millis(1200), ScriptedButtons::default())
        .holding(
            millis(600),
            ScriptedButtons {
                primary: true,
                thumbstick: [0.0, -1.0],
                ..ScriptedButtons::default()
            },
        );
    let mut pointers = Pointers::default();
    let mut gestures = Vec::new();
    let mut moves = Vec::new();
    for frame in 0..220_u64 {
        let time = millis(10 * frame);
        let mut xr = XrFrame {
            predicted_display_time: time,
            ..XrFrame::default()
        };
        play(&left, &mut xr);
        play(&right, &mut xr);
        let aims = [
            pointer_aim(&xr.left_controller, None),
            pointer_aim(&xr.right_controller, None),
        ];
        pointers.update(
            aims,
            time,
            (&[PANEL], &Meshes::default()),
            PointerChoice::default(),
        );
        gestures.extend(pointers.atlas().iter().map(|event| event.gesture));
        moves.extend(pointers.moves().iter().map(|step| step.pose.position[0]));
    }
    assert!(
        gestures.contains(&Shown::Press(PointerButton::Secondary)),
        "the right controller's first face button is a right click: {gestures:?}"
    );
    assert!(
        gestures
            .iter()
            .any(|gesture| matches!(gesture, Shown::Scroll { y, .. } if *y < 0.0)),
        "its thumbstick leaned back scrolls: {gestures:?}"
    );
    let end = moves
        .last()
        .copied()
        .expect("the left controller carries the panel");
    assert!(
        (end - 0.2).abs() < 0.02,
        "the panel moved to the right by {end}: {moves:?}"
    );
}

#[test]
fn an_untracked_step_gives_a_controller_with_no_pose_and_the_next_step_starts_where_it_was() {
    let script = ScriptedController::new(Hand::Left, [0.1, 0.0, 0.0], [0.1, 0.0, -1.0])
        .holding(millis(100), PULLED)
        .untracked(millis(100))
        .holding(millis(100), ScriptedButtons::default());
    frame_at(&[&script], 0);
    let at = |time| frame_at(&[&script], time).left_controller;
    assert!(at(50).aim.is_some() && at(50).trigger.pressed);
    let lost = at(150);
    assert_eq!(lost, Controller::default());
    assert!(lost.profile.is_none() && lost.grip.is_none());
    let back = at(250).aim.expect("an aim").pose.position;
    assert_eq!(back, [0.1, 0.0, 0.0]);
    // A script that ends with such a step keeps the controller missing
    let missing =
        ScriptedController::new(Hand::Right, [0.0; 3], [0.0, 0.0, -1.0]).untracked(millis(1));
    frame_at(&[&missing], 0);
    assert_eq!(
        frame_at(&[&missing], 10_000).right_controller,
        Controller::default()
    );
}
