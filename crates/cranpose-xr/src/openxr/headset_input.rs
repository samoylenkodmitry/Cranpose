//! The headset's state in each frame, for the app's frame effects.

use std::time::Duration;

use openxr as xr;

use super::{
    clock,
    controllers::Controllers,
    eye_views,
    session::{SessionStatus, XrSession},
};
use crate::{Controller, Haptics, Pose, Ray, XrFrame, frame_effects::FrameEffects, pointer::Aim};

// What the app gets of the headset in a frame, the controllers that it reads, and the callbacks
// and the vibrations that it set.
pub(crate) struct HeadsetInput {
    // Filled in place each frame
    frame: XrFrame,
    controllers: Controllers,
    effects: FrameEffects,
    haptics: Haptics,
}

impl HeadsetInput {
    pub(crate) fn new(xr_session: &XrSession) -> Result<Self, String> {
        Ok(Self {
            frame: XrFrame::default(),
            controllers: Controllers::new(xr_session)?,
            effects: FrameEffects::default(),
            haptics: Haptics::new(),
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

    // The head's pose in the frame sampled last.
    pub(crate) fn device_pose(&self) -> Option<Pose> {
        self.frame.device_pose
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
        frame.predicted_display_time = clock::monotonic(xr_session.instance(), time);
        frame.period =
            Duration::from_nanos(state.predicted_display_period.as_nanos().max(0) as u64);
        frame.focused = status.listens();
        frame.device_pose = xr_session.head(time);
        [frame.left_eye, frame.right_eye] = eye_views::locate(
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
