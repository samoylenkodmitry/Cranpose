//! Android media playback behind the framework media service.
//!
//! The stack lives in `CranposeMedia` on the Java side — `MediaPlayer` for the
//! decoding, `AudioManager` for focus, `MediaSession` for the lock screen,
//! `Visualizer` for the samples a visualiser draws — and pushes everything it
//! learns here. Nothing is polled: the position arrives as it moves, focus
//! arrives when the device changes its mind, and a button pressed on a lock
//! screen arrives as a command.

#![allow(unsafe_code)]

use crate::android_jni::{clear_pending_android_jni_exception, with_android_activity_env};
use cranpose_services::{
    publish_audio_focus, publish_media_command, publish_media_samples, publish_playback_progress,
    publish_playback_state, set_platform_media_player, AudioFocus, EqualizerBand,
    EqualizerSettings, MediaCapabilities, MediaCommand, MediaError, MediaItem, MediaMetadata,
    MediaPlayer, MediaSamples, PlaybackProgress, PlaybackState,
};
use jni::objects::{JByteArray, JClass, JIntArray, JString, JValue};
use jni::signature::MethodSignature;
use jni::sys::{jfloat, jint, jlong};
use jni::{jni_sig, jni_str, EnvUnowned, Outcome};
use std::sync::Arc;
use std::time::Duration;

/// State kinds; these mirror the constants on `CranposeMedia`.
const STATE_LOADING: jint = 0;
const STATE_READY: jint = 1;
const STATE_PLAYING: jint = 2;
const STATE_PAUSED: jint = 3;
const STATE_ENDED: jint = 4;
const STATE_FAILED: jint = 5;

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

pub(crate) fn register(app: android_activity::AndroidApp) {
    set_platform_media_player(Arc::new(AndroidMediaPlayer { app }));
}

struct AndroidMediaPlayer {
    app: android_activity::AndroidApp,
}

