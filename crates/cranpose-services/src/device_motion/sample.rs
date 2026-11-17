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

/// How far the person has turned the device from its natural orientation, as
/// Android's `Display.getRotation()` counts it.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum DisplayRotation {
    /// The natural orientation: the device frame is the screen frame.
    #[default]
    Rotation0,
    /// A quarter turn counter-clockwise, as seen by the person: the natural
    /// top edge of the device is at the left.
    Rotation90,
    /// A half turn: the natural top edge is at the bottom.
    Rotation180,
    /// A quarter turn clockwise: the natural top edge is at the right.
    Rotation270,
}

impl DisplayRotation {
    /// The rotation for an angle in degrees, which is rounded to a quarter
    /// turn; the web's `screen.orientation.angle` has this form.
    pub const fn from_degrees(degrees: i32) -> Self {
        match degrees.rem_euclid(360) / 90 {
            1 => Self::Rotation90,
            2 => Self::Rotation180,
            3 => Self::Rotation270,
            _ => Self::Rotation0,
        }
    }

    /// The rotation for Android's `Surface.ROTATION_*` constants, which
    /// count quarter turns.
    pub const fn from_quarter_turns(turns: i32) -> Self {
        Self::from_degrees(turns.wrapping_mul(90))
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
