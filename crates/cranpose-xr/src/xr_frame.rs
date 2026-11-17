//! The headset's state in one frame.

use std::time::Duration;

use crate::{Controller, Hand, Pose, Recenter, RenderViewpoint};

/// The headset's state for one frame: where the head and the eyes are, what the controllers
/// report and which events the runtime sent. [`SpatialFrameEffect`](crate::SpatialFrameEffect)
/// gives an app one for each frame.
///
/// Every pose is in the app's room, the OpenXR LOCAL space, whose origin lies where the head was
/// when the app started, until a [`Recenter`] moves it. The values are predictions for the time
/// at which the frame's pictures reach the display.
#[derive(Clone, Debug, Default, PartialEq)]
#[non_exhaustive]
pub struct XrFrame {
    /// The time at which the frame's pictures reach the display, as the time since the start of
    /// the platform's monotonic clock. On Android that is `CLOCK_MONOTONIC`, which `Instant::now`
    /// and `clock_gettime` read. A runtime without `XR_KHR_convert_timespec_time` gives its own
    /// time, which equals the monotonic clock on Android runtimes.
    pub predicted_display_time: Duration,
    /// The time between two frames at the display's refresh rate.
    pub period: Duration,
    /// Whether the runtime sends this app the headset's input. It does not while a system menu
    /// covers the app. The controllers stay at rest then.
    pub focused: bool,
    /// The pose of the head, or `None` while the runtime does not track it. Jetpack XR's
    /// `ArDevice` state `devicePose`.
    pub device_pose: Option<Pose>,
    /// The left eye's viewpoint, or `None` while the runtime does not track the head.
    pub left_eye: Option<RenderViewpoint>,
    /// The right eye's viewpoint, or `None` while the runtime does not track the head.
    pub right_eye: Option<RenderViewpoint>,
    /// The left hand's controller.
    pub left_controller: Controller,
    /// The right hand's controller.
    pub right_controller: Controller,
    /// The runtime's change of the room's origin that it announced since the last frame.
    pub recenter: Option<Recenter>,
}

impl XrFrame {
    /// The controller of `hand`.
    pub const fn controller(&self, hand: Hand) -> &Controller {
        match hand {
            Hand::Left => &self.left_controller,
            Hand::Right => &self.right_controller,
        }
    }
}
