// The demo's scripted hand: a hand that the backend plays in place of the cameras', which pinches the
// counter's button in a cycle. It shows the hand pointer in the Android XR emulator, whose hand
// simulation does not reach the runtime. `XR_DEMO_SCRIPTED_HAND=right` or `left` builds it. The
// script goes through the path of a tracked hand: the frame's `HandState`, the pointer's choice
// of input, the ray, the press and the button.
//
// The demo's scripted controllers: `XR_DEMO_SCRIPTED_CONTROLLERS=two` plays both controllers at
// once, through the path of the runtime's controllers: the frame's `Controller` of each hand, the
// pointer of each hand, the press, the carry and the scroll. The two controllers carry the counter
// panel and the panel of the readout at the same time. Then the right controller clicks the pad of
// the readout panel with its first face button and its thumbstick's click, and scrolls the list
// below it, while the left controller presses the counter's button.

use std::time::Duration;

use cranpose_xr::{
    Hand, METERS_PER_DP, PinchCycle, Pose, ScriptedButtons, ScriptedController, ScriptedHand,
};

use crate::{presses_panel, type_panel};

// The counter panel is 800 by 800 dp a meter ahead of the head's start, and its button lies 160 dp
// from the panel's left edge and 230 dp from its top.
const PANEL_DP: f32 = 800.0;
const BUTTON_DP: [f32; 2] = [160.0, 230.0];
const PANEL_DISTANCE: f32 = 1.0;
// Where the hand's aim pose stands, to the right of the head's start, below it and ahead of it.
const HAND_AT: [f32; 3] = [0.15, -0.25, -0.3];

// The scripted hand of this build, or `None` for a build that did not ask for it. A build that
// names a text has the hand press the text field and then the keys that type the text. Otherwise the
// hand presses the counter's button.
pub(crate) fn from_build(setting: Option<&str>, text: Option<&str>) -> Option<ScriptedHand> {
    let hand = match setting {
        Some("left") => Hand::Left,
        Some(_) => Hand::Right,
        None if text.is_some() => Hand::Right,
        None => return None,
    };
    if let Some(text) = text {
        return Some(typing(hand, text));
    }
    let button = [
        (BUTTON_DP[0] - PANEL_DP / 2.0) * METERS_PER_DP,
        (PANEL_DP / 2.0 - BUTTON_DP[1]) * METERS_PER_DP,
        -PANEL_DISTANCE,
    ];
    Some(ScriptedHand::pointing(
        hand,
        HAND_AT,
        button,
        PinchCycle::default(),
    ))
}

// The hand that presses the field of the type panel and then types `text`. A press takes a second.
fn typing(hand: Hand, text: &str) -> ScriptedHand {
    let subspace = Pose::at([0.0, 0.0, -PANEL_DISTANCE]);
    ScriptedHand::typing(
        hand,
        HAND_AT,
        subspace.then(&type_panel::modifier().pose()),
        type_panel::SIZE,
        [type_panel::SIZE[0] / 2.0, type_panel::SIZE[1] / 2.0],
        text,
        PinchCycle {
            rest: Duration::from_millis(500),
            closing: Duration::from_millis(150),
            held: Duration::from_millis(250),
            opening: Duration::from_millis(150),
        },
    )
}

// How long each part of the controllers' script lasts.
const SECOND: Duration = Duration::from_secs(1);
const HALF: Duration = Duration::from_millis(500);

// The buttons of a controller that rests, and of one that pulls its trigger.
const REST: ScriptedButtons = ScriptedButtons {
    trigger: 0.0,
    squeeze: 0.0,
    primary: false,
    secondary: false,
    thumbstick: [0.0; 2],
    thumbstick_click: false,
    menu: false,
};
const PULL: ScriptedButtons = ScriptedButtons {
    trigger: 1.0,
    ..REST
};

// The scripted controllers of this build, or none for a build that did not ask for them.
pub(crate) fn controllers_from_build(setting: Option<&str>) -> Vec<ScriptedController> {
    match setting {
        Some(_) => two_controllers(),
        None => Vec::new(),
    }
}

