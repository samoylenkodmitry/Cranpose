//! What a controller or a tracked hand does in a frame, as the pointer reads it.

use crate::{InputSource, Pose, Ray};

// The inputs of a controller besides its trigger and its thumbstick's lean. A tracked hand has
// none, so they stay at rest.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub(crate) struct Buttons {
    pub(crate) squeeze: f32,
    pub(crate) primary: bool,
    pub(crate) secondary: bool,
    pub(crate) stick_click: bool,
    pub(crate) menu: bool,
}

// What a controller or a tracked hand does in a frame: where it points, how far its trigger is
// pulled or its index finger pinches from 0 to 1, how far its thumbstick leans to the right and
// forward from -1 to 1, and which other buttons it has down. A hand has no thumbstick, so its
// stick stays at 0.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct Aim {
    pub(crate) ray: Ray,
    pub(crate) pose: Pose,
    pub(crate) trigger: f32,
    pub(crate) stick: [f32; 2],
    pub(crate) buttons: Buttons,
    pub(crate) source: InputSource,
}

#[cfg(test)]
impl Aim {
    // The aim of a controller that points along `ray` and pulls its trigger by `trigger`, with its
    // stick at rest and no other button down.
    pub(crate) fn pulling(ray: Ray, pose: Pose, trigger: f32) -> Self {
        Self {
            ray,
            pose,
            trigger,
            stick: [0.0; 2],
            buttons: Buttons::default(),
            source: InputSource::Controller,
        }
    }
}
