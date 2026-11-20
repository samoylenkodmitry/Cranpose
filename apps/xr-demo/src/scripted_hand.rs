// The demo's scripted hand: a hand that the backend plays in place of the cameras', which pinches the
// counter's button in a cycle. It shows the hand pointer in the Android XR emulator, whose hand
// simulation does not reach the runtime. `XR_DEMO_SCRIPTED_HAND=right` or `left` builds it. The
// script goes through the path of a tracked hand: the frame's `HandState`, the pointer's choice
// of input, the ray, the press and the button.

use cranpose_xr::{Hand, METERS_PER_DP, PinchCycle, ScriptedHand};

// The counter panel is 800 by 800 dp a meter ahead of the head's start, and its button lies 160 dp
// from the panel's left edge and 230 dp from its top.
const PANEL_DP: f32 = 800.0;
const BUTTON_DP: [f32; 2] = [160.0, 230.0];
const PANEL_DISTANCE: f32 = 1.0;
// Where the hand's aim pose stands, to the right of the head's start, below it and ahead of it.
const HAND_AT: [f32; 3] = [0.15, -0.25, -0.3];

// The scripted hand of this build, or `None` for a build that did not ask for it.
pub(crate) fn from_build(setting: Option<&str>) -> Option<ScriptedHand> {
    let hand = match setting? {
        "left" => Hand::Left,
        _ => Hand::Right,
    };
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
