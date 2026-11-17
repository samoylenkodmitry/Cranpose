use std::time::Duration;

use super::quaternion::Quaternion;

/// The monotonic clock on which a sample's timestamp counts.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum MotionClock {
    /// Nanoseconds since the device booted, asleep time included: Android's
    /// `SystemClock.elapsedRealtimeNanos()`.
    BootTime,
    /// Nanoseconds since the device booted, asleep time left out: the
    /// `systemUptime` of Apple's platforms, in which Core Motion stamps.
    SystemUptime,
    /// Nanoseconds since the page started, from the browser's
    /// `performance.now()`.
    PerformanceNow,
}

/// What the heading of the reference frame is tied to.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum MotionReference {
    /// The heading is arbitrary and fixed when the sensor starts. It drifts
    /// slowly. The magnetic field never disturbs it, so it suits a view that
    /// follows the head.
    #[default]
    ArbitraryHeading,
    /// The y axis points to magnetic north. Iron near the device bends it.
    MagneticNorth,
}

/// How often the sensor reports.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum MotionRate {
    /// As often as the sensor can.
    #[default]
    Fastest,
    /// About this often. The sensor may report sooner.
    Period(Duration),
}

impl MotionRate {
    /// The period asked for, or `None` for the fastest rate.
    pub const fn period(self) -> Option<Duration> {
        match self {
            Self::Fastest => None,
            Self::Period(period) => Some(period),
        }
    }

    /// The rate that serves both this one and `other`.
    pub fn or_faster(self, other: Self) -> Self {
        match (self, other) {
            (Self::Period(a), Self::Period(b)) => Self::Period(a.min(b)),
            _ => Self::Fastest,
        }
    }
}

/// What a reader asks of the sensor.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub struct MotionConfig {
    /// What the heading of the reference frame is tied to.
    pub reference: MotionReference,
    /// How often the sensor reports.
    pub rate: MotionRate,
}

/// One measurement of the rotation of the device.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct MotionSample {
    /// The rotation of the device frame in the reference frame; see the
    /// [module](crate::device_motion) for the frames.
    pub rotation: Quaternion,
    /// When the sensor measured, in nanoseconds on [`clock`](Self::clock).
    pub timestamp_ns: u64,
    /// The clock of [`timestamp_ns`](Self::timestamp_ns).
    pub clock: MotionClock,
    /// The reference the sensor used. A device without the sensor for the
    /// arbitrary heading reports the magnetic one.
    pub reference: MotionReference,
    /// How many samples the sensor has produced since it started, from one.
    /// A reader that sees the same index twice has the same sample.
    pub index: u64,
}
