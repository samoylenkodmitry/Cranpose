#![expect(unsafe_code, reason = "Objective-C messages to Core Motion")]

//! The device's rotation on Apple's phones and watches through Core Motion's
//! device motion, which sensor-fuses the gyroscope and the accelerometer.
//! Apple does not state the direction of the attitude quaternion. Every sample
//! goes through `Quaternion::from_core_motion`, which assumes the rotation from
//! the reference frame to the device frame. A tilted iPhone checks the
//! assumption: `rotation.conjugate().rotate([0.0, 0.0, -1.0])` must equal the
//! `gravity` of the same `CMDeviceMotion`.
//!
//! The manager is used on the main thread only. Samples arrive on an
//! operation queue of their own, so a busy main thread does not delay them.

use std::{
    sync::{Arc, Mutex, MutexGuard, PoisonError},
    time::Duration,
};

use block2::RcBlock;
use cranpose_services::{
    DeviceMotionError, DeviceMotionPermission, DeviceMotionSensor, MotionClock, MotionConfig,
    MotionRate, MotionReference, Quaternion, publish_device_motion, set_platform_device_motion,
};
use objc2::rc::Retained;
use objc2_core_motion::{CMAttitudeReferenceFrame, CMDeviceMotion, CMMotionManager};
use objc2_foundation::{NSError, NSOperationQueue, NSProcessInfo, NSQualityOfService};

use super::mobile::on_main_after;

// What one reading of `CMMotionManager` asks for when the rate is the
// fastest: Core Motion clamps an interval to the rate the hardware has.
const FASTEST_INTERVAL_SECONDS: f64 = 0.001;

struct Held {
    manager: Retained<CMMotionManager>,
    queue: Retained<NSOperationQueue>,
}

// SAFETY: a `Held` is made, used and dropped only inside closures that run on
// the main thread, and `on_main_after` runs them there. The mutex lets a
// static hold it and is never contended.
unsafe impl Send for Held {}

static HELD: Mutex<Option<Held>> = Mutex::new(None);

fn held() -> MutexGuard<'static, Option<Held>> {
    HELD.lock().unwrap_or_else(PoisonError::into_inner)
}

pub(crate) fn register() {
    on_main_after(Duration::ZERO, |_| {
        // SAFETY: a fresh manager, asked whether the device has the sensor.
        let manager = unsafe { CMMotionManager::new() };
        // SAFETY: as above.
        let available = unsafe { manager.isDeviceMotionAvailable() };
        let queue = NSOperationQueue::new();
        queue.setMaxConcurrentOperationCount(1);
        queue.setQualityOfService(NSQualityOfService::UserInteractive);
        *held() = Some(Held { manager, queue });
        set_platform_device_motion(Arc::new(AppleDeviceMotion { available }));
    });
}

struct AppleDeviceMotion {
    available: bool,
}

impl DeviceMotionSensor for AppleDeviceMotion {
    fn clock(&self) -> MotionClock {
        // Core Motion stamps with `ProcessInfo.systemUptime`.
        MotionClock::SystemUptime
    }

    fn now_ns(&self) -> u64 {
        (NSProcessInfo::processInfo().systemUptime() * 1.0e9) as u64
    }

    fn available(&self) -> bool {
        self.available
    }

    fn permission(&self) -> DeviceMotionPermission {
        // Device motion asks nobody and needs no Info.plist entry.
        DeviceMotionPermission::Granted
    }

    fn request_permission(&self) {}

    fn start(&self, config: MotionConfig) -> Result<(), DeviceMotionError> {
        on_main_after(Duration::ZERO, move |_| begin(config));
        Ok(())
    }

    fn stop(&self) {
        on_main_after(Duration::ZERO, |_| {
            if let Some(held) = held().as_ref() {
                // SAFETY: the manager belongs to the main thread, where this runs.
                unsafe { held.manager.stopDeviceMotionUpdates() };
            }
        });
    }
}

// Starts the updates, or switches them to `config`.
fn begin(config: MotionConfig) {
    let held = held();
    let Some(held) = held.as_ref() else {
        return;
    };
    // SAFETY: a class method that reads the frames the device offers.
    let frames = unsafe { CMMotionManager::availableAttitudeReferenceFrames() };
    let (frame, reference) = match config.reference {
        MotionReference::MagneticNorth
            if frames.contains(CMAttitudeReferenceFrame::XMagneticNorthZVertical) =>
        {
            (
                CMAttitudeReferenceFrame::XMagneticNorthZVertical,
                MotionReference::MagneticNorth,
            )
        }
        _ => (
            CMAttitudeReferenceFrame::XArbitraryZVertical,
            MotionReference::ArbitraryHeading,
        ),
    };
    let interval = match config.rate {
        MotionRate::Fastest => FASTEST_INTERVAL_SECONDS,
        MotionRate::Period(period) => period.as_secs_f64().max(FASTEST_INTERVAL_SECONDS),
    };
    let handler = RcBlock::new(move |motion: *mut CMDeviceMotion, _error: *mut NSError| {
        // SAFETY: Core Motion passes a valid motion object or null, which
        // stays valid for this call.
        let Some(motion) = (unsafe { motion.as_ref() }) else {
            return;
        };
        // SAFETY: as above, on an object this handler reads and does not keep.
        let (timestamp, attitude) = unsafe { (motion.timestamp(), motion.attitude().quaternion()) };
        publish_device_motion(
            Quaternion::from_core_motion(attitude.x, attitude.y, attitude.z, attitude.w),
            (timestamp * 1.0e9) as u64,
            reference,
        );
    });
    // SAFETY: the interval is a plain number. The queue and the manager live
    // in `HELD`. The system copies the handler and calls it on the queue
    // until `stopDeviceMotionUpdates`, which `stop` and this function send
    // before another start.
    unsafe {
        held.manager.stopDeviceMotionUpdates();
        held.manager.setDeviceMotionUpdateInterval(interval);
        held.manager
            .startDeviceMotionUpdatesUsingReferenceFrame_toQueue_withHandler(
                frame,
                &held.queue,
                RcBlock::as_ptr(&handler),
            );
    }
}
