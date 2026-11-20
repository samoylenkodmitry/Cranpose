//! The weights of the face's blend shapes.
#![expect(
    unsafe_code,
    reason = "XR_FB_face_tracking2 and XR_ANDROID_face_tracking, which the openxr binding wraps in no safe call"
)]

use std::ptr;

use openxr as xr;

use super::created_handle::created_handle;
#[cfg(target_os = "android")]
use crate::{FaceBlendShapeType, FaceConfidenceRegion, TrackingState};
use crate::{
    FaceDataSource, FaceExpression, FaceState, FaceTrackingProvider, MetaFaceState,
    openxr::graphics::Api,
};

// The data sources that the Meta tracker asks for: the visual one and the audio one
const SOURCES: u32 = 2;
// The confidences that the Meta tracker gives: the lower face and the upper face
const CONFIDENCES: u32 = 2;

// A face tracker of the extension that supplies the face.
pub(crate) enum FaceTracker {
    Meta(MetaFaceTracker),
    #[cfg(target_os = "android")]
    Android(AndroidFaceTracker),
}

impl FaceTracker {
    pub(crate) fn create(
        provider: FaceTrackingProvider,
        session: &xr::Session<Api>,
        instance: &xr::Instance,
    ) -> Result<Self, xr::sys::Result> {
        match provider {
            FaceTrackingProvider::Meta => {
                MetaFaceTracker::create(session, instance).map(Self::Meta)
            }
            #[cfg(target_os = "android")]
            FaceTrackingProvider::Android => {
                AndroidFaceTracker::create(session, instance).map(Self::Android)
            }
            #[cfg(not(target_os = "android"))]
            FaceTrackingProvider::Android => Err(xr::sys::Result::ERROR_EXTENSION_NOT_PRESENT),
        }
    }

    // Writes the face at `time` into both of the frame's face states. A face that the runtime does
    // not give leaves them paused.
    pub(crate) fn locate(&self, time: xr::Time, face: &mut FaceState, meta: &mut MetaFaceState) {
        match self {
            Self::Meta(tracker) => {
                tracker.locate(time, meta);
                *face = FaceState::from_meta(meta);
            }
            #[cfg(target_os = "android")]
            Self::Android(tracker) => tracker.locate(time, face),
        }
    }
}

// A tracker of `XR_FB_face_tracking2`. It asks for the visual source and the audio source, and the
// runtime picks the one that the headset can give: a Quest 3 has no camera for the mouth and
// gives the audio's.
pub(crate) struct MetaFaceTracker {
    instance: xr::Instance,
    handle: xr::sys::FaceTracker2FB,
}

impl MetaFaceTracker {
    fn create(
        session: &xr::Session<Api>,
        instance: &xr::Instance,
    ) -> Result<Self, xr::sys::Result> {
        let fp = instance
            .exts()
            .fb_face_tracking2
            .as_ref()
            .ok_or(xr::sys::Result::ERROR_EXTENSION_NOT_PRESENT)?;
        let mut sources = [
            xr::sys::FaceTrackingDataSource2FB::VISUAL,
            xr::sys::FaceTrackingDataSource2FB::AUDIO,
        ];
        debug_assert_eq!(sources.len(), SOURCES as usize);
        let info = xr::sys::FaceTrackerCreateInfo2FB {
            ty: xr::sys::FaceTrackerCreateInfo2FB::TYPE,
            next: ptr::null(),
            face_expression_set: xr::sys::FaceExpressionSet2FB::DEFAULT,
            requested_data_source_count: SOURCES,
            requested_data_sources: sources.as_mut_ptr(),
        };
        // SAFETY: the session handle is live. `info` is initialized with its type, and its
        // `requested_data_sources` names the two sources that `sources` holds.
        let handle = created_handle(|handle| unsafe {
            (fp.create_face_tracker2)(session.as_raw(), &info, handle)
        })?;
        Ok(Self {
            instance: instance.clone(),
            handle,
        })
    }

