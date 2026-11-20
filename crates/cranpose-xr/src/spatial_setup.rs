//! What the app asked of anchors and planes, and what the runtime offers, decided at the start.

use crate::{
    AnchorCapabilities, TrackingAvailability,
    spatial_offer::{SpatialOffer, SpatialProvider},
    spatial_request::SpatialRequest,
    spatial_support::SpatialSupport,
};

// The anchors and planes of a session: the app's request, the extensions that the instance enabled
// for it and what the runtime offers.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct SpatialSetup {
    pub(crate) request: SpatialRequest,
    pub(crate) enabled: SpatialOffer,
    pub(crate) provider: Option<SpatialProvider>,
    pub(crate) anchors: AnchorCapabilities,
    pub(crate) planes: TrackingAvailability,
}

impl SpatialSetup {
    // What the runtime offers, given the extensions it lists, the app's request and what the
    // system said of the extensions that the instance enabled.
    //
    // Anchors need no permission: Meta's `USE_ANCHOR_API` is a normal one. Persistence on Android XR
    // and the planes of both runtimes wait for the user's answer to a prompt, so a request for them
    // starts as `AwaitingPermission`. A Quest's persistence needs no prompt, but the runtime lists the
    // stored anchors some frames after the start, so it starts as `AwaitingPermission` too, and the
    // listing turns it `Active`.
    pub(crate) fn from_runtime(
        request: SpatialRequest,
        listed: SpatialOffer,
        support: SpatialSupport,
    ) -> Self {
        let provider = listed.provider();
        let anchors = listed.offers_anchors()
            && !SpatialSupport::lacks(match provider {
                Some(SpatialProvider::Meta) => support.meta_anchors,
                _ => support.android_anchors,
            });
        let persistence = anchors
            && listed.offers_persistence()
            && !(provider == Some(SpatialProvider::Android)
                && SpatialSupport::lacks(support.android_persistence));
        let planes = anchors && listed.offers_planes();
        let level =
            |usable: bool, asked: bool, asked_level: TrackingAvailability| match (usable, asked) {
                (false, _) => TrackingAvailability::Unavailable,
                (true, false) => TrackingAvailability::Offered,
                (true, true) => asked_level,
            };
        Self {
            request,
            enabled: listed.enabled_for(request),
            provider,
            anchors: AnchorCapabilities {
                anchors: level(anchors, request.anchors, TrackingAvailability::Active),
                persistence: level(
                    persistence,
                    request.anchors,
                    TrackingAvailability::AwaitingPermission,
                ),
                max_anchors: support.android_max_anchors.filter(|_| anchors),
            },
            planes: level(
                planes,
                request.planes,
                TrackingAvailability::AwaitingPermission,
            ),
        }
    }
}

#[cfg(test)]
#[path = "tests/spatial_setup_tests.rs"]
mod tests;
