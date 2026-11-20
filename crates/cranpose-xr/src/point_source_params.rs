//! How a point source sounds.

use cranpose::DistanceAttenuation;

/// How a point source in a subspace sounds. Jetpack XR's `PointSourceParams`.
///
/// Jetpack XR's version also sets the directivity of the source and its spread. Cranpose's
/// panner has neither. A source is a point that sounds the same in every direction.
///
/// ```
/// use cranpose::DistanceAttenuation;
/// use cranpose_xr::PointSourceParams;
///
/// let params = PointSourceParams::new().distance_attenuation(DistanceAttenuation::Linear {
///     min_distance: 1.0,
///     max_distance: 8.0,
///     gain_at_max_distance: 0.0,
/// });
/// ```
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct PointSourceParams {
    /// How the level falls with the distance from the head. Jetpack XR's
    /// `PointSourceParams.distanceAttenuation`.
    pub distance_attenuation: DistanceAttenuation,
}

impl PointSourceParams {
    /// A source whose level falls as [`DistanceAttenuation::Auto`] says.
    pub const fn new() -> Self {
        Self {
            distance_attenuation: DistanceAttenuation::Auto,
        }
    }

    /// Sets how the level falls with the distance.
    pub const fn distance_attenuation(mut self, distance_attenuation: DistanceAttenuation) -> Self {
        self.distance_attenuation = distance_attenuation;
        self
    }
}
