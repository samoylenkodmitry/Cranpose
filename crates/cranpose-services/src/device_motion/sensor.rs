use std::sync::{
    Arc,
    atomic::{AtomicU8, Ordering},
};

use super::{
    lease::refresh,
    quaternion::Quaternion,
    sample::{MotionClock, MotionConfig, MotionReference, MotionSample},
    slot::LatestSample,
    status::{
        DeviceMotionError, DeviceMotionPermission, DeviceMotionStatus, mark_acquiring, mark_live,
        set_status,
    },
};
use crate::registry::ServiceRegistry;

/// A platform's rotation sensor.
///
/// The methods run on whichever thread starts the sensor. They must return
/// without waiting for a sample, and must not call back into the functions
/// of this module that start or stop it.
pub trait DeviceMotionSensor: Send + Sync {
    /// The clock of the timestamps this sensor produces.
    fn clock(&self) -> MotionClock;
    /// The time now on [`clock`](Self::clock), in nanoseconds.
    fn now_ns(&self) -> u64;
    /// Whether the device has a rotation sensor and the build declared the
    /// service.
    fn available(&self) -> bool;
    /// Whether the application may read the sensor. Where the platform asks
    /// nobody, it is always granted.
    fn permission(&self) -> DeviceMotionPermission;
    /// Asks the person, once per call. The answer arrives through
    /// [`publish_device_motion_permission`].
    fn request_permission(&self);
    /// Starts the sensor, or switches a running one to `config`. Samples
    /// arrive through [`publish_device_motion`].
    fn start(&self, config: MotionConfig) -> Result<(), DeviceMotionError>;
    /// Stops the sensor. No sample arrives afterwards.
    fn stop(&self);
}

/// Shared handle to a [`DeviceMotionSensor`].
pub type DeviceMotionSensorRef = Arc<dyn DeviceMotionSensor>;

static PLATFORM: ServiceRegistry<dyn DeviceMotionSensor> = ServiceRegistry::new();
static LATEST: LatestSample = LatestSample::new();
static CLOCK: AtomicU8 = AtomicU8::new(0);

fn clock_code(clock: MotionClock) -> u8 {
    match clock {
        MotionClock::BootTime => 0,
        MotionClock::SystemUptime => 1,
        MotionClock::PerformanceNow => 2,
    }
}

fn platform_clock() -> MotionClock {
    match CLOCK.load(Ordering::Relaxed) {
        1 => MotionClock::SystemUptime,
        2 => MotionClock::PerformanceNow,
        _ => MotionClock::BootTime,
    }
}

pub(super) fn platform() -> Option<DeviceMotionSensorRef> {
    PLATFORM.get().filter(|sensor| sensor.available())
}

pub(super) fn latest() -> &'static LatestSample {
    &LATEST
}

/// Installs the platform sensor, replacing any previous one.
pub fn set_platform_device_motion(sensor: DeviceMotionSensorRef) {
    CLOCK.store(clock_code(sensor.clock()), Ordering::Relaxed);
    PLATFORM.set(sensor);
    refresh();
}

/// Removes the platform sensor (tests and teardown).
pub fn clear_platform_device_motion() {
    PLATFORM.clear();
    LATEST.end();
    super::lease::forget_running();
    set_status(DeviceMotionStatus::Unavailable);
}

/// Whether this device can report its rotation to this application at all.
pub fn device_motion_available() -> bool {
    platform().is_some()
}

/// Whether the application may read the sensor.
pub fn device_motion_permission() -> DeviceMotionPermission {
    platform().map_or(DeviceMotionPermission::Denied, |sensor| sensor.permission())
}

/// Asks the person for the sensor, where the platform asks at all; the web
/// on iOS does, from a tap. A no-op where nothing needs asking.
pub fn request_device_motion_permission() {
    if let Some(sensor) = platform()
        && sensor.permission() == DeviceMotionPermission::NotAsked
    {
        sensor.request_permission();
    }
}

/// The time now on the clock of [`MotionSample::timestamp_ns`], in
/// nanoseconds, or `None` without a sensor. `now - timestamp_ns` is the age
/// of a sample.
pub fn device_motion_now_ns() -> Option<u64> {
    platform().map(|sensor| sensor.now_ns())
}

/// The newest sample, or `None` while the sensor is stopped or has not
/// reported since it started.
///
/// The call allocates nothing and takes no lock, so any thread may make it
/// every frame.
pub fn latest_device_motion() -> Option<MotionSample> {
    LATEST.read()
}

/// Publishes a sample. Backends call this from whichever thread the platform
/// delivers on. A sample that arrives while the sensor is stopped is dropped.
pub fn publish_device_motion(rotation: Quaternion, timestamp_ns: u64, reference: MotionReference) {
    if LATEST.publish(rotation, timestamp_ns, reference, platform_clock()) {
        mark_live();
    }
}

/// Tells the service that the platform stopped delivering although the
/// sensor was started, as when the application is paused. Until the next
/// sample the status is [`DeviceMotionStatus::Acquiring`] and
/// [`latest_device_motion`] returns `None`.
pub fn device_motion_interrupted() {
    LATEST.interrupt();
    mark_acquiring();
}

/// Publishes the person's answer. Backends call this when the permission
/// prompt closes. A reader that waited on it starts the sensor.
pub fn publish_device_motion_permission(_granted: bool) {
    refresh();
}
