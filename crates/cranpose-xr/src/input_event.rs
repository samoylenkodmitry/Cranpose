//! What a ray did at a volume.

use std::time::Duration;

use crate::{Hand, HitInfo, InputAction, InputSource};

/// What a controller's or a hand's ray did at a volume: Jetpack XR's `InputEvent`. A volume's
/// content takes it from [`SpatialVolumeScope::on_input_event`](crate::SpatialVolumeScope::on_input_event).
///
/// The ray is in the volume's content space, which has its origin at the box's center, x to the
/// right, y down and z away from the viewer, and a dp for a unit. Jetpack XR gives the ray in the
/// activity space of the receiver. Jetpack XR's `Pointer` (left, right or default) is the
/// [`Hand`]: Cranpose has no default pointer.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct InputEvent {
    /// Which input casts the ray.
    pub source: InputSource,
    /// The hand whose controller or whose tracked hand casts the ray. Jetpack XR's `pointerType`.
    pub pointer_type: Hand,
    /// The time of the frame that the event belongs to, from the start of the session's clock.
    /// Jetpack XR's is the uptime of the system in milliseconds.
    pub timestamp: Duration,
    /// Where the ray starts, in dp.
    pub origin: [f32; 3],
    /// The ray's unit direction.
    pub direction: [f32; 3],
    /// What the ray did.
    pub action: InputAction,
    /// The mesh that the ray meets, or that a press began on. `None` while the ray meets no
    /// mesh of the volume and no press is on: a volume's box that holds no mesh gives events with
    /// no hit.
    pub hit_info: Option<HitInfo>,
}