// The room's place of the point `at` dp from the top left corner of the readout panel, after the
// panel moved by `shift` meters.
fn on_presses_panel(at: [f32; 2], shift: [f32; 3]) -> [f32; 3] {
    let panel = Pose::at([0.0, 0.0, -PANEL_DISTANCE]).then(&presses_panel::modifier().pose());
    let place = panel.then(&Pose::at(shift));
    let size = presses_panel::SIZE;
    let x = (at[0] - size[0] / 2.0) * METERS_PER_DP;
    let y = (size[1] / 2.0 - at[1]) * METERS_PER_DP;
    place.then(&Pose::at([x, y, 0.0])).position
}

// Where a controller that stands at `position` points when its ray meets `target`. The script moves
// a controller and its target together, so the ray keeps its turn and a carried panel keeps its
// turn.
fn shifted(target: [f32; 3], by: [f32; 3]) -> [f32; 3] {
    [0, 1, 2].map(|axis| target[axis] + by[axis])
}

// The left controller carries the counter panel and the right one the readout panel, each by its
// handle and both at the same time, 12 cm up and to the side and back. Then the right controller
// presses the pad with its first face button twice and its thumbstick's click once, and scrolls the
// list back and forth, while the left controller presses the counter's button three times. The
// script repeats.
fn two_controllers() -> Vec<ScriptedController> {
    let counter = Pose::at([0.0, 0.0, -PANEL_DISTANCE]);
    let counter_handle = ScriptedController::handle_of(&counter, [PANEL_DP, PANEL_DP]);
    let readout_panel = counter.then(&presses_panel::modifier().pose());
    let readout_handle = ScriptedController::handle_of(&readout_panel, presses_panel::SIZE);
    let (left_from, right_from) = ([-0.25, -0.3, -0.3], [0.25, -0.3, -0.3]);
    let carried = |hand: Hand, from: [f32; 3], handle: [f32; 3], by: [f32; 3]| {
        let lifted = (shifted(from, by), shifted(handle, by));
        ScriptedController::new(hand, from, handle)
            .holding(2 * SECOND, REST)
            .holding(HALF, PULL)
            .moving_to(2 * SECOND, lifted, PULL)
            .moving_to(2 * SECOND, (from, handle), PULL)
            .holding(HALF, REST)
    };
    let left_pad = on_counter_button();
    let pad = on_presses_panel(presses_panel::PAD_AT, [0.0; 3]);
    let list = on_presses_panel(presses_panel::LIST_AT, [0.0; 3]);
    let first_face = ScriptedButtons {
        primary: true,
        ..REST
    };
    let stick_click = ScriptedButtons {
        thumbstick_click: true,
        ..REST
    };
    let lean = |y: f32| ScriptedButtons {
        thumbstick: [0.0, y],
        ..REST
    };
    let right = carried(Hand::Right, right_from, readout_handle, [0.1, 0.12, 0.0])
        .moving_to(SECOND, (right_from, pad), REST)
        .holding(HALF, first_face)
        .holding(HALF, REST)
        .holding(HALF, first_face)
        .holding(HALF, REST)
        .holding(HALF, stick_click)
        .moving_to(SECOND, (right_from, list), REST)
        .holding(2 * SECOND, lean(-0.3))
        .holding(2 * SECOND, lean(0.3))
        .holding(HALF, REST)
        .repeating();
    let left = carried(Hand::Left, left_from, counter_handle, [-0.15, 0.12, 0.0])
        .moving_to(SECOND, (left_from, left_pad), REST)
        .holding(HALF, REST)
        .holding(HALF, PULL)
        .holding(HALF, REST)
        .holding(HALF, PULL)
        .holding(HALF, REST)
        .holding(HALF, PULL)
        .holding(HALF, REST)
        .holding(9 * HALF, REST)
        .repeating();
    vec![left, right]
}

// The room's place of the counter's button.
fn on_counter_button() -> [f32; 3] {
    [
        (BUTTON_DP[0] - PANEL_DP / 2.0) * METERS_PER_DP,
        (PANEL_DP / 2.0 - BUTTON_DP[1]) * METERS_PER_DP,
        -PANEL_DISTANCE,
    ]
}
