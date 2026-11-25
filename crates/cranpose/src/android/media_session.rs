//! Shared Android `AudioManager` focus and `MediaSession` JNI bridge used by both
//! `android::media` (`MediaPlayer`) and `android::player` (`Player`).
#![expect(
    unsafe_code,
    reason = "JNI calls and exports for Android MediaSession and AudioFocus"
)]

use std::{sync::OnceLock, time::Duration};

use cranpose_services::{
    AudioFocus, MediaCommand, MediaMetadata, publish_audio_focus, publish_media_command,
};
use jni::{
    EnvUnowned, jni_sig, jni_str,
    objects::{JClass, JValue},
    signature::MethodSignature,
    sys::{jfloat, jint, jlong},
};

use super::jni::{clear_pending_android_jni_exception, with_android_activity_env};

pub(crate) const SESSION_STOPPED: jint = 0;
pub(crate) const SESSION_PLAYING: jint = 1;
pub(crate) const SESSION_PAUSED: jint = 2;

const FOCUS_GAINED: jint = 0;
const FOCUS_DUCKED: jint = 1;
const FOCUS_LOST_TRANSIENT: jint = 2;
const FOCUS_LOST: jint = 3;

const COMMAND_PLAY: jint = 0;
const COMMAND_PAUSE: jint = 1;
const COMMAND_TOGGLE: jint = 2;
const COMMAND_STOP: jint = 3;
const COMMAND_NEXT: jint = 4;
const COMMAND_PREVIOUS: jint = 5;
const COMMAND_SEEK: jint = 6;

const NO_DURATION: jlong = -1;

static ACTIVITY: OnceLock<android_activity::AndroidApp> = OnceLock::new();

pub(crate) fn set_activity(app: android_activity::AndroidApp) {
    drop(ACTIVITY.set(app));
}

pub(crate) fn request_focus() -> bool {
    let Some(app) = ACTIVITY.get() else {
        return true;
    };
    call_bool(app, jni_str!("cranposeMediaRequestFocus"), true)
}

pub(crate) fn abandon_focus() {
    let Some(app) = ACTIVITY.get() else {
        return;
    };
    call_void(
        app,
        jni_str!("cranposeMediaAbandonFocus"),
        jni_sig!("()V"),
        &[],
    );
}

pub(crate) fn update_session(
    state: jint,
    position: Duration,
    duration: Option<Duration>,
    speed: f32,
) {
    let Some(app) = ACTIVITY.get() else {
        return;
    };
    let pos_ms = i64::try_from(position.as_millis()).unwrap_or(i64::MAX);
    let dur_ms = duration.map_or(NO_DURATION, |d| {
        i64::try_from(d.as_millis()).unwrap_or(i64::MAX)
    });
    call_void(
        app,
        jni_str!("cranposeMediaSessionUpdate"),
        jni_sig!("(IJJF)V"),
        &[
            JValue::Int(state),
            JValue::Long(pos_ms),
            JValue::Long(dur_ms),
            JValue::Float(speed as jfloat),
        ],
    );
}

pub(crate) fn set_metadata(metadata: &MediaMetadata) {
    let Some(app) = ACTIVITY.get() else {
        return;
    };
    let called = with_android_activity_env(app, |env, activity| {
        let title = env.new_string(&metadata.title).map_err(|error| {
            clear_pending_android_jni_exception(env);
            error.to_string()
        })?;
        let artist = env.new_string(&metadata.artist).map_err(|error| {
            clear_pending_android_jni_exception(env);
            error.to_string()
        })?;
        let album = env
            .new_string(&metadata.album)
            .map_err(|error| error.to_string())?;
        let artwork = env
            .byte_array_from_slice(metadata.artwork.as_ref().map_or(&[], |art| &art.bytes))
            .map_err(|error| error.to_string())?;
        env.call_method(
            &activity,
            jni_str!("cranposeMediaSetMetadata"),
            jni_sig!("(Ljava/lang/String;Ljava/lang/String;Ljava/lang/String;[B)V"),
            &[
                JValue::Object(&title),
                JValue::Object(&artist),
                JValue::Object(&album),
                JValue::Object(&artwork),
            ],
        )
        .map(|_| ())
        .map_err(|error| {
            clear_pending_android_jni_exception(env);
            error.to_string()
        })
    });
    if let Err(error) = called {
        log::warn!("cranpose: android media metadata failed: {error}");
    }
}

fn call_bool(
    app: &android_activity::AndroidApp,
    name: &'static jni::strings::JNIStr,
    default: bool,
) -> bool {
    let called = with_android_activity_env(app, |env, activity| {
        env.call_method(&activity, name, jni_sig!("()Z"), &[])
            .and_then(jni::JValueOwned::z)
            .map_err(|error| {
                clear_pending_android_jni_exception(env);
                error.to_string()
            })
    });
    match called {
        Ok(value) => value,
        Err(error) => {
            log::warn!("cranpose: android media call failed: {error}");
            default
        }
    }
}

fn call_void(
    app: &android_activity::AndroidApp,
    name: &'static jni::strings::JNIStr,
    signature: MethodSignature<'static, 'static>,
    arguments: &[JValue<'_>],
) {
    let called = with_android_activity_env(app, |env, activity| {
        env.call_method(&activity, name, signature, arguments)
            .map(|_| ())
            .map_err(|error| {
                clear_pending_android_jni_exception(env);
                error.to_string()
            })
    });
    if let Err(error) = called {
        log::warn!("cranpose: android media call failed: {error}");
    }
}

#[doc(hidden)]
#[unsafe(no_mangle)]
pub(crate) extern "system" fn Java_dev_cranpose_android_CranposeActivity_nativeOnMediaAudioFocus(
    _env: EnvUnowned<'_>,
    _class: JClass<'_>,
    focus: jint,
) {
    publish_audio_focus(match focus {
        FOCUS_DUCKED => AudioFocus::Ducked,
        FOCUS_LOST_TRANSIENT => AudioFocus::LostTransient,
        FOCUS_LOST => AudioFocus::Lost,
        FOCUS_GAINED => AudioFocus::Gained,
        _ => return,
    });
}

#[doc(hidden)]
#[unsafe(no_mangle)]
pub(crate) extern "system" fn Java_dev_cranpose_android_CranposeActivity_nativeOnMediaCommand(
    _env: EnvUnowned<'_>,
    _class: JClass<'_>,
    command: jint,
    position_ms: jlong,
) {
    publish_media_command(match command {
        COMMAND_PLAY => MediaCommand::Play,
        COMMAND_PAUSE => MediaCommand::Pause,
        COMMAND_TOGGLE => MediaCommand::TogglePlayPause,
        COMMAND_STOP => MediaCommand::Stop,
        COMMAND_NEXT => MediaCommand::Next,
        COMMAND_PREVIOUS => MediaCommand::Previous,
        COMMAND_SEEK => {
            MediaCommand::SeekTo(Duration::from_millis(position_ms.max(0).cast_unsigned()))
        }
        _ => return,
    });
}
