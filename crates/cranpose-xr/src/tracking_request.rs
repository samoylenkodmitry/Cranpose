//! The tracking that the app asked for in its build.

use cranpose::capabilities::{Capabilities, Service};

use crate::TrackingFeature;

// The kinds of tracking that the app declared with `Use::hand_tracking()` and its siblings.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) struct TrackingRequest {
    asked: [bool; TrackingFeature::ALL.len()],
}

impl TrackingRequest {
    pub(crate) fn of(capabilities: &Capabilities<'_>) -> Self {
        let mut request = Self::default();
        for feature in TrackingFeature::ALL {
            request.asked[feature.index()] = capabilities.has(service_of(feature));
        }
        request
    }

    pub(crate) const fn asks(self, feature: TrackingFeature) -> bool {
        self.asked[feature.index()]
    }

    // Whether the app asked for any kind.
    pub(crate) fn asks_any(self) -> bool {
        self.asked.contains(&true)
    }
}

// The build service whose declaration asks for the feature.
pub(crate) const fn service_of(feature: TrackingFeature) -> Service {
    match feature {
        TrackingFeature::Hands => Service::HandTracking,
        TrackingFeature::Body => Service::BodyTracking,
        TrackingFeature::Face => Service::FaceTracking,
        TrackingFeature::Eyes => Service::EyeTracking,
    }
}
