//! What the runtime offers of hands, body, face and eyes.

use cranpose_core::CompositionLocal;

use crate::{FaceTrackingProvider, TrackingAvailability, TrackingFeature, locals::with_locals};
#[cfg(any(test, headset))]
use crate::{
    extension_offer::ExtensionOffer, system_support::SystemSupport,
    tracking_request::TrackingRequest,
};

/// What the runtime offers of hands, body, face and eyes, and how far each has come. An app reads
/// it through [`local_tracking_capabilities`], and [`XrFrame::tracking`](crate::XrFrame::tracking)
/// holds the live one.
///
/// A feature that the app did not ask for is [`Offered`](TrackingAvailability::Offered) when the
/// runtime lists an extension for it. Nothing is enabled then, so the list cannot tell whether the
/// headset has the sensor. A feature that the app asked for gets the system's answer, so a Quest 3
/// reports its eyes as [`Unavailable`](TrackingAvailability::Unavailable). Jetpack XR reports the
/// same through `Session.configure`, which returns the result of the `Config` that it applied.
///
/// Outside a headset every feature is unavailable.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct TrackingCapabilities {
    availability: [TrackingAvailability; TrackingFeature::ALL.len()],
    hand_aim: bool,
    full_body: bool,
    face_provider: Option<FaceTrackingProvider>,
    eye_gaze_interaction: bool,
    eye_tracking_social: bool,
}

impl TrackingCapabilities {
    // What the runtime offers, given the extensions it lists, the app's request and what the system
    // said of the extensions that the instance enabled. The system said nothing of an extension that
    // is off, so a feature that the app did not ask for has only the list to go by.
    #[cfg(any(test, headset))]
    pub(crate) fn from_runtime(
        request: TrackingRequest,
        listed: ExtensionOffer,
        system: SystemSupport,
    ) -> Self {
        let allows =
            |listed: bool, reported: Option<bool>| listed && !SystemSupport::lacks(reported);
        let hands = allows(listed.hand_tracking, system.hand_tracking);
        let body = allows(listed.body_tracking, system.body_tracking);
        // A face comes from one extension. For an app that asked, it is the one that the instance
        // enabled, because the other one is off and answers nothing.
        let face = if request.asks(TrackingFeature::Face) {
            listed.enabled_for(request)
        } else {
            listed
        };
        let face_provider = if allows(face.face_tracking2, system.face_tracking2) {
            Some(FaceTrackingProvider::Meta)
        } else if allows(face.android_face_tracking, system.android_face_tracking) {
            Some(FaceTrackingProvider::Android)
        } else {
            None
        };
        let eye_gaze_interaction = allows(listed.eye_gaze_interaction, system.eye_gaze_interaction);
        let eye_tracking_social = allows(listed.eye_tracking_social, system.eye_tracking_social);
        let usable = [
            hands,
            body,
            face_provider.is_some(),
            eye_gaze_interaction || eye_tracking_social,
        ];
        let mut availability = [TrackingAvailability::Unavailable; TrackingFeature::ALL.len()];
        for (feature, usable) in TrackingFeature::ALL.into_iter().zip(usable) {
            availability[feature.index()] = match (usable, request.asks(feature)) {
                (false, _) => TrackingAvailability::Unavailable,
                (true, false) => TrackingAvailability::Offered,
                (true, true) => TrackingAvailability::AwaitingPermission,
            };
        }
        Self {
            availability,
            hand_aim: hands && listed.hand_tracking_aim,
            full_body: body && allows(listed.full_body_tracking, system.full_body_tracking),
            face_provider,
            eye_gaze_interaction,
            eye_tracking_social,
        }
    }

    // The same capabilities with one feature at another availability.
    #[cfg(any(test, headset))]
    pub(crate) fn with_availability(
        mut self,
        feature: TrackingFeature,
        availability: TrackingAvailability,
    ) -> Self {
        self.availability[feature.index()] = availability;
        self
    }

    /// How far `feature` has come.
    pub const fn availability(&self, feature: TrackingFeature) -> TrackingAvailability {
        self.availability[feature.index()]
    }

    /// Whether the runtime offers `feature`: its availability is not
    /// [`Unavailable`](TrackingAvailability::Unavailable).
    pub const fn offers(&self, feature: TrackingFeature) -> bool {
        !matches!(
            self.availability(feature),
            TrackingAvailability::Unavailable
        )
    }

    /// Whether the runtime gives where a hand points and how it pinches, through
    /// `XR_FB_hand_tracking_aim`. It does only with hand tracking.
    pub const fn is_hand_aim_available(&self) -> bool {
        self.hand_aim
    }

    /// Whether the runtime tracks the legs and feet too, through
    /// `XR_META_body_tracking_full_body`. It does only with body tracking.
    pub const fn is_full_body_available(&self) -> bool {
        self.full_body
    }

    /// The extension that supplies the face, or `None` where the runtime offers no face tracking.
    pub const fn face_tracking_provider(&self) -> Option<FaceTrackingProvider> {
        self.face_provider
    }

    /// Whether the runtime gives one gaze pose through `XR_EXT_eye_gaze_interaction`.
    pub const fn is_eye_gaze_interaction_available(&self) -> bool {
        self.eye_gaze_interaction
    }

    /// Whether the runtime gives the pose of each eye through `XR_FB_eye_tracking_social`.
    pub const fn is_eye_tracking_social_available(&self) -> bool {
        self.eye_tracking_social
    }
}

/// The composition local that holds the [`TrackingCapabilities`] of the session's start. Its value
/// does not change. [`XrFrame::tracking`](crate::XrFrame::tracking) follows the permissions that
/// the user grants during the session.
///
/// ```ignore
/// if local_tracking_capabilities().current().offers(TrackingFeature::Hands) {
///     Text("This headset tracks hands", Modifier::empty(), TextStyle::default());
/// }
/// ```
pub fn local_tracking_capabilities() -> CompositionLocal<TrackingCapabilities> {
    with_locals(|locals| locals.tracking_capabilities.clone())
}

#[cfg(test)]
#[path = "tests/tracking_capabilities_tests.rs"]
mod tests;
