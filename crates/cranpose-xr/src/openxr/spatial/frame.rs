//! What a backend of anchors and planes gets in one frame.

use std::time::Duration;

use openxr as xr;

use super::{
    super::{host::Host, session::XrSession},
    events::SpatialEvent,
};
use crate::spatial_store::SpatialStore;

// The frame of a running session as the anchors and the planes read it.
pub(crate) struct Frame<'a> {
    pub(crate) xr_session: &'a XrSession,
    pub(crate) host: &'a Host,
    pub(crate) store: &'a SpatialStore,
    // The events about spatial entities that arrived since the last frame
    pub(crate) events: &'a [SpatialEvent],
    // The time on the app's monotonic clock, which paces the looks at what changes slowly
    pub(crate) now: Duration,
    // The time at which the frame's pictures reach the display, at which the poses are read
    pub(crate) time: xr::Time,
    // Whether the runtime tracks the head in this frame, which an anchor needs to be placed
    pub(crate) head_tracked: bool,
}
