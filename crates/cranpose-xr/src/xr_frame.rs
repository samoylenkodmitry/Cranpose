//! The headset's state in one frame.

use std::time::Duration;

use crate::{
    BodyState, Controller, Eyes, FaceState, Hand, HandState, MetaFaceState, Pose, Recenter,
    RenderViewpoint, TrackingCapabilities,
};

/// The headset's state for one frame: where the head and the eyes are, what the controllers
/// report, which events the runtime sent and, for the tracking that the app asked for, the user's
/// hands, body, face and eyes. [`SpatialFrameEffect`](crate::SpatialFrameEffect)
/// gives an app one for each frame.
///
/// Every pose is in the app's room, the OpenXR LOCAL space, whose origin lies where the head was
/// when the app started, until a [`Recenter`] moves it. The values are predictions for the time
/// at which the frame's pictures reach the display.
///
/// The hands, the body, the face and the eyes stay at rest, with a [`TrackingState`] of `Stopped`,
/// until the app asks for them in its build with `Use::hand_tracking()`, `Use::body_tracking()`,
/// `Use::face_tracking()` or `Use::eye_tracking()` of `cranpose::capabilities`, and the user grants
/// their permissions. An app that asks for none pays nothing in a frame. Every field is a fixed
/// array, so the frame loop allocates nothing for them.
///
/// [`TrackingState`]: crate::TrackingState
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
    /// How fast the head moves, in meters per second along the axes of the app's room, or `None`
    /// while the runtime gives no velocity of the head. A host uses it to predict the head's pose
    /// beyond the frame's display time.
    pub device_linear_velocity: Option<[f32; 3]>,
    /// How fast the head turns, in radians per second about the axes of the app's room, or `None`
    /// while the runtime gives no velocity of the head.
    pub device_angular_velocity: Option<[f32; 3]>,
    /// The pose of the OpenXR STAGE space in the app's room, or `None` when the runtime has no
    /// stage or does not locate it. The stage's origin lies on the floor at the center of the play
    /// area, with its y axis up, so the y of its position is the floor's height in the room.
    /// A streamed game takes its floor from it.
    pub stage_pose: Option<Pose>,
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
    /// The joints of the left hand, which cameras track.
    pub left_hand: HandState,
    /// The joints of the right hand, which cameras track.
    pub right_hand: HandState,
    /// The joints of the body.
    pub body: BodyState,
    /// The weights of the face's blend shapes, with Jetpack XR's names.
    pub face: FaceState,
    /// The weights of the face's blend shapes as `XR_FB_face_tracking2` gives them, with Meta's
    /// names. They stay at rest on a runtime that supplies the face through another extension.
    pub meta_face: MetaFaceState,
    /// Where the eyes look.
    pub eyes: Eyes,
    /// How far each tracking feature has come in this session. It follows the permissions that the
    /// user grants.
    pub tracking: TrackingCapabilities,
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

    /// The height of the floor in the app's room in meters, or `None` while the runtime gives no
    /// [`stage_pose`](Self::stage_pose). It is negative when the floor lies below where the app
    /// started.
    pub fn floor_height(&self) -> Option<f32> {
        self.stage_pose.map(|stage| stage.position[1])
    }

    /// The tracked joints of `hand`. A hand that is not tracked has the state of a
    /// [`HandState`], at rest.
    pub const fn hand(&self, hand: Hand) -> &HandState {
        match hand {
            Hand::Left => &self.left_hand,
            Hand::Right => &self.right_hand,
        }
    }

    /// The controller of `hand`.
    pub const fn controller(&self, hand: Hand) -> &Controller {
        match hand {
            Hand::Left => &self.left_controller,
            Hand::Right => &self.right_controller,
        }
    }
}
