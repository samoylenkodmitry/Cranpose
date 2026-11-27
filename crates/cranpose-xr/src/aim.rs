//! What a controller or a tracked hand does in a frame, as the pointer reads it.

use crate::{InputSource, Pose, Ray, direct_touch::Fingertip};

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
// stick stays at 0. A tracked hand also has the tip of its index finger, which touches the panels.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct Aim {
    pub(crate) ray: Ray,
    pub(crate) pose: Pose,
    pub(crate) trigger: f32,
    pub(crate) stick: [f32; 2],
    pub(crate) buttons: Buttons,
    pub(crate) source: InputSource,
    pub(crate) tip: Option<Fingertip>,
}

impl Aim {
    // The aim with its ray starting at the point of its line nearest `start`, such as the tip of
    // the controller's model. The ray keeps its line and its direction, so it meets the panels
    // where it met them before. Only its visible start moves. A tracked hand keeps its ray.
    #[cfg_attr(
        not(any(test, headset)),
        expect(dead_code, reason = "the headset backend calls it")
    )]
    pub(crate) fn starting_near(self, start: [f32; 3]) -> Self {
        if self.source != InputSource::Controller {
            return self;
        }
        let Ray { origin, direction } = self.ray;
        let along: f32 = (0..3)
            .map(|axis| (start[axis] - origin[axis]) * direction[axis])
            .sum();
        Self {
            ray: Ray {
                origin: [0, 1, 2].map(|axis| origin[axis] + along * direction[axis]),
                direction,
            },
            ..self
        }
    }
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
            tip: None,
        }
    }
}

#[cfg(test)]
#[path = "tests/aim_tests.rs"]
mod tests;
