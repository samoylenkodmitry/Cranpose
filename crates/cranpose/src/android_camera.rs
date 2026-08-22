//! Android Camera2 behind the framework camera service.
//!
//! The session lives in `CranposeCamera` on the Java side and pushes what it
//! produces here: preview frames as NV12 — the format the sensor already
//! produces — a still when one is asked for, and what the session is doing.
//! Nothing is polled, nothing is written to a file, and no call waits for a
//! capture: the previous transport JPEG-compressed every preview frame, wrote
//! it to the cache directory, renamed it, read it back and decoded it, and a
//! still was waited for by sleeping in twenty-millisecond steps until a marker
//! file appeared.

#![allow(unsafe_code)]

use crate::android_jni::{clear_pending_android_jni_exception, with_android_activity_env};
use cranpose_services::{
    publish_camera_frame, publish_camera_state, publish_camera_still, record_dropped_camera_frame,
    set_platform_camera, Camera, CameraError, CameraFrame, CameraLens, CameraState, CameraStill,
    FlashMode, FrameFormat,
};
use jni::objects::{JByteArray, JClass, JObject, JString, JValue};
use jni::sys::{jint, jlong};
use jni::{jni_sig, jni_str, EnvUnowned, Outcome};
use std::sync::Arc;

pub(crate) fn register(app: android_activity::AndroidApp) {
    set_platform_camera(Arc::new(AndroidCamera { app }));
}

struct AndroidCamera {
    app: android_activity::AndroidApp,
}

impl AndroidCamera {
    fn call_void(&self, name: &'static jni::strings::JNIStr) -> Result<(), String> {
        with_android_activity_env(&self.app, |env, activity| {
            env.call_method(&activity, name, jni_sig!("()V"), &[])
                .map(|_| ())
                .map_err(|error| {
                    clear_pending_android_jni_exception(env);
                    error.to_string()
                })
        })
    }

    fn call_bool(&self, name: &'static jni::strings::JNIStr) -> bool {
        with_android_activity_env(&self.app, |env, activity| {
            env.call_method(&activity, name, jni_sig!("()Z"), &[])
                .and_then(|value| value.z())
                .map_err(|error| {
                    clear_pending_android_jni_exception(env);
                    error.to_string()
                })
        })
        .unwrap_or(false)
    }

    fn call_string(&self, name: &'static jni::strings::JNIStr) -> Option<String> {
        with_android_activity_env(&self.app, |env, activity| {
            let value = env
                .call_method(&activity, name, jni_sig!("()Ljava/lang/String;"), &[])
                .and_then(|value| value.l())
                .map_err(|error| {
                    clear_pending_android_jni_exception(env);
                    error.to_string()
                })?;
            JString::cast_local(env, value)
                .map_err(|error| error.to_string())?
                .try_to_string(env)
                .map_err(|error| error.to_string())
        })
        .ok()
        .filter(|value| !value.is_empty())
    }
}

impl Camera for AndroidCamera {
    fn start(&self) -> Result<(), CameraError> {
        // Android answers a permission request on its own thread; the session
        // opens once it is granted, and the state says so when it does.
        self.call_void(jni_str!("cranposeCameraStart"))
            .map_err(CameraError::Failed)
    }

    fn stop(&self) {
        let _ = self.call_void(jni_str!("cranposeCameraStop"));
    }

    fn request_still(&self) -> Result<(), CameraError> {
        self.call_void(jni_str!("cranposeCameraRequestStill"))
            .map_err(CameraError::Failed)
    }

    fn lenses(&self) -> Vec<CameraLens> {
        self.call_string(jni_str!("cranposeCameraLenses"))
            .unwrap_or_default()
            .lines()
            .filter_map(|line| line.split_once('|'))
            .map(|(id, name)| CameraLens {
                id: id.to_string(),
                name: name.to_string(),
            })
            .collect()
    }

    fn lens(&self) -> Option<String> {
        self.call_string(jni_str!("cranposeCameraLens"))
    }

    fn use_lens(&self, id: &str) -> bool {
        with_android_activity_env(&self.app, |env, activity| {
            let id = env.new_string(id).map_err(|error| error.to_string())?;
            let id: &JObject = id.as_ref();
            env.call_method(
                &activity,
                jni_str!("cranposeCameraUseLens"),
                jni_sig!("(Ljava/lang/String;)Z"),
                &[JValue::Object(id)],
            )
            .and_then(|value| value.z())
            .map_err(|error| error.to_string())
        })
        .unwrap_or(false)
    }

