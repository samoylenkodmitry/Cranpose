//! The pose of the gaze, which `XR_EXT_eye_gaze_interaction` gives as an input action.

use openxr as xr;

use crate::{
    Pose,
    openxr::{graphics::Api, hand_actions::failed, session::XrSession, tracked_pose::tracked},
};

const PROFILE: &str = "/interaction_profiles/ext/eye_gaze_interaction";
const GAZE_POSE: &str = "/user/eyes_ext/input/gaze_ext/pose";

// The gaze pose action and its action set. The session attaches the set once, with the
// controllers' set, so the action is declared before the attach and its space is made after.
pub(crate) struct EyeGazeAction {
    set: xr::ActionSet,
    action: xr::Action<xr::Posef>,
    space: Option<xr::Space>,
}

impl EyeGazeAction {
    // Declares the action and suggests its binding. The instance must have enabled the extension. A
    // runtime that rejects the profile leaves the action without a pose.
    pub(crate) fn declare(instance: &xr::Instance) -> Result<Self, String> {
        let set = instance
            .create_action_set("eye_gaze", "Eye gaze", 0)
            .map_err(failed("eye gaze action set"))?;
        let action = set
            .create_action::<xr::Posef>("gaze", "Gaze", &[])
            .map_err(failed("eye gaze action"))?;
        let path = |path: &str| instance.string_to_path(path).map_err(failed("path"));
        if let Err(error) = instance.suggest_interaction_profile_bindings(
            path(PROFILE)?,
            &[xr::Binding::new(&action, path(GAZE_POSE)?)],
        ) {
            log::warn!("cranpose xr: the eye gaze binding: {error}");
        }
        Ok(Self {
            set,
            action,
            space: None,
        })
    }

    // The action set, for the session to attach and for each frame to sync.
    pub(crate) fn set(&self) -> &xr::ActionSet {
        &self.set
    }

    // Makes the action's space, after the session attached the set.
    pub(crate) fn attach_space(&mut self, session: &xr::Session<Api>) -> Result<(), String> {
        self.space = Some(
            self.action
                .create_space(session, xr::Path::NULL, xr::Posef::IDENTITY)
                .map_err(failed("eye gaze space"))?,
        );
        Ok(())
    }

    // Where the gaze points at `time` in the app's room, or `None` while the runtime gives no gaze.
    // The set must have been synced.
    pub(crate) fn locate(&self, xr_session: &XrSession, time: xr::Time) -> Option<Pose> {
        let space = self.space.as_ref()?;
        tracked(space, &xr_session.local, time).map(|located| located.pose)
    }
}
