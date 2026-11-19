//! What the app asked for and what the runtime offers, decided when the instance is made.

use crate::{
    TrackingCapabilities, extension_offer::ExtensionOffer, tracking_request::TrackingRequest,
};

// The tracking of a session: the app's request, the extensions that the instance enabled for it and
// what the runtime offers.
#[derive(Clone, Copy, Debug)]
pub(crate) struct TrackingSetup {
    pub(crate) request: TrackingRequest,
    pub(crate) enabled: ExtensionOffer,
    pub(crate) capabilities: TrackingCapabilities,
}
