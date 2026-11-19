//! The pose of each eye.
#![expect(
    unsafe_code,
    reason = "XR_FB_eye_tracking_social, which the openxr binding wraps in no safe call"
)]

use std::ptr;

use openxr as xr;

use super::created_handle::created_handle;
use crate::{
    EyeState, Eyes, TrackingState,
    openxr::{graphics::Api, session::pose_of},
};

// A tracker of `XR_FB_eye_tracking_social`: the pose of each eye with a confidence.
pub(crate) struct EyeTracker {
    instance: xr::Instance,
    handle: xr::sys::EyeTrackerFB,
}

impl EyeTracker {
    pub(crate) fn create(
        session: &xr::Session<Api>,
        instance: &xr::Instance,
    ) -> Result<Self, xr::sys::Result> {
        let fp = instance
            .exts()
            .fb_eye_tracking_social
            .as_ref()
            .ok_or(xr::sys::Result::ERROR_EXTENSION_NOT_PRESENT)?;
        let info = xr::sys::EyeTrackerCreateInfoFB {
            ty: xr::sys::EyeTrackerCreateInfoFB::TYPE,
            next: ptr::null(),
        };
        // SAFETY: the session handle is live, and `info` is initialized with its type.
        let handle = created_handle(|handle| unsafe {
            (fp.create_eye_tracker)(session.as_raw(), &info, handle)
        })?;
        Ok(Self {
            instance: instance.clone(),
            handle,
        })
    }

    // Writes the two eyes at `time` in the space `base` into `eyes`, and leaves its combined gaze
    // alone. An eye that the runtime does not give is paused.
    pub(crate) fn locate(&self, base: xr::sys::Space, time: xr::Time, eyes: &mut Eyes) {
        let paused = EyeState {
            tracking_state: TrackingState::Paused,
            ..EyeState::default()
        };
        [eyes.left, eyes.right] = [paused, paused];
        let Some(fp) = self.instance.exts().fb_eye_tracking_social.as_ref() else {
            return;
        };
        let info = xr::sys::EyeGazesInfoFB {
            ty: xr::sys::EyeGazesInfoFB::TYPE,
            next: ptr::null(),
            base_space: base,
            time,
        };
        let mut gazes = xr::sys::EyeGazesFB {
            ty: xr::sys::EyeGazesFB::TYPE,
            next: ptr::null_mut(),
            gaze: [xr::sys::EyeGazeFB {
                is_valid: false.into(),
                gaze_pose: xr::Posef::IDENTITY,
                gaze_confidence: 0.0,
            }; xr::sys::EYE_POSITION_COUNT_FB],
            time: xr::Time::from_nanos(0),
        };
        // SAFETY: the tracker handle is live, and `info` and `gazes` are initialized with their
        // types.
        let result = unsafe { (fp.get_eye_gazes)(self.handle, &info, &mut gazes) };
        if result.into_raw() < 0 {
            return;
        }
        let eye = |gaze: &xr::sys::EyeGazeFB| {
            if bool::from(gaze.is_valid) {
                EyeState {
                    tracking_state: TrackingState::Tracking,
                    pose: pose_of(&gaze.gaze_pose),
                    confidence: gaze.gaze_confidence,
                }
            } else {
                paused
            }
        };
        // The array holds the left eye first, as `XR_EYE_POSITION_LEFT_FB` is 0
        eyes.left = eye(&gazes.gaze[0]);
        eyes.right = eye(&gazes.gaze[1]);
    }
}

impl Drop for EyeTracker {
    fn drop(&mut self) {
        if let Some(fp) = self.instance.exts().fb_eye_tracking_social.as_ref() {
            // SAFETY: the handle is live and nothing uses it after this.
            unsafe {
                (fp.destroy_eye_tracker)(self.handle);
            }
        }
    }
}
