//! The joints of the body.
#![expect(
    unsafe_code,
    reason = "XR_FB_body_tracking, which the openxr binding wraps in no safe call"
)]

use std::ptr;

use openxr as xr;

use super::created_handle::created_handle;
use crate::{
    BodyJoint, BodyJointType, BodyState, TrackingCapabilities,
    joint_flags::joint_flags,
    openxr::{graphics::Api, session::pose_of},
};

// The number of joints of the upper body, which are the first of the full body's.
const UPPER_BODY_JOINTS: u32 = 70;
// The number of joints of the full body
const FULL_BODY_JOINTS: u32 = BodyJointType::COUNT as u32;

// A body tracker. It tracks the full body when the runtime enables `XR_META_body_tracking_full_body`
// and the headset supports it.
pub(crate) struct BodyTracker {
    instance: xr::Instance,
    handle: xr::sys::BodyTrackerFB,
    joint_count: u32,
}

impl BodyTracker {
    // Makes the tracker. Where the capabilities offer the full body it asks for the legs and feet
    // too, and falls back to the upper body when the headset refuses them.
    pub(crate) fn create(
        session: &xr::Session<Api>,
        instance: &xr::Instance,
        capabilities: &TrackingCapabilities,
    ) -> Result<Self, xr::sys::Result> {
        let fp = instance
            .exts()
            .fb_body_tracking
            .as_ref()
            .ok_or(xr::sys::Result::ERROR_EXTENSION_NOT_PRESENT)?;
        let make = |joint_set: xr::sys::BodyJointSetFB| {
            let info = xr::sys::BodyTrackerCreateInfoFB {
                ty: xr::sys::BodyTrackerCreateInfoFB::TYPE,
                next: ptr::null(),
                body_joint_set: joint_set,
            };
            // SAFETY: the session handle is live, and `info` is initialized with its type.
            created_handle(|handle| unsafe {
                (fp.create_body_tracker)(session.as_raw(), &info, handle)
            })
        };
        let (handle, joint_count) = match capabilities
            .is_full_body_available()
            .then(|| make(xr::sys::BodyJointSetFB::FULL_BODY_M))
            .and_then(Result::ok)
        {
            Some(handle) => (handle, FULL_BODY_JOINTS),
            None => (make(xr::sys::BodyJointSetFB::DEFAULT)?, UPPER_BODY_JOINTS),
        };
        Ok(Self {
            instance: instance.clone(),
            handle,
            joint_count,
        })
    }

    // Writes the body at `time` in the space `base`. A body that the runtime does not track leaves
    // the state inactive.
    pub(crate) fn locate(&self, base: xr::sys::Space, time: xr::Time, state: &mut BodyState) {
        *state = BodyState::default();
        let Some(fp) = self.instance.exts().fb_body_tracking.as_ref() else {
            return;
        };
        let info = xr::sys::BodyJointsLocateInfoFB {
            ty: xr::sys::BodyJointsLocateInfoFB::TYPE,
            next: ptr::null(),
            base_space: base,
            time,
        };
        let mut joints = [xr::sys::BodyJointLocationFB {
            location_flags: xr::SpaceLocationFlags::EMPTY,
            pose: xr::Posef::IDENTITY,
        }; BodyJointType::COUNT];
        let mut locations = xr::sys::BodyJointLocationsFB {
            ty: xr::sys::BodyJointLocationsFB::TYPE,
            next: ptr::null_mut(),
            is_active: false.into(),
            confidence: 0.0,
            joint_count: self.joint_count,
            joint_locations: joints.as_mut_ptr(),
            skeleton_changed_count: 0,
            time: xr::Time::from_nanos(0),
        };
        // SAFETY: the tracker handle is live. `info` and `locations` are initialized with their
        // types, and `joints` has room for the `joint_count` joints that `locations` names, at most
        // the 84 of the full body.
        let result = unsafe { (fp.locate_body_joints)(self.handle, &info, &mut locations) };
        if result.into_raw() < 0 || !bool::from(locations.is_active) {
            return;
        }
        state.active = true;
        state.confidence = locations.confidence;
        state.joint_count = self.joint_count as usize;
        for (joint, located) in state
            .joints
            .iter_mut()
            .zip(&joints)
            .take(self.joint_count as usize)
        {
            let (valid, tracked) = joint_flags(located.location_flags.into_raw());
            *joint = BodyJoint {
                pose: pose_of(&located.pose),
                valid,
                tracked,
            };
        }
    }
}

impl Drop for BodyTracker {
    fn drop(&mut self) {
        if let Some(fp) = self.instance.exts().fb_body_tracking.as_ref() {
            // SAFETY: the handle is live and nothing uses it after this.
            unsafe {
                (fp.destroy_body_tracker)(self.handle);
            }
        }
    }
}
