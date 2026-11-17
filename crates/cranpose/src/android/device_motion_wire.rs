//! The numbers `CranposeDeviceMotion` and the service trade over JNI, read as
//! the service's values. Kept apart from the JNI so the host tests read them.

use cranpose_services::{MotionRate, MotionReference};

// What `nativeOnDeviceMotion` says about the sensor that produced a sample,
// and what `cranposeDeviceMotionStart` asks for.
const ARBITRARY_HEADING: i32 = 0;
const MAGNETIC_NORTH: i32 = 1;

pub(crate) fn reference_code(reference: MotionReference) -> i32 {
    match reference {
        MotionReference::ArbitraryHeading => ARBITRARY_HEADING,
        MotionReference::MagneticNorth => MAGNETIC_NORTH,
    }
}

pub(crate) fn reference_of(code: i32) -> MotionReference {
    match code {
        MAGNETIC_NORTH => MotionReference::MagneticNorth,
        _ => MotionReference::ArbitraryHeading,
    }
}

// The sampling period in microseconds that `cranposeDeviceMotionStart` takes;
// zero is the fastest rate the sensor has.
pub(crate) fn period_micros(rate: MotionRate) -> i32 {
    match rate {
        MotionRate::Fastest => 0,
        // A period under a microsecond is not the fastest rate.
        MotionRate::Period(period) => i32::try_from(period.as_micros()).unwrap_or(i32::MAX).max(1),
    }
}

#[cfg(test)]
#[path = "tests/device_motion_wire_tests.rs"]
mod tests;
