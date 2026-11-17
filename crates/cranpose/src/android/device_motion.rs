#![expect(
    unsafe_code,
    reason = "JNI calls into the Android system and the C clock"
)]

//! The rotation sensor on Android, through `CranposeDeviceMotion`, which an
//! application carries only when its build script declares
//! `Use::device_motion`. Samples reach Rust from a sensor thread of the Java
//! class, so a busy main thread does not delay them.

use std::sync::Arc;

use cranpose_services::{
    DeviceMotionError, DeviceMotionPermission, DeviceMotionSensor, DisplayRotation, MotionClock,
    MotionConfig, Quaternion, device_motion_interrupted, publish_device_motion,
    set_platform_device_motion,
};
use jni::{
    EnvUnowned, jni_sig, jni_str,
    objects::{JClass, JValue},
    sys::{jfloat, jint, jlong},
};

use super::{
    device_motion_wire::{period_micros, reference_code, reference_of},
    jni::{call_cranpose_activity_method, call_cranpose_class},
};

const DEVICE_MOTION_CLASS: &str = "dev/cranpose/android/CranposeDeviceMotion";

pub(crate) fn register(app: android_activity::AndroidApp) {
    // Asked once: the answer does not change while the process runs, and the
    // service asks `available` on every read of the clock.
    let available = call_cranpose_class(&app, DEVICE_MOTION_CLASS, |env, activity, class| {
        env.call_static_method(
            class,
            jni_str!("cranposeDeviceMotionAvailable"),
            jni_sig!("(Landroid/content/Context;)Z"),
            &[JValue::Object(activity)],
        )
        .and_then(jni::JValueOwned::z)
    })
    .unwrap_or_else(|error| {
        log::warn!("cranpose: the rotation sensor could not be read: {error}");
        false
    });
    set_platform_device_motion(Arc::new(AndroidDeviceMotion { app, available }));
}

struct AndroidDeviceMotion {
    app: android_activity::AndroidApp,
    available: bool,
}

impl DeviceMotionSensor for AndroidDeviceMotion {
    fn clock(&self) -> MotionClock {
        // `SensorEvent.timestamp` counts on `SystemClock.elapsedRealtimeNanos()`.
        MotionClock::BootTime
    }

    #[allow(
        clippy::useless_conversion,
        reason = "time_t and a C long are i64 on a 64-bit ABI and i32 on a 32-bit one"
    )]
    fn now_ns(&self) -> u64 {
        let mut now = libc::timespec {
            tv_sec: 0,
            tv_nsec: 0,
        };
        // SAFETY: `now` is a live, correctly sized `timespec`, and
        // CLOCK_BOOTTIME is a clock Android defines.
        unsafe { libc::clock_gettime(libc::CLOCK_BOOTTIME, &mut now) };
        (i64::from(now.tv_sec) * 1_000_000_000 + i64::from(now.tv_nsec)).cast_unsigned()
    }

    fn available(&self) -> bool {
        self.available
    }

    fn permission(&self) -> DeviceMotionPermission {
        // The sensor asks nobody. `HIGH_SAMPLING_RATE_SENSORS` is a normal
        // permission, granted at install.
        DeviceMotionPermission::Granted
    }

    fn request_permission(&self) {}

    fn start(&self, config: MotionConfig) -> Result<(), DeviceMotionError> {
        let started =
            call_cranpose_class(&self.app, DEVICE_MOTION_CLASS, |env, activity, class| {
                env.call_static_method(
                    class,
                    jni_str!("cranposeDeviceMotionStart"),
                    jni_sig!("(Landroid/app/Activity;II)Z"),
                    &[
                        JValue::Object(activity),
                        JValue::Int(reference_code(config.reference)),
                        JValue::Int(period_micros(config.rate)),
                    ],
                )
                .and_then(jni::JValueOwned::z)
            })
            .map_err(DeviceMotionError::Failed)?;
        if started {
            Ok(())
        } else {
            Err(DeviceMotionError::Unavailable)
        }
    }

    fn stop(&self) {
        let _ = call_cranpose_activity_method(
            &self.app,
            DEVICE_MOTION_CLASS,
            jni_str!("cranposeDeviceMotionStop"),
        );
    }

    fn display_rotation(&self) -> DisplayRotation {
        call_cranpose_class(&self.app, DEVICE_MOTION_CLASS, |env, activity, class| {
            env.call_static_method(
                class,
                jni_str!("cranposeDeviceMotionDisplayRotation"),
                jni_sig!("(Landroid/app/Activity;)I"),
                &[JValue::Object(activity)],
            )
            .and_then(jni::JValueOwned::i)
        })
        .map_or_default(DisplayRotation::from_quarter_turns)
    }
}

#[doc(hidden)]
#[unsafe(no_mangle)]
pub(crate) extern "system" fn Java_dev_cranpose_android_CranposeDeviceMotion_nativeOnDeviceMotion(
    _env: EnvUnowned<'_>,
    _class: JClass<'_>,
    x: jfloat,
    y: jfloat,
    z: jfloat,
    w: jfloat,
    timestamp_ns: jlong,
    reference: jint,
) {
    publish_device_motion(
        Quaternion::from_rotation_vector(x, y, z, Some(w)),
        timestamp_ns.cast_unsigned(),
        reference_of(reference),
    );
}

#[doc(hidden)]
#[unsafe(no_mangle)]
pub(crate) extern "system" fn Java_dev_cranpose_android_CranposeDeviceMotion_nativeOnDeviceMotionInterrupted(
    _env: EnvUnowned<'_>,
    _class: JClass<'_>,
) {
    device_motion_interrupted();
}
