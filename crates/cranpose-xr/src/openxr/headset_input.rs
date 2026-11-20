//! The headset's state in each frame, for the app's frame effects.

use std::time::Duration;

use openxr as xr;

use super::{
    clock,
    controllers::Controllers,
    eye_views,
    host::Host,
    session::XrSession,
    system_properties,
    tracking::{EyeGazeAction, Trackers, TrackingSetup},
};
use crate::{
    Hand, Haptics, HeadsetProperties, PointerMapping, PointerSettings, Pose, Recenter,
    RenderViewpoint, TrackingFeature, XrFrame, aim::Aim, frame_effects::FrameEffects,
    haptics::controller_asks, pointer_aim::pointer_aim, session_status::SessionStatus,
};

// The handles on the headset that the app's content reads: the registry of its frame effects, its
// vibrations, the headset's facts and the pointer's settings.
pub(crate) struct Handles {
    pub(crate) effects: FrameEffects,
    pub(crate) haptics: Haptics,
    pub(crate) properties: HeadsetProperties,
    pub(crate) pointer_settings: PointerSettings,
}

// What the app gets of the headset in a frame, the controllers that it reads, and the callbacks
// and the vibrations that it set.
pub(crate) struct HeadsetInput {
    // Filled in place each frame
    frame: XrFrame,
    controllers: Controllers,
    trackers: Trackers,
    effects: FrameEffects,
    haptics: Haptics,
    pointer_settings: PointerSettings,
    properties: HeadsetProperties,
    // Whether tracked hands point: the app asked for hand tracking
    hands_point: bool,
}

impl HeadsetInput {
    pub(crate) fn new(xr_session: &XrSession, tracking: TrackingSetup) -> Result<Self, String> {
        let properties = system_properties::query(xr_session)?;
        log::info!("cranpose xr: headset {properties:?}");
        // The gaze action joins the controllers' attach, because a session attaches once
        let mut gaze = tracking
            .capabilities
            .is_eye_gaze_interaction_available()
            .then(|| EyeGazeAction::declare(xr_session.instance()))
            .transpose()?;
        let controllers = Controllers::new(xr_session, gaze.as_ref().map(EyeGazeAction::set))?;
        if let Some(gaze) = &mut gaze {
            gaze.attach_space(&xr_session.session)?;
        }
        let frame = XrFrame {
            tracking: tracking.capabilities,
            ..XrFrame::default()
        };
        let hands_point = tracking.request.asks(TrackingFeature::Hands);
        Ok(Self {
            frame,
            controllers,
            trackers: Trackers::new(tracking, gaze),
            effects: FrameEffects::default(),
            haptics: Haptics::new(),
            pointer_settings: PointerSettings::new(),
            properties,
            hands_point,
        })
    }

    // The handles that the content reads as composition locals.
    pub(crate) fn handles(&self) -> Handles {
        Handles {
            effects: self.effects.clone(),
            haptics: self.haptics.clone(),
            properties: self.properties.clone(),
            pointer_settings: self.pointer_settings.clone(),
        }
    }

    // The app's choices that the pointer reads each frame: the button mapping, and whether the
    // beams show.
    pub(crate) fn pointer_choice(&self) -> (PointerMapping, bool) {
        (
            self.pointer_settings.mapping(),
            self.pointer_settings.beams_visible(),
        )
    }

    // The time at which the frame sampled last reaches the display, on the monotonic clock.
    pub(crate) fn display_time(&self) -> Duration {
        self.frame.predicted_display_time
    }

    // The time at which the backend read the clock for the frame sampled last.
    pub(crate) fn now(&self) -> Duration {
        self.frame.now
    }

    // The runtime's change of the room's origin that the frame sampled last reports.
    pub(crate) fn recenter(&self) -> Option<Recenter> {
        self.frame.recenter
    }

    // The head's pose in the frame sampled last.
    pub(crate) fn device_pose(&self) -> Option<Pose> {
        self.frame.device_pose
    }

    // The left and the right eye's view in the frame sampled last, or `None` for an eye that the
    // runtime does not locate.
    pub(crate) fn eyes(&self) -> [Option<RenderViewpoint>; 2] {
        [self.frame.left_eye, self.frame.right_eye]
    }

    // The size of the pictures that the eyes' displays take, in pixels: the left eye's
    // recommended size.
    pub(crate) fn eye_image_size(&self) -> (u32, u32) {
        let [width, height] = self.properties.eye_image_sizes[0].recommended;
        (width, height)
    }

    // Where each controller is in the frame sampled last, left then right: the place of its grip
    // with the turn of its aim, or `None` for a controller that the runtime does not track. The
    // grip's own forward axis runs along the handle, which points steeply upward when the hand
    // holds the controller as usual. The aim's forward axis is the ray, so its heading stays
    // steady when the hand rises.
    pub(crate) fn controllers(&self) -> [Option<Pose>; 2] {
        [&self.frame.left_controller, &self.frame.right_controller].map(|controller| {
            Some(Pose {
                position: controller.grip?.pose.position,
                orientation: controller.aim?.pose.orientation,
            })
        })
    }

    // Samples the headset for the frame that `state` describes, runs the app's frame effects and
    // plays the vibrations that they or the app's handlers asked for. Returns where each hand
    // points, for the pointer: its controller, or its tracked hand while it holds no controller.
    pub(crate) fn sample(
        &mut self,
        xr_session: &mut XrSession,
        state: &xr::FrameState,
        status: SessionStatus,
        host: &Host,
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
        self.controllers
            .sample(xr_session, time, status, self.trackers.gaze_set(), frame);
        #[cfg(feature = "scripted-hand")]
        if frame.focused {
            crate::scripted_controller::apply(frame);
        }
        self.trackers
            .sample(xr_session, host, frame.now, time, status, frame);
        let hands = self.hands_point;
        let aims = Hand::BOTH
            .map(|side| pointer_aim(frame.controller(side), hands.then_some(frame.hand(side))));
        [frame.left_pointer, frame.right_pointer] = aims.map(|aim| aim.map(|aim| aim.source));
        self.effects.run(&self.frame);
        let asks = controller_asks(
            self.haptics.take(),
            [self.frame.left_pointer, self.frame.right_pointer],
        );
        self.controllers.vibrate(xr_session, asks);
        aims
    }
}
