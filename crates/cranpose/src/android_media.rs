#![allow(unsafe_code)]

use std::{
    sync::{
        Arc,
        atomic::{AtomicU32, Ordering},
    },
    time::Duration,
};

use cranpose_media::SoftwareMediaPlayer;
use cranpose_services::{
    AudioFocus, EqualizerBand, EqualizerSettings, MediaCapabilities, MediaCommand, MediaError,
    MediaItem, MediaMetadata, MediaPlayer, MediaSourceHandle, MediaSourceOpener, playback_progress,
    publish_audio_focus, publish_media_command, set_platform_media_player,
    set_platform_media_source_opener,
};
use jni::{
    EnvUnowned, jni_sig, jni_str,
    objects::{JClass, JValue},
    signature::MethodSignature,
    sys::{jfloat, jint, jlong},
};

use crate::android_jni::{clear_pending_android_jni_exception, with_android_activity_env};

const SESSION_STOPPED: jint = 0;
const SESSION_PLAYING: jint = 1;
const SESSION_PAUSED: jint = 2;

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

pub(crate) fn register(app: android_activity::AndroidApp) {
    set_platform_media_source_opener(Arc::new(DocumentOpener));
    set_platform_media_player(Arc::new(AndroidMediaPlayer {
        app,
        player: SoftwareMediaPlayer::new(),
        speed: AtomicU32::new(1.0f32.to_bits()),
    }));
}

struct DocumentOpener;

impl MediaSourceOpener for DocumentOpener {
    fn open(&self, uri: &str) -> std::io::Result<MediaSourceHandle> {
        Ok(MediaSourceHandle {
            stream: crate::android_file_picker::open_content_uri(uri)?,
            len: crate::android_file_picker::content_uri_length(uri),
        })
    }
}

struct AndroidMediaPlayer {
    app: android_activity::AndroidApp,
    player: SoftwareMediaPlayer,
    speed: AtomicU32,
}

impl AndroidMediaPlayer {
    fn call_bool(&self, name: &'static jni::strings::JNIStr, default: bool) -> bool {
        let called = with_android_activity_env(&self.app, |env, activity| {
            env.call_method(&activity, name, jni_sig!("()Z"), &[])
                .and_then(|value| value.z())
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

    fn call(
        &self,
        name: &'static jni::strings::JNIStr,
        signature: MethodSignature,
        arguments: &[JValue<'_>],
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

    fn publish_session(&self, state: jint) {
        let progress = playback_progress();
        let duration = progress
            .duration
            .map(|duration| duration.as_millis().min(jlong::MAX as u128) as jlong)
            .unwrap_or(NO_DURATION);
        self.call(
            jni_str!("cranposeMediaSessionUpdate"),
            jni_sig!("(IJJF)V"),
            &[
                JValue::Int(state),
                JValue::Long(progress.position.as_millis().min(jlong::MAX as u128) as jlong),
                JValue::Long(duration),
                JValue::Float(f32::from_bits(self.speed.load(Ordering::Relaxed)) as jfloat),
            ],
        );
    }
}

impl MediaPlayer for AndroidMediaPlayer {
    fn capabilities(&self) -> MediaCapabilities {
        MediaCapabilities {
            session: true,
            ..self.player.capabilities()
        }
    }

    fn probe_duration(&self, item: &MediaItem) -> Option<Duration> {
        self.player.probe_duration(item)
    }

    fn audio_extensions(&self) -> Vec<&'static str> {
        self.player.audio_extensions()
    }

    fn prepare(&self, item: &MediaItem) -> Result<(), MediaError> {
        self.player.prepare(item)?;
        self.publish_session(SESSION_PAUSED);
        Ok(())
    }

    fn play(&self) -> Result<(), MediaError> {
        if !self.call_bool(jni_str!("cranposeMediaRequestFocus"), true) {
            return Err(MediaError::Failed(
                "another app holds the audio output".to_owned(),
            ));
        }
        self.player.play()?;
        self.publish_session(SESSION_PLAYING);
        Ok(())
    }

    fn pause(&self) {
        self.player.pause();
        self.publish_session(SESSION_PAUSED);
    }

    fn stop(&self) {
        self.player.stop();
        self.publish_session(SESSION_STOPPED);
        self.call(jni_str!("cranposeMediaAbandonFocus"), jni_sig!("()V"), &[]);
    }

    fn seek_to(&self, position: Duration) -> Result<(), MediaError> {
        self.player.seek_to(position)?;
        self.publish_session(if cranpose_services::playback_state().is_playing() {
            SESSION_PLAYING
        } else {
            SESSION_PAUSED
        });
        Ok(())
    }

    fn set_volume(&self, volume: f32) {
        self.player.set_volume(volume);
    }

    fn set_speed(&self, speed: f32) -> bool {
        if !self.player.set_speed(speed) {
            return false;
        }
        self.speed.store(speed.to_bits(), Ordering::Relaxed);
        true
    }

    fn set_looping(&self, looping: bool) {
        self.player.set_looping(looping);
    }

    fn equalizer_bands(&self) -> Vec<EqualizerBand> {
        self.player.equalizer_bands()
    }

    fn set_equalizer(&self, settings: &EqualizerSettings) {
        self.player.set_equalizer(settings);
    }

    fn set_analysis_enabled(&self, enabled: bool) -> bool {
        self.player.set_analysis_enabled(enabled)
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

#[doc(hidden)]
#[unsafe(no_mangle)]
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

#[doc(hidden)]
#[unsafe(no_mangle)]
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