    fn locate(&self, time: xr::Time, state: &mut MetaFaceState) {
        *state = MetaFaceState::default();
        let Some(fp) = self.instance.exts().fb_face_tracking2.as_ref() else {
            return;
        };
        let info = xr::sys::FaceExpressionInfo2FB {
            ty: xr::sys::FaceExpressionInfo2FB::TYPE,
            next: ptr::null(),
            time,
        };
        let mut confidences = [0.0_f32; CONFIDENCES as usize];
        let mut weights = xr::sys::FaceExpressionWeights2FB {
            ty: xr::sys::FaceExpressionWeights2FB::TYPE,
            next: ptr::null_mut(),
            weight_count: FaceExpression::COUNT as u32,
            weights: state.weights.as_mut_ptr(),
            confidence_count: CONFIDENCES,
            confidences: confidences.as_mut_ptr(),
            is_valid: false.into(),
            is_eye_following_blendshapes_valid: false.into(),
            data_source: xr::sys::FaceTrackingDataSource2FB::VISUAL,
            time: xr::Time::from_nanos(0),
        };
        // SAFETY: the tracker handle is live. `info` and `weights` are initialized with their types.
        // `weights.weights` points into the state's array of 70 floats, which `weight_count`
        // names, and `confidences` holds the two floats that `confidence_count` names.
        let result = unsafe { (fp.get_face_expression_weights2)(self.handle, &info, &mut weights) };
        if result.into_raw() < 0 {
            *state = MetaFaceState::default();
            return;
        }
        state.is_valid = weights.is_valid.into();
        state.is_eye_following_blendshapes_valid =
            weights.is_eye_following_blendshapes_valid.into();
        state.data_source = Some(
            if weights.data_source == xr::sys::FaceTrackingDataSource2FB::AUDIO {
                FaceDataSource::Audio
            } else {
                FaceDataSource::Visual
            },
        );
        state.lower_face_confidence = confidences[0];
        state.upper_face_confidence = confidences[1];
        if !state.is_valid {
            state.weights = [0.0; FaceExpression::COUNT];
        }
    }
}

impl Drop for MetaFaceTracker {
    fn drop(&mut self) {
        if let Some(fp) = self.instance.exts().fb_face_tracking2.as_ref() {
            // SAFETY: the handle is live and nothing uses it after this.
            unsafe {
                (fp.destroy_face_tracker2)(self.handle);
            }
        }
    }
}

// A tracker of `XR_ANDROID_face_tracking`.
#[cfg(target_os = "android")]
pub(crate) struct AndroidFaceTracker {
    instance: xr::Instance,
    handle: xr::sys::FaceTrackerANDROID,
}

#[cfg(target_os = "android")]
impl AndroidFaceTracker {
    fn create(
        session: &xr::Session<Api>,
        instance: &xr::Instance,
    ) -> Result<Self, xr::sys::Result> {
        let fp = instance
            .exts()
            .android_face_tracking
            .as_ref()
            .ok_or(xr::sys::Result::ERROR_EXTENSION_NOT_PRESENT)?;
        let info = xr::sys::FaceTrackerCreateInfoANDROID {
            ty: xr::sys::FaceTrackerCreateInfoANDROID::TYPE,
            next: ptr::null(),
        };
        // SAFETY: the session handle is live, and `info` is initialized with its type.
        let handle = created_handle(|handle| unsafe {
            (fp.create_face_tracker)(session.as_raw(), &info, handle)
        })?;
        Ok(Self {
            instance: instance.clone(),
            handle,
        })
    }

    fn locate(&self, time: xr::Time, state: &mut FaceState) {
        *state = FaceState::default();
        let Some(fp) = self.instance.exts().android_face_tracking.as_ref() else {
            return;
        };
        let info = xr::sys::FaceStateGetInfoANDROID {
            ty: xr::sys::FaceStateGetInfoANDROID::TYPE,
            next: ptr::null(),
            time,
        };
        let mut out = xr::sys::FaceStateANDROID {
            ty: xr::sys::FaceStateANDROID::TYPE,
            next: ptr::null_mut(),
            parameters_capacity_input: FaceBlendShapeType::COUNT as u32,
            parameters_count_output: 0,
            parameters: state.blend_shapes.as_mut_ptr(),
            face_tracking_state: xr::sys::FaceTrackingStateANDROID::STOPPED,
            sample_time: xr::Time::from_nanos(0),
            is_valid: false.into(),
            region_confidences_capacity_input: FaceConfidenceRegion::COUNT as u32,
            region_confidences_count_output: 0,
            region_confidences: state.confidence_regions.as_mut_ptr(),
        };
        // SAFETY: the tracker handle is live. `info` and `out` are initialized with their types.
        // `out.parameters` and `out.region_confidences` point into the state's arrays of 68 and 3
        // floats, which their capacities name.
        let result = unsafe { (fp.get_face_state)(self.handle, &info, &mut out) };
        if result.into_raw() < 0 {
            *state = FaceState::default();
            return;
        }
        state.tracking_state =
            if out.face_tracking_state == xr::sys::FaceTrackingStateANDROID::TRACKING {
                TrackingState::Tracking
            } else if out.face_tracking_state == xr::sys::FaceTrackingStateANDROID::PAUSED {
                TrackingState::Paused
            } else {
                TrackingState::Stopped
            };
        state.is_valid = out.is_valid.into();
        if !state.is_valid {
            *state = FaceState {
                tracking_state: state.tracking_state,
                ..FaceState::default()
            };
        }
    }
}

#[cfg(target_os = "android")]
impl Drop for AndroidFaceTracker {
    fn drop(&mut self) {
        if let Some(fp) = self.instance.exts().android_face_tracking.as_ref() {
            // SAFETY: the handle is live and nothing uses it after this.
            unsafe {
                (fp.destroy_face_tracker)(self.handle);
            }
        }
    }
}
