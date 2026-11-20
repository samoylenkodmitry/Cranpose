//! The anchors and planes that the app asked for in its build.

use cranpose::capabilities::{Capabilities, Service};

// What the app declared with `Use::anchors()` and `Use::plane_tracking()`.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) struct SpatialRequest {
    pub(crate) anchors: bool,
    pub(crate) planes: bool,
}

impl SpatialRequest {
    pub(crate) fn of(capabilities: &Capabilities<'_>) -> Self {
        Self {
            anchors: capabilities.has(Service::Anchors),
            planes: capabilities.has(Service::PlaneTracking),
        }
    }

    // Whether the app asked for either.
    pub(crate) const fn asks_any(self) -> bool {
        self.anchors || self.planes
    }
}
