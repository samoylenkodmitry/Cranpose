//! How soft content follows its target.

use std::time::Duration;

use crate::{FollowThresholds, TrackedDimensions};

/// How content follows its target in [`FollowMode::Soft`](crate::FollowMode::Soft): it stays
/// where it is while the target moves a little, and glides after the target once the target moved
/// far enough for long enough. Jetpack XR's `SoftFollowMode`, which `FollowMode.soft` makes.
///
/// The follow starts when the target stays past one of the
/// [`start_thresholds`](Self::start_thresholds) for [`start_delay`](Self::start_delay). The
/// content then covers half of its distance to the target in each
/// [`half_life`](Self::half_life), so it slows down as it nears the target, as an exponential
/// decay does. When it reaches the target it stops and waits for the thresholds again. A menu that
/// floats in front of the user thus holds still for the small moves of the head that come with
/// pointing at it, and comes back into view when the user turns away.
///
/// The first place, and the place after the runtime recenters the room, has no glide: the content
/// stands at the target at once.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SoftFollowMode {
    /// The dimensions of the target's pose that the content follows.
    pub dimensions: TrackedDimensions,
    /// The time in which the content covers half of its distance to the target. Jetpack XR's
    /// `halfLifeMillis`.
    pub half_life: Duration,
    /// The time for which the target must stay past a threshold before the follow starts.
    /// Jetpack XR's `startDelay`.
    pub start_delay: Duration,
    /// How far the target must move away from the content before the follow starts. Jetpack XR's
    /// `startThresholds`.
    pub start_thresholds: FollowThresholds,
}

impl SoftFollowMode {
    /// Jetpack XR's default half life: 175 ms.
    pub const DEFAULT_HALF_LIFE: Duration = Duration::from_millis(175);
    /// Jetpack XR's default start delay: 200 ms.
    pub const DEFAULT_START_DELAY: Duration = Duration::from_millis(200);
    /// Jetpack XR's default start thresholds: 0.2 m of distance, 16 degrees of pitch, 24 degrees
    /// of yaw and 20 degrees of roll.
    pub const DEFAULT_START_THRESHOLDS: FollowThresholds = FollowThresholds {
        translation_meters: 0.2,
        pitch_degrees: 16.0,
        yaw_degrees: 24.0,
        roll_degrees: 20.0,
    };

    /// The soft follow of `dimensions` with Jetpack XR's defaults.
    pub const fn new(dimensions: TrackedDimensions) -> Self {
        Self {
            dimensions,
            half_life: Self::DEFAULT_HALF_LIFE,
            start_delay: Self::DEFAULT_START_DELAY,
            start_thresholds: Self::DEFAULT_START_THRESHOLDS,
        }
    }
}
