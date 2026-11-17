//! The rotation of the device, from its motion sensors, fast and with
//! timestamps.
//!
//! An application that turns a view with the phone, such as a head tracker,
//! reads this service in one of two ways.
//!
//! **Every frame, on any thread.** [`latest_device_motion`] returns the newest
//! [`MotionSample`] from a snapshot that the sensor thread writes. A call
//! allocates nothing and takes no lock. It repeats its reads only while the
//! sensor thread is in the middle of one write of a few stores.
//!
//! **State for the UI.** [`rememberDeviceMotion`] starts the sensor with the
//! composition and stops it when the call leaves the composition. Its
//! [`DeviceMotion::status`] is a [`State`](cranpose_core::State) that changes
//! only when the sensor's condition changes. [`rememberDeviceMotionSample`]
//! adds a state of the newest sample that refreshes once per frame, which
//! keeps frames coming while the sensor runs.
//!
//! # The frame of a sample
//!
//! A sample is the rotation of the device relative to a reference frame, as a
//! unit [`Quaternion`] on every platform.
//!
//! - The device frame has x to the right of the screen, y to the top of the
//!   screen and z out of the screen toward the person. It is the frame of
//!   the natural orientation of the device. The rotation of the display does
//!   not change it.
//! - The reference frame is right-handed with z up, against gravity. The x
//!   and y axes lie in the horizontal plane. With
//!   [`MotionReference::ArbitraryHeading`] their direction is chosen when the
//!   sensor starts and drifts slowly. With [`MotionReference::MagneticNorth`]
//!   y points to magnetic north.
//! - The quaternion is the active rotation that takes a device-frame vector
//!   to a reference-frame vector: `rotation.rotate(device_vector)`. A device
//!   that lies face up has the identity. A device held upright in portrait
//!   turns its y axis onto the reference z axis. The quaternions `q` and `-q`
//!   are the same rotation.
//!
//! The service does not apply the display rotation. An application on a
//! rotated display multiplies the rotation by a quarter turn about the device
//! z axis, one per step of the display's rotation.
//!
//! # Timestamps
//!
//! [`MotionSample::timestamp_ns`] is the time at which the sensor measured,
//! on the monotonic clock named by [`MotionSample::clock`]. The frame loop
//! counts on another clock. [`device_motion_now_ns`] reads the sensor's clock,
//! so `now - timestamp_ns` is the age of a sample.
//!
//! # Platforms
//!
//! - Android: `Sensor.TYPE_GAME_ROTATION_VECTOR`, or `TYPE_ROTATION_VECTOR`
//!   for the magnetic reference and on devices without the first. The
//!   application declares `cranpose_capabilities::Use::device_motion`, which
//!   brings `HIGH_SAMPLING_RATE_SENSORS` and the Java class. Samples arrive
//!   on a thread of their own, at the sensor's own rate for
//!   [`MotionRate::Fastest`].
//! - iOS: `CMMotionManager` device motion in the `xArbitraryZVertical` or
//!   `xMagneticNorthZVertical` frame, with the `cranpose` feature
//!   `device-motion`.
//! - Web: `deviceorientation` events. Safari asks the person once, after a
//!   tap: call [`request_device_motion_permission`] from a click handler.
//! - Desktop and where the build did not declare the service: the status is
//!   [`DeviceMotionStatus::Unavailable`].

mod lease;
mod quaternion;
mod remember;
mod sample;
mod sensor;
mod slot;
mod status;

pub use lease::{DeviceMotionLease, start_device_motion};
pub use quaternion::Quaternion;
pub use remember::{DeviceMotion, rememberDeviceMotion, rememberDeviceMotionSample};
pub use sample::{MotionClock, MotionConfig, MotionRate, MotionReference, MotionSample};
pub use sensor::{
    DeviceMotionSensor, DeviceMotionSensorRef, clear_platform_device_motion,
    device_motion_available, device_motion_interrupted, device_motion_now_ns,
    device_motion_permission, latest_device_motion, publish_device_motion,
    publish_device_motion_permission, request_device_motion_permission, set_platform_device_motion,
};
pub use status::{
    DeviceMotionError, DeviceMotionPermission, DeviceMotionStatus, DeviceMotionStatusObserver,
    device_motion_status, observe_device_motion_status,
};
