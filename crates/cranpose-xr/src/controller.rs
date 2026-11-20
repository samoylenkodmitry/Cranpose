//! What a hand's controller reports in a frame.

#[cfg(any(test, headset))]
use crate::press_threshold::pressed;
use crate::{InteractionProfile, Pose};

/// Where a space is and how it moves.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct TrackedPose {
    /// The place and turn in the app's room.
    pub pose: Pose,
    /// How fast the place changes, in meters per second, when the runtime gives it.
    pub linear_velocity: Option<[f32; 3]>,
    /// The turn about each axis, in radians per second, when the runtime gives it.
    pub angular_velocity: Option<[f32; 3]>,
}

/// A button that the finger can rest on.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Button {
    /// Whether the button is down.
    pub pressed: bool,
    /// Whether a finger rests on the button. A controller that cannot tell leaves it `false`.
    pub touched: bool,
}

/// The index finger's trigger.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Trigger {
    /// How far the trigger is pulled, from 0 to 1.
    pub value: f32,
    /// Whether the trigger is down: from a pull of 0.6 until it falls below 0.4. A profile whose
    /// trigger is a click gives a value of 0 or 1, and `pressed` follows the click.
    pub pressed: bool,
    /// Whether a finger rests on the trigger. A controller that cannot tell leaves it `false`.
    pub touched: bool,
}

/// The grip button under the other fingers.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Squeeze {
    /// How far the grip is squeezed, from 0 to 1.
    pub value: f32,
    /// Whether the grip is down: from a squeeze of 0.6 until it falls below 0.4. A profile whose
    /// grip is a click gives a value of 0 or 1, and `pressed` follows the click.
    pub pressed: bool,
}

/// The thumbstick, or the trackpad of a controller that has one.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Thumbstick {
    /// How far the stick leans to the right, from -1 to 1.
    pub x: f32,
    /// How far the stick leans forward, from -1 to 1.
    pub y: f32,
    /// Whether the stick is pressed in.
    pub pressed: bool,
    /// Whether the thumb rests on the stick. A controller that cannot tell leaves it `false`.
    pub touched: bool,
}

/// The state of one hand's controller in a frame: its trigger and grip with their values and
/// presses, the two face buttons, the thumbstick's lean and click, the menu button, and the touch
/// of each where the runtime reports it. An input that the active profile lacks stays at rest: no
/// press, no touch and a value of 0.
///
/// Jetpack XR has no controller type in its ARCore classes. The names of the buttons follow the
/// Meta Touch controllers: `primary` is A on the right hand and X on the left, and `secondary` is B
/// on the right hand and Y on the left. A controller of another maker maps its two face buttons the
/// same way.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Controller {
    /// The interaction profile that the runtime reports for the hand, or `None` while it reports
    /// none, such as while the controller sleeps.
    pub profile: Option<InteractionProfile>,
    /// The pose that the controller points with, or `None` while the runtime does not track it.
    pub aim: Option<TrackedPose>,
    /// The pose of the hand that holds the controller, or `None` while the runtime does not track
    /// it.
    pub grip: Option<TrackedPose>,
    /// The trigger under the index finger.
    pub trigger: Trigger,
    /// The grip button under the other fingers.
    pub squeeze: Squeeze,
    /// The thumbstick or trackpad.
    pub thumbstick: Thumbstick,
    /// The first face button: A on the right hand, X on the left.
    pub primary: Button,
    /// The second face button: B on the right hand, Y on the left.
    pub secondary: Button,
    /// Whether the menu button is down. Only the left Touch controller has one.
    pub menu: bool,
    /// Whether the thumb rests on the thumbrest, which a controller has beside its thumbstick.
    pub thumbrest_touched: bool,
}

#[cfg(any(test, headset))]
impl Controller {
    // Sets which of the trigger and the grip count as pressed, from their values in this frame and
    // their presses in `before`, the frame before.
    pub(crate) fn settle_presses(&mut self, before: &Self) {
        self.trigger.pressed = pressed(self.trigger.value, before.trigger.pressed.into());
        self.squeeze.pressed = pressed(self.squeeze.value, before.squeeze.pressed.into());
    }
}

#[cfg(test)]
#[path = "tests/controller_tests.rs"]
mod tests;