impl AndroidMediaPlayer {
    fn call(
        &self,
        name: &'static jni::strings::JNIStr,
        signature: MethodSignature<'_, '_>,
        arguments: &[JValue],
    ) {
        let called = with_android_activity_env(&self.app, |env, activity| {
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

    fn call_void(&self, name: &'static jni::strings::JNIStr) {
        self.call(name, jni_sig!("()V"), &[]);
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

    fn call_int(&self, name: &'static jni::strings::JNIStr) -> i32 {
        with_android_activity_env(&self.app, |env, activity| {
            env.call_method(&activity, name, jni_sig!("()I"), &[])
                .and_then(|value| value.i())
                .map_err(|error| {
                    clear_pending_android_jni_exception(env);
                    error.to_string()
                })
        })
        .unwrap_or(0)
    }

    fn call_int_array(&self, name: &'static jni::strings::JNIStr) -> Vec<i32> {
        with_android_activity_env(&self.app, |env, activity| {
            let returned = env
                .call_method(&activity, name, jni_sig!("()[I"), &[])
                .and_then(|value| value.l())
                .map_err(|error| {
                    clear_pending_android_jni_exception(env);
                    error.to_string()
                })?;
            let array = JIntArray::cast_local(env, returned).map_err(|error| {
                clear_pending_android_jni_exception(env);
                error.to_string()
            })?;
            let length = array.len(env).map_err(|error| {
                clear_pending_android_jni_exception(env);
                error.to_string()
            })?;
            let mut values = vec![0i32; length];
            array.get_region(env, 0, &mut values).map_err(|error| {
                clear_pending_android_jni_exception(env);
                error.to_string()
            })?;
            Ok(values)
        })
        .unwrap_or_default()
    }

    fn call_set_equalizer(&self, enabled: bool, preamp_millibels: i32, gains: &[i32]) {
        let called = with_android_activity_env(&self.app, |env, activity| {
            let array = JIntArray::new(env, gains.len()).map_err(|error| {
                clear_pending_android_jni_exception(env);
                error.to_string()
            })?;
            array.set_region(env, 0, gains).map_err(|error| {
                clear_pending_android_jni_exception(env);
                error.to_string()
            })?;
            env.call_method(
                &activity,
                jni_str!("cranposeMediaSetEqualizer"),
                jni_sig!("(ZI[I)V"),
                &[
                    JValue::Bool(enabled),
                    JValue::Int(preamp_millibels),
                    JValue::Object(&array),
                ],
            )
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

    fn call_int_with_string(&self, name: &'static jni::strings::JNIStr, value: &str) -> i32 {
        with_android_activity_env(&self.app, |env, activity| {
            let text = env.new_string(value).map_err(|error| {
                clear_pending_android_jni_exception(env);
                error.to_string()
            })?;
            env.call_method(
                &activity,
                name,
                jni_sig!("(Ljava/lang/String;)I"),
                &[JValue::Object(&text)],
            )
            .and_then(|value| value.i())
            .map_err(|error| {
                clear_pending_android_jni_exception(env);
                error.to_string()
            })
        })
        .unwrap_or(-1)
    }

    fn call_with_string(&self, name: &'static jni::strings::JNIStr, value: &str) {
        let called = with_android_activity_env(&self.app, |env, activity| {
            let text = env.new_string(value).map_err(|error| {
                clear_pending_android_jni_exception(env);
                error.to_string()
            })?;
            env.call_method(
                &activity,
                name,
                jni_sig!("(Ljava/lang/String;)V"),
                &[JValue::Object(&text)],
            )
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
}

impl MediaPlayer for AndroidMediaPlayer {
    fn capabilities(&self) -> MediaCapabilities {
        MediaCapabilities {
            seeking: true,
            speed: true,
            looping: true,
            // `Visualizer` reads the output mix, which Android treats as
            // recording: without `RECORD_AUDIO` there are no samples to give,
            // and saying so is better than publishing silence.
            analysis: self.call_bool(jni_str!("cranposeMediaSupportsAnalysis")),
            session: true,
            // `MediaMetadataRetriever` reads a container's duration without
            // opening it for playback.
            probing: true,
            // Whether there is an `AudioEffect` for it is up to the device, so
            // the answer is whether it reported any bands.
            equalizer: !self
                .call_int_array(jni_str!("cranposeMediaEqualizerBands"))
                .is_empty(),
        }
    }

    fn probe_duration(&self, item: &MediaItem) -> Option<Duration> {
        let millis = self.call_int_with_string(jni_str!("cranposeMediaProbeDurationMs"), &item.uri);
        (millis > 0).then(|| Duration::from_millis(millis as u64))
    }

    fn prepare(&self, item: &MediaItem) -> Result<(), MediaError> {
        if item.uri.is_empty() {
            return Err(MediaError::UnsupportedSource(item.uri.clone()));
        }
        self.call_with_string(jni_str!("cranposeMediaPrepare"), &item.uri);
        Ok(())
    }

    fn play(&self) -> Result<(), MediaError> {
        self.call_void(jni_str!("cranposeMediaPlay"));
        Ok(())
    }

    fn pause(&self) {
        self.call_void(jni_str!("cranposeMediaPause"));
    }

    fn stop(&self) {
        self.call_void(jni_str!("cranposeMediaStop"));
    }

    fn seek_to(&self, position: Duration) -> Result<(), MediaError> {
        let millis = position.as_millis().min(jint::MAX as u128) as jint;
        self.call(
            jni_str!("cranposeMediaSeekTo"),
            jni_sig!("(I)V"),
            &[JValue::Int(millis)],
        );
        Ok(())
    }

    fn set_volume(&self, volume: f32) {
        self.call(
            jni_str!("cranposeMediaSetVolume"),
            jni_sig!("(F)V"),
            &[JValue::Float(volume.clamp(0.0, 1.0) as jfloat)],
        );
    }

    fn set_speed(&self, speed: f32) -> bool {
        self.call(
            jni_str!("cranposeMediaSetSpeed"),
            jni_sig!("(F)V"),
            &[JValue::Float(speed as jfloat)],
        );
        true
    }

    fn set_looping(&self, looping: bool) {
        self.call(
            jni_str!("cranposeMediaSetLooping"),
            jni_sig!("(Z)V"),
            &[JValue::Bool(looping)],
        );
    }

    fn equalizer_bands(&self) -> Vec<EqualizerBand> {
        // The device reports its own centres and its own range; a band that is
        // not there is not reported, so a screen draws what exists.
        let range_db = self.call_int(jni_str!("cranposeMediaEqualizerRange")) as f32 / 100.0;
        self.call_int_array(jni_str!("cranposeMediaEqualizerBands"))
            .into_iter()
            .map(|center_hz| EqualizerBand::new(center_hz as f32, range_db))
            .collect()
    }

    fn set_equalizer(&self, settings: &EqualizerSettings) {
        // `AudioEffect` works in millibels, a hundredth of a decibel.
        let gains: Vec<i32> = settings
            .gains_db
            .iter()
            .map(|gain| (gain * 100.0).round() as i32)
            .collect();
        self.call_set_equalizer(
            settings.enabled,
            (settings.preamp_db * 100.0).round() as i32,
            &gains,
        );
    }

    fn set_analysis_enabled(&self, enabled: bool) -> bool {
        if enabled && !self.call_bool(jni_str!("cranposeMediaSupportsAnalysis")) {
            return false;
        }
        self.call(
            jni_str!("cranposeMediaSetAnalysisEnabled"),
            jni_sig!("(Z)V"),
            &[JValue::Bool(enabled)],
        );
        true
    }

    fn set_session_metadata(&self, metadata: &MediaMetadata) {
        let called = with_android_activity_env(&self.app, |env, activity| {
            let title = env.new_string(&metadata.title).map_err(|error| {
                clear_pending_android_jni_exception(env);
                error.to_string()
            })?;
            let artist = env.new_string(&metadata.artist).map_err(|error| {
                clear_pending_android_jni_exception(env);
                error.to_string()
            })?;
            env.call_method(
                &activity,
                jni_str!("cranposeMediaSetMetadata"),
                jni_sig!("(Ljava/lang/String;Ljava/lang/String;)V"),
                &[JValue::Object(&title), JValue::Object(&artist)],
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
}

/// A duration Java reported, or `None` for the negative it uses to mean "this
/// item has no length" — a live stream, or an item whose header has not been
/// read yet.
fn optional_millis(millis: jlong) -> Option<Duration> {
    (millis > 0).then(|| Duration::from_millis(millis as u64))
}

/// What the media stack is doing.
#[doc(hidden)]
#[no_mangle]
pub extern "system" fn Java_dev_cranpose_android_CranposeActivity_nativeOnMediaState<'local>(
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
        STATE_LOADING => PlaybackState::Loading,
        STATE_READY | STATE_PAUSED => PlaybackState::Paused,
        STATE_PLAYING => PlaybackState::Playing,
        STATE_ENDED => PlaybackState::Ended,
        STATE_FAILED => PlaybackState::Failed(MediaError::Failed(if detail.is_empty() {
            "the media stack failed".to_string()
        } else {
            detail
        })),
        _ => PlaybackState::Idle,
    };
    publish_playback_state(state);
}

/// Where the open item is.
#[doc(hidden)]
#[no_mangle]
pub extern "system" fn Java_dev_cranpose_android_CranposeActivity_nativeOnMediaProgress(
    _env: EnvUnowned<'_>,
    _class: JClass<'_>,
    position_ms: jlong,
    duration_ms: jlong,
    buffered_ms: jlong,
) {
    let duration = optional_millis(duration_ms);
    publish_playback_progress(PlaybackProgress {
        position: Duration::from_millis(position_ms.max(0) as u64),
        duration,
        buffered: optional_millis(buffered_ms)
            .or(duration)
            .unwrap_or_default(),
    });
}

/// What the rest of the device is doing with the output.
#[doc(hidden)]
#[no_mangle]
pub extern "system" fn Java_dev_cranpose_android_CranposeActivity_nativeOnMediaAudioFocus(
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

/// A button pressed on the lock screen, the notification or a headset.
#[doc(hidden)]
#[no_mangle]
pub extern "system" fn Java_dev_cranpose_android_CranposeActivity_nativeOnMediaCommand(
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
        COMMAND_SEEK => MediaCommand::SeekTo(Duration::from_millis(position_ms.max(0) as u64)),
        _ => return,
    });
}

/// One block of waveform for a visualiser.
///
/// `Visualizer` delivers unsigned 8-bit samples centred on 128; the framework
/// publishes samples in `[-1, 1]`, so they are centred and scaled here rather
/// than in every application that draws one.
#[doc(hidden)]
#[no_mangle]
pub extern "system" fn Java_dev_cranpose_android_CranposeActivity_nativeOnMediaSamples<'local>(
    mut env: EnvUnowned<'local>,
    _class: JClass<'local>,
    waveform: JByteArray<'local>,
    sample_rate: jint,
) {
    let decoded = env.with_env(|env| env.convert_byte_array(&waveform));
    let Outcome::Ok(bytes) = decoded.into_outcome() else {
        return;
    };
    let samples: Vec<f32> = bytes
        .iter()
        .map(|byte| (*byte as u8 as f32 - 128.0) / 128.0)
        .collect();
    let sequence = next_sample_sequence();
    if let Some(samples) = MediaSamples::new(sample_rate.max(0) as u32, 1, sequence, samples) {
        publish_media_samples(samples);
    }
}

fn next_sample_sequence() -> u64 {
    use std::sync::atomic::{AtomicU64, Ordering};
    static SEQUENCE: AtomicU64 = AtomicU64::new(0);
    SEQUENCE.fetch_add(1, Ordering::AcqRel) + 1
}