    fn has_flash(&self) -> bool {
        self.call_bool(jni_str!("cranposeCameraHasFlash"))
    }

    fn set_flash(&self, mode: FlashMode) -> bool {
        let mode = match mode {
            FlashMode::Off => 0,
            FlashMode::Auto => 1,
            FlashMode::On => 2,
        };
        with_android_activity_env(&self.app, |env, activity| {
            env.call_method(
                &activity,
                jni_str!("cranposeCameraSetFlash"),
                jni_sig!("(I)Z"),
                &[JValue::Int(mode)],
            )
            .and_then(|value| value.z())
            .map_err(|error| error.to_string())
        })
        .unwrap_or(false)
    }
}

/// One preview frame, in the format the sensor produced.
#[doc(hidden)]
#[no_mangle]
pub extern "system" fn Java_dev_cranpose_android_CranposeActivity_nativeOnCameraFrame<'local>(
    mut env: EnvUnowned<'local>,
    _class: JClass<'local>,
    nv12: JByteArray<'local>,
    width: jint,
    height: jint,
    rotation_degrees: jint,
    sequence: jlong,
) {
    let decoded = env.with_env(|env| env.convert_byte_array(&nv12));
    let Outcome::Ok(bytes) = decoded.into_outcome() else {
        record_dropped_camera_frame();
        return;
    };
    let frame = CameraFrame::new(
        width.max(0) as u32,
        height.max(0) as u32,
        FrameFormat::Nv12,
        rotation_degrees.rem_euclid(360) as u16,
        sequence.max(0) as u64,
        bytes,
    );
    // A frame whose bytes do not match its size is a backend bug, and letting
    // it through reads as corrupted video rather than as a fault.
    match frame {
        Some(frame) => publish_camera_frame(frame),
        None => record_dropped_camera_frame(),
    }
}

/// A frame the device produced while the previous one was still in flight.
#[doc(hidden)]
#[no_mangle]
pub extern "system" fn Java_dev_cranpose_android_CranposeActivity_nativeOnCameraFrameDropped(
    _env: EnvUnowned<'_>,
    _class: JClass<'_>,
) {
    record_dropped_camera_frame();
}

/// What the session is doing.
#[doc(hidden)]
#[no_mangle]
pub extern "system" fn Java_dev_cranpose_android_CranposeActivity_nativeOnCameraState<'local>(
    mut env: EnvUnowned<'local>,
    _class: JClass<'local>,
    kind: jint,
    detail: JString<'local>,
) {
    let decoded = env.with_env(|env| detail.try_to_string(env));
    let Outcome::Ok(detail) = decoded.into_outcome() else {
        return;
    };
    let state = match kind {
        1 => CameraState::Running {
            device: if detail.is_empty() {
                "Back camera".to_string()
            } else {
                detail
            },
        },
        2 => CameraState::Stopped,
        3 => CameraState::Failed(CameraError::Failed(if detail.is_empty() {
            "the camera session failed".to_string()
        } else {
            detail
        })),
        _ => CameraState::Idle,
    };
    publish_camera_state(state);
}

/// A still, or the reason there is none.
#[doc(hidden)]
#[no_mangle]
pub extern "system" fn Java_dev_cranpose_android_CranposeActivity_nativeOnCameraStill<'local>(
    mut env: EnvUnowned<'local>,
    _class: JClass<'local>,
    jpeg: JByteArray<'local>,
    error: JString<'local>,
) {
    let decoded = env.with_env(|env| -> jni::errors::Result<(Option<Vec<u8>>, String)> {
        let bytes = if jpeg.is_null() {
            None
        } else {
            Some(env.convert_byte_array(&jpeg)?)
        };
        Ok((bytes, error.try_to_string(env)?))
    });
    let Outcome::Ok((bytes, error)) = decoded.into_outcome() else {
        return;
    };
    publish_camera_still(match bytes {
        Some(jpeg) if error.is_empty() => Ok(CameraStill { jpeg }),
        _ => Err(CameraError::Failed(if error.is_empty() {
            "the still capture produced no image".to_string()
        } else {
            error
        })),
    });
}
