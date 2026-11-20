//! Where a tracked hand points and how it pinches.

use crate::{Pinch, Pose, Ray};

/// Where a tracked hand points and how it pinches. A hand with this works as a pointer:
/// [`ray`](Self::ray) is the ray of a controller, and a pinch of the index finger is its trigger.
/// An app that declared `Use::hand_tracking()` gets that pointer for a hand that holds no
/// controller, as [`InputSource::Hands`](crate::InputSource::Hands). It comes from
/// `XR_FB_hand_tracking_aim`, which the Meta Quest offers. Jetpack XR has no such API.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct HandAim {
    /// The pose that the hand points with, in the app's room. It points along its -z, as the aim
    /// pose of a controller does.
    pub pose: Pose,
    /// The index finger's pinch.
    pub index: Pinch,
    /// The middle finger's pinch.
    pub middle: Pinch,
    /// The ring finger's pinch.
    pub ring: Pinch,
    /// The little finger's pinch.
    pub little: Pinch,
    /// Whether the user makes the gesture that opens the system menu.
    pub system_gesture: bool,
    /// Whether the menu gesture is pressed.
    pub menu_pressed: bool,
    /// Whether the system counts this hand as the user's dominant one.
    pub dominant: bool,
}

impl HandAim {
    /// The ray along which the hand points.
    pub fn ray(&self) -> Ray {
        Ray::of_aim(&self.pose)
    }
}
