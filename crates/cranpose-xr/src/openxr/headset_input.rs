//! The headset's state in each frame, for the app's frame effects.

use std::time::Duration;

use openxr as xr;

use super::{
    clock,
    controllers::Controllers,
    eye_views,
    session::{SessionStatus, XrSession},
    system_properties,
};
use crate::{
    Controller, Haptics, HeadsetProperties, Pose, Ray, Recenter, XrFrame,
    frame_effects::FrameEffects, pointer::Aim,
};

// What the app gets of the headset in a frame, the controllers that it reads, and the callbacks
// and the vibrations that it set.
pub(crate) struct HeadsetInput {
    // Filled in place each frame
    frame: XrFrame,
    controllers: Controllers,
    effects: FrameEffects,
    haptics: Haptics,
    properties: HeadsetProperties,
}

impl HeadsetInput {
    pub(crate) fn new(xr_session: &XrSession) -> Result<Self, String> {
        let properties = system_properties::query(xr_session)?;
        log::info!("cranpose xr: headset {properties:?}");
        Ok(Self {
            frame: XrFrame::default(),
            controllers: Controllers::new(xr_session)?,
            effects: FrameEffects::default(),
            haptics: Haptics::new(),
            properties,
        })
    }

    // The registry of the app's frame effects, which the content reads as a composition local.
    pub(crate) fn effects(&self) -> FrameEffects {
        self.effects.clone()
    }

    // The handle of the app's vibrations, which the content reads as a composition local.
    pub(crate) fn haptics(&self) -> Haptics {
        self.haptics.clone()
    }

    // The headset's facts, which the content reads as a composition local.
    pub(crate) fn properties(&self) -> HeadsetProperties {
        self.properties.clone()
    }

    // The time at which the frame sampled last reaches the display, on the monotonic clock.
    pub(crate) fn display_time(&self) -> Duration {
        self.frame.predicted_display_time
    }

    // The runtime's change of the room's origin that the frame sampled last reports.
    pub(crate) fn recenter(&self) -> Option<Recenter> {
        self.frame.recenter
    }

    // The head's pose in the frame sampled last.
    pub(crate) fn device_pose(&self) -> Option<Pose> {
        self.frame.device_pose
    }

    // The pose of each controller's grip in the frame sampled last, left then right, or `None`
    // for a controller that the runtime does not track.
    pub(crate) fn grips(&self) -> [Option<Pose>; 2] {
        [&self.frame.left_controller, &self.frame.right_controller]
            .map(|controller| controller.grip.map(|grip| grip.pose))
    }

    // Samples the headset for the frame that `state` describes, runs the app's frame effects and
    // plays the vibrations that they or the app's handlers asked for. Returns where each
    // controller points, for the pointer.
    pub(crate) fn sample(
        &mut self,
        xr_session: &mut XrSession,
        state: &xr::FrameState,
        status: SessionStatus,
    ) -> [Option<Aim>; 2] {
        let time = state.predicted_display_time;
        let frame = &mut self.frame;
        frame.now = clock::now();
        frame.predicted_display_time = clock::monotonic(xr_session.instance(), time);
        frame.period =
            Duration::from_nanos(state.predicted_display_period.as_nanos().max(0) as u64);
        frame.focused = status.listens();
        let head = xr_session.head(time);
        frame.device_pose = head.map(|head| head.pose);
        frame.device_linear_velocity = head.and_then(|head| head.linear_velocity);
        frame.device_angular_velocity = head.and_then(|head| head.angular_velocity);
        frame.stage_pose = xr_session.stage(time);
        [frame.left_eye, frame.right_eye] = eye_views::locate(
            frame.device_pose.as_ref(),
            xr_session.instance(),
            &xr_session.session,
            &xr_session.local,
            time,
        );
        frame.recenter = xr_session.recenters.take();
        self.controllers.sample(xr_session, time, status, frame);
        self.effects.run(&self.frame);
        self.controllers.vibrate(xr_session, self.haptics.take());
        [&self.frame.left_controller, &self.frame.right_controller].map(aim_of)
    }
}

// Where the controller points and how its trigger and stick lean, for the pointer. A controller
// that the runtime does not track points nowhere.
fn aim_of(controller: &Controller) -> Option<Aim> {
    let aim = controller.aim?;
    Some(Aim {
        ray: Ray::of_aim(&aim.pose),
        pose: aim.pose,
        trigger: controller.trigger.value,
        stick: controller.thumbstick.y,
    })
}
