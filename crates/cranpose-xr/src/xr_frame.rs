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
    /// the platform's monotonic clock. On Android and Linux that is `CLOCK_MONOTONIC`, which
    /// `Instant::now` and `clock_gettime` read. On Windows it is the performance counter. A
    /// runtime without `XR_KHR_convert_timespec_time` (Windows:
    /// `XR_KHR_win32_convert_performance_counter_time`) gives its own time, which equals the
    /// monotonic clock on Android runtimes.
    pub predicted_display_time: Duration,
    /// The time at which the backend read the clock for this frame, on the same clock as
    /// [`predicted_display_time`](Self::predicted_display_time). The difference of the two is how
    /// far ahead the frame predicts. An app converts between the runtime's times and its own
    /// readings of `CLOCK_MONOTONIC` with it.
    pub now: Duration,
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
    /// The rate in hertz at which the display runs in this frame, from the frame's
    /// [`period`](Self::period), or `None` while the period is unknown. The rates that the
    /// runtime offers are in [`HeadsetProperties`](crate::HeadsetProperties).
    pub fn refresh_rate(&self) -> Option<f32> {
        let seconds = self.period.as_secs_f32();
        (seconds > 0.0).then(|| 1.0 / seconds)
    }

    /// The distance in meters between the two eyes' places, or `None` while the runtime does not
    /// track the head.
    pub fn interpupillary_distance(&self) -> Option<f32> {
        let (left, right) = (self.left_eye?, self.right_eye?);
        let [x, y, z] = [0, 1, 2].map(|axis| left.pose.position[axis] - right.pose.position[axis]);
        Some((x * x + y * y + z * z).sqrt())
    }

    /// The controller of `hand`.
    pub const fn controller(&self, hand: Hand) -> &Controller {
        match hand {
            Hand::Left => &self.left_controller,
            Hand::Right => &self.right_controller,
        }
    }
}
