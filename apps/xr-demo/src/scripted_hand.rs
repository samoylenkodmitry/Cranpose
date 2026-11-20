// The demo's scripted hand: a hand that the backend plays in place of the cameras', which pinches the
// counter's button in a cycle. It shows the hand pointer in the Android XR emulator, whose hand
// simulation does not reach the runtime. `XR_DEMO_SCRIPTED_HAND=right` or `left` builds it. The
// script goes through the path of a tracked hand: the frame's `HandState`, the pointer's choice
// of input, the ray, the press and the button.

use std::time::Duration;

use cranpose_xr::{Hand, METERS_PER_DP, PinchCycle, Pose, ScriptedHand};

use crate::type_panel;

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
