//! The joints of both hands, and where each hand points.
#![expect(
    unsafe_code,
    reason = "xrLocateHandJointsEXT with the aim state chained on the stack, because the safe wrapper chains nothing"
)]

use std::ptr;

use openxr as xr;

use crate::{
    HandAim, HandJoint, HandJointType, HandState, Pinch, TrackingState,
    extension_offer::ExtensionOffer,
    joint_flags::joint_flags,
    openxr::{graphics::Api, session::pose_of},
};

// One tracker for each hand. The aim state of `XR_FB_hand_tracking_aim` rides along when the
// runtime enables it.
pub(crate) struct HandTrackers {
    instance: xr::Instance,
    trackers: [xr::HandTracker; 2],
    aim: bool,
}

impl HandTrackers {
    pub(crate) fn create(
        session: &xr::Session<Api>,
        instance: &xr::Instance,
        enabled: &ExtensionOffer,
    ) -> Result<Self, xr::sys::Result> {
        Ok(Self {
            instance: instance.clone(),
            trackers: [
                session.create_hand_tracker(xr::Hand::LEFT)?,
                session.create_hand_tracker(xr::Hand::RIGHT)?,
            ],
            aim: enabled.hand_tracking_aim,
        })
    }

    // Writes both hands at `time` in the space `base`, left then right. A hand that the runtime
    // does not locate is paused with every joint at rest.
    pub(crate) fn locate(&self, base: xr::sys::Space, time: xr::Time, states: [&mut HandState; 2]) {
        for (tracker, state) in self.trackers.iter().zip(states) {
            *state = self.locate_one(tracker, base, time);
        }
    }

    fn locate_one(
        &self,
        tracker: &xr::HandTracker,
        base: xr::sys::Space,
        time: xr::Time,
    ) -> HandState {
        let paused = HandState {
            tracking_state: TrackingState::Paused,
            ..HandState::default()
        };
        let Some(fp) = self.instance.exts().ext_hand_tracking.as_ref() else {
            return paused;
        };
        let info = xr::sys::HandJointsLocateInfoEXT {
            ty: xr::sys::HandJointsLocateInfoEXT::TYPE,
            next: ptr::null(),
            base_space: base,
            time,
        };
        let mut aim = xr::sys::HandTrackingAimStateFB {
            ty: xr::sys::HandTrackingAimStateFB::TYPE,
            next: ptr::null_mut(),
            status: xr::sys::HandTrackingAimFlagsFB::EMPTY,
            aim_pose: xr::Posef::IDENTITY,
            pinch_strength_index: 0.0,
            pinch_strength_middle: 0.0,
            pinch_strength_ring: 0.0,
            pinch_strength_little: 0.0,
        };
        let mut joints = [xr::sys::HandJointLocationEXT {
            location_flags: xr::SpaceLocationFlags::EMPTY,
            pose: xr::Posef::IDENTITY,
            radius: 0.0,
        }; HandJointType::COUNT];
        let mut locations = xr::sys::HandJointLocationsEXT {
            ty: xr::sys::HandJointLocationsEXT::TYPE,
            next: if self.aim {
                ptr::from_mut(&mut aim).cast()
            } else {
                ptr::null_mut()
            },
            is_active: false.into(),
            joint_count: HandJointType::COUNT as u32,
            joint_locations: joints.as_mut_ptr(),
        };
        // SAFETY: the tracker handle is live. `info` and `locations` are initialized with their
        // types. `joints` has room for the 26 joints that `joint_count` names, and `aim`, which
        // the chain holds only when the instance enabled `XR_FB_hand_tracking_aim`, lives past
        // the call.
        let result = unsafe { (fp.locate_hand_joints)(tracker.as_raw(), &info, &mut locations) };
        if result.into_raw() < 0 || !bool::from(locations.is_active) {
            return paused;
        }
        let mut state = HandState {
            tracking_state: TrackingState::Tracking,
            ..HandState::default()
        };
        for (joint, located) in state.joints.iter_mut().zip(&joints) {
            let (valid, tracked) = joint_flags(located.location_flags.into_raw());
            *joint = HandJoint {
                pose: pose_of(&located.pose),
                radius: located.radius,
                valid,
                tracked,
            };
        }
        state.aim = (self.aim && aim.status.contains(xr::sys::HandTrackingAimFlagsFB::VALID))
            .then(|| aim_of(&aim));
        state
    }
}

fn aim_of(aim: &xr::sys::HandTrackingAimStateFB) -> HandAim {
    use xr::sys::HandTrackingAimFlagsFB as Flag;
    let pinch = |strength: f32, flag: Flag| Pinch {
        strength,
        pinching: aim.status.contains(flag),
    };
    HandAim {
        pose: pose_of(&aim.aim_pose),
        index: pinch(aim.pinch_strength_index, Flag::INDEX_PINCHING),
        middle: pinch(aim.pinch_strength_middle, Flag::MIDDLE_PINCHING),
        ring: pinch(aim.pinch_strength_ring, Flag::RING_PINCHING),
        little: pinch(aim.pinch_strength_little, Flag::LITTLE_PINCHING),
        system_gesture: aim.status.contains(Flag::SYSTEM_GESTURE),
        menu_pressed: aim.status.contains(Flag::MENU_PRESSED),
        dominant: aim.status.contains(Flag::DOMINANT_HAND),
    }
}
